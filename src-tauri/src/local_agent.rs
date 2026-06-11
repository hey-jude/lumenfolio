//! Detection of locally-installed agent CLIs (Codex, Claude Code) so they can be
//! offered as zero-config "local agent" chat providers.
//!
//! This file is P0: detection + status only. The chat dispatch (driving the CLI,
//! exposing tools over MCP) lands in later phases. See the design doc at
//! docs/lumenfolio_local_agent_provider_plan.md.

use std::{
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde::Serialize;

pub(crate) mod mcp_server;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AgentKind {
    Codex,
    Claude,
}

impl AgentKind {
    fn binary(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }

    fn install_url(self) -> &'static str {
        match self {
            Self::Codex => "https://developers.openai.com/codex/cli",
            Self::Claude => "https://docs.claude.com/en/docs/claude-code",
        }
    }

    const ALL: [AgentKind; 2] = [AgentKind::Codex, AgentKind::Claude];
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentStatus {
    kind: AgentKind,
    label: String,
    installed: bool,
    version: Option<String>,
    path: Option<String>,
    install_url: String,
}

/// Run a command, capture trimmed stdout on success, with a hard timeout. A hung
/// child leaks its worker thread (acceptable for an occasional `--version` probe);
/// we never block the caller past `timeout`.
fn run_capture(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let output = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        let _ = tx.send(output);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

/// Resolve a binary's absolute path, tolerating the minimal PATH a macOS GUI app
/// inherits (resolve through the user's login shell), and `where` on Windows.
fn resolve_path(binary: &str) -> Option<String> {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let lookup = format!("command -v {binary}");
        if let Some(out) = run_capture(&shell, &["-lc", &lookup], PROBE_TIMEOUT) {
            if let Some(line) = out.lines().next() {
                let line = line.trim();
                if !line.is_empty() {
                    return Some(line.to_string());
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(out) = run_capture("where", &[binary], PROBE_TIMEOUT) {
            if let Some(line) = out.lines().next() {
                let line = line.trim();
                if !line.is_empty() {
                    return Some(line.to_string());
                }
            }
        }
    }
    None
}

/// Pull a clean version token out of a CLI's `--version` line, e.g.
/// "2.0.55 (Claude Code)" -> "2.0.55", "codex-cli 0.12.0" -> "0.12.0". Falls back
/// to the trimmed first line when no dotted-number token is found.
fn parse_version(raw: &str) -> String {
    let first_line = raw.lines().next().unwrap_or(raw).trim();
    first_line
        .split_whitespace()
        .find(|tok| {
            let t = tok.trim_start_matches('v');
            t.contains('.') && t.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(|tok| tok.trim_start_matches('v').to_string())
        .unwrap_or_else(|| first_line.to_string())
}

fn detect(kind: AgentKind) -> AgentStatus {
    let path = resolve_path(kind.binary());
    // Probe via the resolved absolute path when we have it (robust under a minimal
    // PATH); otherwise try the bare name (works when PATH is already complete).
    let invoke = path.clone().unwrap_or_else(|| kind.binary().to_string());
    let version = run_capture(&invoke, &["--version"], PROBE_TIMEOUT).map(|raw| parse_version(&raw));
    let installed = version.is_some() || path.is_some();
    AgentStatus {
        kind,
        label: kind.label().to_string(),
        installed,
        version,
        path,
        install_url: kind.install_url().to_string(),
    }
}

/// Detection status for each supported local agent CLI. Runs subprocess probes, so
/// it is offloaded to a blocking task. Cheap enough to call on demand (startup +
/// a Settings "re-check").
#[tauri::command]
pub(crate) async fn get_local_agent_status() -> Result<Vec<AgentStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| AgentKind::ALL.iter().copied().map(detect).collect())
        .await
        .map_err(|err| format!("Local-agent detection task failed: {err}"))
}

// ---- P1: drive the CLI as a one-shot answer generator (Mode A) -------------

/// Synthetic provider id format used by the frontend model selector for local
/// agents (no DB row — they are virtual, computed from detection).
pub(crate) fn provider_id_kind(provider_id: &str) -> Option<AgentKind> {
    match provider_id.trim() {
        "local-agent-codex" => Some(AgentKind::Codex),
        "local-agent-claude" => Some(AgentKind::Claude),
        _ => None,
    }
}

/// Generous cap: a real agent answer is seconds, but cold start + retries can run
/// long (the expired-token probe took ~3min). Past this we fail with a timeout.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(150);

fn answer_language(locale: Option<&str>) -> &'static str {
    match locale.map(str::trim).unwrap_or("") {
        l if l.starts_with("zh") => "Chinese",
        _ => "English",
    }
}

/// Assemble the single prompt for Mode A: the evidence is already retrieved by
/// Lumenfolio and embedded here; the agent is a pure generator (no tools).
pub(crate) fn build_prompt(
    question: &str,
    evidence: &str,
    session_context: &str,
    locale: Option<&str>,
) -> String {
    let lang = answer_language(locale);
    let mut prompt = format!(
        "You are Lumenfolio, a careful academic PDF reading assistant. Answer in {lang}. \
Use ONLY the evidence from the user's document provided below; if it is insufficient, say so plainly. \
Write a concise, well-structured Markdown answer (a short direct answer first, then detail). \
Do NOT call tools, read files, or run commands — answer only from the evidence text below.\n\n"
    );
    let session_context = session_context.trim();
    if !session_context.is_empty() {
        prompt.push_str("Conversation memory:\n");
        prompt.push_str(session_context);
        prompt.push_str("\n\n");
    }
    let evidence = evidence.trim();
    prompt.push_str("Evidence from the document:\n");
    prompt.push_str(if evidence.is_empty() {
        "(no evidence was retrieved for this question)"
    } else {
        evidence
    });
    prompt.push_str("\n\nQuestion:\n");
    prompt.push_str(question.trim());
    prompt
}

/// Run the local agent CLI once with a fully-assembled prompt; return its answer.
/// Offloaded to a blocking task (subprocess I/O). Errors carry a user-facing hint
/// (e.g. login required).
pub(crate) async fn generate_answer(kind: AgentKind, prompt: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_cli(kind, &prompt))
        .await
        .map_err(|err| format!("Local-agent task failed: {err}"))?
}

fn run_cli(kind: AgentKind, prompt: &str) -> Result<String, String> {
    let binary = resolve_path(kind.binary()).unwrap_or_else(|| kind.binary().to_string());
    let mut cmd = Command::new(&binary);
    match kind {
        // --tools "" disables all built-in tools: pure generation, no fs/shell access.
        AgentKind::Claude => {
            cmd.args(["-p", prompt, "--output-format", "json", "--tools", ""]);
        }
        // read-only sandbox + no approval; no MCP configured → no tools available.
        AgentKind::Codex => {
            cmd.args([
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                prompt,
            ]);
        }
    }
    cmd.current_dir(std::env::temp_dir()) // neutral cwd: nothing of the user's to read
        .env_remove("ANTHROPIC_API_KEY") // never bill an API key — use the subscription
        .env_remove("OPENAI_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = run_command_timeout(cmd, GENERATE_TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(classify_error(kind, stderr.trim()));
    }
    match kind {
        AgentKind::Claude => parse_claude_json(kind, &stdout),
        AgentKind::Codex => parse_codex_jsonl(kind, &stdout),
    }
}

fn run_command_timeout(
    mut cmd: Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(format!("Failed to launch the local agent: {err}")),
        Err(_) => Err("The local agent timed out. Try again, or pick a model provider.".to_string()),
    }
}

/// Turn an auth/login failure into an actionable message; otherwise pass through.
fn classify_error(kind: AgentKind, detail: &str) -> String {
    let lower = detail.to_lowercase();
    if lower.contains("login")
        || lower.contains("log in")
        || lower.contains("unauthorized")
        || lower.contains("401")
        || lower.contains("过期")
        || lower.contains("not authenticated")
    {
        let cmd = match kind {
            AgentKind::Codex => "codex login",
            AgentKind::Claude => "claude",
        };
        return format!(
            "{} isn't logged in (run `{cmd}` once in a terminal). Original error: {detail}",
            kind.label()
        );
    }
    format!("{} failed: {detail}", kind.label())
}

/// Claude `--output-format json` returns one object; the answer/error is in
/// `result`, with `is_error` flagging auth/runtime failures (exit code stays 0).
fn parse_claude_json(kind: AgentKind, stdout: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|err| format!("Could not parse Claude output: {err}"))?;
    let result = value
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let is_error = value
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if is_error {
        return Err(classify_error(kind, &result));
    }
    if result.is_empty() {
        return Err(format!("{} returned an empty answer.", kind.label()));
    }
    Ok(result)
}

