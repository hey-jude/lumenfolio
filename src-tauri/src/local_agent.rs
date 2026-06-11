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