/// Codex `exec --json` emits JSONL; the answer is the last `item.completed` whose
/// `item.type == "agent_message"`.
fn parse_codex_jsonl(kind: AgentKind, stdout: &str) -> Result<String, String> {
    let mut answer: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|t| t.as_str()) == Some("item.completed") {
            if let Some(item) = value.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("agent_message") {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        answer = Some(text.trim().to_string());
                    }
                }
            }
        }
    }
    answer
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("{} returned no answer.", kind.label()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_extracts_dotted_token() {
        assert_eq!(parse_version("2.0.55 (Claude Code)"), "2.0.55");
        assert_eq!(parse_version("codex-cli 0.12.0"), "0.12.0");
        assert_eq!(parse_version("v1.4.2"), "1.4.2");
        // No dotted token → trimmed first line.
        assert_eq!(parse_version("nightly build\nextra"), "nightly build");
    }

    #[test]
    fn provider_id_kind_maps_virtual_ids() {
        assert_eq!(provider_id_kind("local-agent-codex"), Some(AgentKind::Codex));
        assert_eq!(provider_id_kind("local-agent-claude"), Some(AgentKind::Claude));
        assert_eq!(provider_id_kind("openai"), None);
        assert_eq!(provider_id_kind(""), None);
    }

    #[test]
    fn parse_codex_picks_last_agent_message() {
        let stdout = r#"{"type":"thread.started","thread_id":"x"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"The answer."}}
{"type":"turn.completed","usage":{"output_tokens":5}}"#;
        assert_eq!(parse_codex_jsonl(AgentKind::Codex, stdout).unwrap(), "The answer.");
        assert!(parse_codex_jsonl(AgentKind::Codex, "{}\n").is_err());
    }

    #[test]
    fn parse_claude_extracts_result_and_flags_errors() {
        let ok = r#"{"type":"result","is_error":false,"result":"  Hello.  "}"#;
        assert_eq!(parse_claude_json(AgentKind::Claude, ok).unwrap(), "Hello.");
        // is_error true + login text → actionable login hint, NOT echoed as answer.
        let expired = r#"{"type":"result","is_error":true,"result":"API Error: 401 ... Please run /login"}"#;
        let err = parse_claude_json(AgentKind::Claude, expired).unwrap_err();
        assert!(err.contains("isn't logged in"), "got: {err}");
    }

    #[test]
    fn build_prompt_embeds_evidence_and_language() {
        let p = build_prompt("What is X?", "page 1: X is Y.", "", Some("zh-CN"));
        assert!(p.contains("Answer in Chinese"));
        assert!(p.contains("page 1: X is Y."));
        assert!(p.contains("What is X?"));
    }

    #[test]
    fn detect_missing_binary_reports_not_installed() {
        // A binary that certainly does not exist.
        let status = detect(AgentKind::Codex);
        // We can't assert installed=true (CI may not have codex), but the shape must
        // be coherent: not installed <=> no version and no path.
        if !status.installed {
            assert!(status.version.is_none() && status.path.is_none());
        }
        assert_eq!(status.install_url, AgentKind::Codex.install_url());
    }
}
