//! Unified native tool-calling agent loop (P1 of the unified-loop refactor).
//!
//! This is the modern, codex/opencode-style replacement for the fragmented
//! "M4 judge → separate answer generation" pipeline: ONE growing message history
//! that the model always sees in full. The model decides which retrieval tools to
//! call (via the provider's native `tools`/`tool_calls`), the tool results are
//! appended back into the SAME history, and the loop ends when the model stops
//! requesting tools and writes the answer. Because retrieval and answering share
//! one context, the model has true global awareness of everything it explored.
//!
//! Design choice (deliberate, to avoid the biggest pit): the tool-DECISION rounds
//! are NON-streaming (robust JSON parsing of `tool_calls`), while the FINAL answer
//! reuses the proven streaming reader (`read_openai_answer_stream`, with its
//! ThinkSplitter + SSE handling) so the UI still streams tokens. Both phases send
//! the identical full message history.
//!
//! Routing is gated by the model's `tool_call` capability (models.dev profile).
//! Models without native tool-calling fall back to the existing M3+M4 path, so
//! weak/local models keep working unchanged.

use std::time::Duration;

use serde::Deserialize;
use tauri::Emitter;

use crate::runtime::agent::{tool_call_event, tool_result_event};
use crate::{
    llm, normalize_base_url, optional_non_empty, runtime, truncate_for_error,
    AgentActivityEventOutput, AskAnswerResult, AskDocumentInput, OpenAiCompatibleProvider,
};

/// Per tool-round HTTP timeout. Generous: a round may run an expensive retrieval
/// plan server-side before the provider responds.
const UNIFIED_TOOL_ROUND_TIMEOUT_SECS: u64 = 90;
/// Default upper bound on tool-calling rounds before we force a final answer.
const DEFAULT_MAX_TOOL_ROUNDS: u32 = 8;
/// Hard clamp so a stray `max_retrieval_steps` can't run the loop forever.
const MAX_TOOL_ROUNDS_CLAMP: u32 = 12;

pub(crate) struct UnifiedLoopInput<'a> {
    pub(crate) input: &'a AskDocumentInput,
    pub(crate) database: &'a crate::AppDatabase,
    pub(crate) app: &'a tauri::AppHandle,
    pub(crate) question: &'a str,
    pub(crate) document_id: &'a str,
    /// Dispatch whitelist for cross-document `documentId` tool routing.
    pub(crate) visible_document_ids: &'a [&'a str],
    /// Pre-rendered workspace manifest (titles/dirs/abstracts) — the model's
    /// "library view", injected so it can answer library questions and route
    /// cross-document tool calls.
    pub(crate) workspace_manifest: &'a str,
    /// Session memory block (recent turns / selection), already rendered.
    pub(crate) session_context: &'a str,
    pub(crate) provider: &'a OpenAiCompatibleProvider,
    pub(crate) activity_event_id: Option<&'a str>,
}

/// Whether to route this turn through the unified loop. Requires a model that the
/// catalog marks as supporting native tool-calling and a non-image question
/// (image questions keep the existing vision path).
pub(crate) fn should_use_unified_loop(
    provider: &OpenAiCompatibleProvider,
    input: &AskDocumentInput,
) -> bool {
    if provider.model_profile.tool_call != Some(true) {
        return false;
    }
    let has_image = input
        .image_data_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    !has_image
}

// ---- Non-streaming tool-round response shapes -----------------------------

#[derive(Deserialize)]
struct ToolRoundResponse {
    #[serde(default)]
    choices: Vec<ToolRoundChoice>,
}

#[derive(Deserialize)]
struct ToolRoundChoice {
    message: ToolRoundMessage,
}

#[derive(Deserialize)]
struct ToolRoundMessage {
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallEntry>>,
}

#[derive(Deserialize, Clone)]
struct ToolCallEntry {
    #[serde(default)]
    id: String,
    function: ToolCallFunction,
}

#[derive(Deserialize, Clone)]
struct ToolCallFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

pub(crate) async fn run_unified_agent_loop(
    ctx: UnifiedLoopInput<'_>,
    agent_run: &mut runtime::agent::AgentRunResult,
) -> Result<AskAnswerResult, String> {
    let vision_enabled = ctx
        .provider
        .capabilities
        .iter()
        .any(|capability| capability == "vision");
    let rag_capabilities = runtime::rag::RagToolCapabilities {
        vision_enabled,
        max_quote_chars: agent_run.retrieval_run.context_budget.max_quote_chars,
    };
    let tools = build_openai_tools(vision_enabled);
    let mut messages = build_initial_messages(&ctx, agent_run);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(UNIFIED_TOOL_ROUND_TIMEOUT_SECS))
        .build()
        .map_err(|err| format!("Failed to create agent loop client: {err}"))?;
    let endpoint = format!(
        "{}/chat/completions",
        normalize_base_url(&ctx.provider.base_url)
    );
    let max_rounds = ctx
        .input
        .max_retrieval_steps
        .unwrap_or(DEFAULT_MAX_TOOL_ROUNDS)
        .clamp(1, MAX_TOOL_ROUNDS_CLAMP);

    for round in 0..max_rounds {
        let request = serde_json::json!({
            "model": ctx.provider.model,
            "messages": messages,
            "temperature": 0.1,
            "stream": false,
            "tools": tools,
            "tool_choice": "auto",
        });
        let response = send_tool_round(&client, &endpoint, ctx.provider, &request).await?;
        let Some(choice) = response.choices.into_iter().next() else {
            return Err("Agent loop response had no choices".to_string());
        };
        let tool_calls = choice.message.tool_calls.clone().unwrap_or_default();
        if tool_calls.is_empty() {
            // The model is done exploring and is ready to answer.
            break;
        }
        log::info!(
            "unified_loop round={} requested {} tool call(s)",
            round + 1,
            tool_calls.len()
        );
        // Echo the assistant's tool-call message back so the provider can match
        // the tool results to it in the next request.
        messages.push(assistant_tool_call_message(&choice.message.content, &tool_calls));

        for call in &tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::json!({}));
            let fallback_query = args
                .get("query")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| ctx.question.to_string());

            let start_event = tool_call_event(
                call.function.name.clone(),
                args.clone(),
                format!("Calling {}", call.function.name),
                String::new(),
                format!(
                    "unified_loop tool={} args={}",
                    call.function.name,
                    truncate_for_error(&args.to_string(), 200)
                ),
            );
            emit_activity(ctx.app, ctx.activity_event_id, start_event.clone());
            agent_run.trace.events.push(start_event);

            let output = {
                let conn = ctx
                    .database
                    .conn
                    .lock()
                    .map_err(|_| "SQLite lock was poisoned".to_string())?;
                runtime::rag::execute_rag_tool_call_for_capabilities(
                    &conn,
                    ctx.document_id,
                    ctx.visible_document_ids,
                    &call.function.name,
                    &args,
                    &fallback_query,
                    rag_capabilities,
                )
            };

            let result_event = tool_result_event(
                &output,
                output.tool_call.tool.clone(),
                format!(
                    "unified_loop tool={} results={}",
                    output.tool_call.tool, output.tool_call.result_count
                ),
            );
            emit_activity(ctx.app, ctx.activity_event_id, result_event.clone());
            agent_run.trace.events.push(result_event);

            let rendered = render_tool_output(&call.function.name, &output);
            // Merge the gained citations into the shared run (budget-aware merge +
            // coverage + trace sync) — the same accounting the M4 loop uses.
            crate::agent_judge::apply_judge_tool_output(agent_run, &output);
            messages.push(tool_result_message(&call.id, &rendered));
        }
    }

    finalize_answer(&ctx, agent_run, messages, &client, &endpoint).await
}

/// Build the OpenAI `tools` array from the RAG tool specs.
fn build_openai_tools(vision_enabled: bool) -> Vec<serde_json::Value> {
    runtime::rag::rag_tool_specs_for_capabilities(vision_enabled)
        .into_iter()
        .map(|spec| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": spec.name,
                    "description": spec.description,
                    "parameters": spec.input_schema,
                }
            })
        })
        .collect()
}

fn build_initial_messages(
    ctx: &UnifiedLoopInput<'_>,
    agent_run: &runtime::agent::AgentRunResult,
) -> Vec<serde_json::Value> {
    let answer_language =
        llm::chat::answer_language_for_question(ctx.question, ctx.input.locale.as_deref());
    let system = format!(
        "You are Lumenfolio, a careful academic PDF reading assistant. Answer in {answer_language}. \
You can call retrieval tools to read the user's PDFs (search passages, open sections, open tables and pages, inspect the structure, recall prior chat). \
Call the tools you need to gather evidence, then write the answer. Use only evidence you retrieved or that is already provided below — do not invent facts. \
When the question is about the user's document library/workspace itself — which documents or papers they have, what is in the sidebar/list, which of their papers is about a topic — the 'Workspace documents' list below is authoritative: answer directly from it (list the relevant titles), no retrieval is needed. \
Prefer the focus document; only pass another document's id as the `documentId` tool argument when the question genuinely needs cross-document evidence, and only use an id listed in 'Workspace documents'. \
When you have enough evidence, stop calling tools and reply with a structured Markdown answer (a short direct answer first, then concise paragraphs or lists). Do not return JSON. If the evidence is insufficient, say so clearly and state what is missing."
    );

    let mut context = String::new();
    if !ctx.session_context.trim().is_empty() {
        context.push_str("Conversation memory:\n");
        context.push_str(ctx.session_context.trim_end());
        context.push_str("\n\n");
    }
    if !ctx.workspace_manifest.trim().is_empty() {
        context.push_str(ctx.workspace_manifest.trim_end());
        context.push_str("\n\n");
    }
    let seed_evidence = agent_run.retrieval_run.prompt_context.trim();
    if !seed_evidence.is_empty() {
        context.push_str(
            "Initial evidence already gathered (you may rely on it or retrieve more):\n",
        );
        context.push_str(seed_evidence);
        context.push_str("\n\n");
    }
    if let Some(page) = ctx.input.page {
        context.push_str(&format!("The user is currently viewing page {page}.\n\n"));
    }
    context.push_str("Question:\n");
    context.push_str(ctx.question);

    vec![
        serde_json::json!({ "role": "system", "content": system }),
        serde_json::json!({ "role": "user", "content": context }),
    ]
}

fn assistant_tool_call_message(
    content: &serde_json::Value,
    tool_calls: &[ToolCallEntry],
) -> serde_json::Value {
    let calls: Vec<serde_json::Value> = tool_calls
        .iter()
        .map(|call| {
            serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.function.name,
                    "arguments": call.function.arguments,
                }
            })
        })
        .collect();
    // Preserve any assistant preface text; null when empty (OpenAI tool-call shape).
    let content_value = match content {
        serde_json::Value::String(text) if !text.trim().is_empty() => {
            serde_json::Value::String(text.clone())
        }
        _ => serde_json::Value::Null,
    };
    serde_json::json!({
        "role": "assistant",
        "content": content_value,
        "tool_calls": calls,
    })
}

fn tool_result_message(tool_call_id: &str, rendered: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": rendered,
    })
}

/// Render a tool execution result into text the model reads as the tool message.
fn render_tool_output(tool: &str, output: &runtime::rag::RagToolExecutionOutput) -> String {
    if output.tool_call.status == "error" {
        if let Some(error) = &output.tool_call.error {
            return format!("Tool {tool} failed: {error}");
        }
    }
    if output.citations.is_empty() {
        if !output.tree_nodes.is_empty() {
            let nodes = output
                .tree_nodes
                .iter()
                .take(20)
                .map(|node| format!("- {} (p.{}) [id: {}]", node.title, node.page, node.id))
                .collect::<Vec<_>>()
                .join("\n");
            return format!(
                "Tool {tool} found these sections (open one for its passages):\n{nodes}"
            );
        }
        return format!("Tool {tool} returned no new evidence.");
    }
    let mut rendered = format!(
        "Tool {tool} returned {} evidence snippet(s):\n",
        output.citations.len()
    );
    for (index, citation) in output.citations.iter().enumerate().take(40) {
        let section = citation
            .section_title
            .as_deref()
            .map(|title| format!(" · {title}"))
            .unwrap_or_default();
        rendered.push_str(&format!(
            "[{}] (p.{}{}) {}\n",
            index + 1,
            citation.page,
            section,
            truncate_for_error(citation.quote.trim(), 600)
        ));
    }
    rendered
}

async fn send_tool_round(
    client: &reqwest::Client,
    endpoint: &str,
    provider: &OpenAiCompatibleProvider,
    request: &serde_json::Value,
) -> Result<ToolRoundResponse, String> {
    let mut builder = client.post(endpoint).json(request);
    if let Some(api_key) = &provider.api_key {
        builder = builder.bearer_auth(api_key);
    }
    let response = builder
        .send()
        .await
        .map_err(|err| format!("Agent loop request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Agent loop provider returned {status}: {}",
            truncate_for_error(&body, 600)
        ));
    }
    response
        .json::<ToolRoundResponse>()
        .await
        .map_err(|err| format!("Failed to decode agent loop response: {err}"))
}

/// Final answer: stream from the SAME full message history (tool results included)
/// with tools disabled so the model writes prose instead of calling more tools.
async fn finalize_answer(
    ctx: &UnifiedLoopInput<'_>,
    agent_run: &mut runtime::agent::AgentRunResult,
    messages: Vec<serde_json::Value>,
    client: &reqwest::Client,
    endpoint: &str,
) -> Result<AskAnswerResult, String> {
    if let Some(event_id) = ctx.activity_event_id {
        let _ = ctx.app.emit(
            "lumenfolio://agent-activity",
            AgentActivityEventOutput {
                event_id: event_id.to_string(),
                event: runtime::agent::AgentTraceEvent::new(
                    "answer_start",
                    "generate_answer",
                    "running",
                    "Generating answer",
                    "Streaming answer from the unified agent loop",
                    "unified_loop streaming final answer",
                ),
            },
        );
    }

    let tools = build_openai_tools(
        ctx.provider
            .capabilities
            .iter()
            .any(|capability| capability == "vision"),
    );
    let request = serde_json::json!({
        "model": ctx.provider.model,
        "messages": messages,
        "temperature": 0.2,
        "stream": true,
        // Keep `tools` present but force prose so providers that validate
        // tool_call_id references against a tool list stay happy.
        "tools": tools,
        "tool_choice": "none",
    });

    let mut builder = client
        .post(endpoint)
        .header("Accept", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .json(&request);
    if let Some(api_key) = &ctx.provider.api_key {
        builder = builder.bearer_auth(api_key);
    }
    let response = builder
        .send()
        .await
        .map_err(|err| format!("Agent loop answer request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Agent loop answer provider returned {status}: {}",
            truncate_for_error(&body, 600)
        ));
    }

    let streamed = llm::openai_stream::read_openai_answer_stream(
        response,
        ctx.app,
        ctx.activity_event_id,
    )
    .await?;
    let answer = streamed.answer.trim().to_string();
    if answer.is_empty() {
        return Err("Agent loop returned an empty answer".to_string());
    }

    // Stamp an answerable gate so downstream persistence/UI treats this turn as
    // answered via the unified runtime.
    let gate = serde_json::json!({
        "status": "answerable",
        "reason": "Answered via the unified tool-calling agent loop.",
        "missing": serde_json::json!([]),
        "nextToolCall": serde_json::Value::Null,
        "citationCount": agent_run.retrieval_run.citations.len(),
        "runtime": "unified-loop",
    });
    agent_run.retrieval_run.trace.finalize_gate = gate.clone();
    agent_run.trace.finalize_gate = gate;

    let claims = llm::claims::fallback_claims_from_answer(&answer, &agent_run.retrieval_run.citations);
    let answer = llm::claims::strip_known_inline_citation_labels(
        &answer,
        &agent_run.retrieval_run.citations,
    );

    Ok(AskAnswerResult {
        answer,
        reasoning_content: optional_non_empty(streamed.reasoning_content),
        claims,
    })
}

fn emit_activity(
    app: &tauri::AppHandle,
    activity_event_id: Option<&str>,
    event: runtime::agent::AgentTraceEvent,
) {
    if let Some(event_id) = activity_event_id {
        let _ = app.emit(
            "lumenfolio://agent-activity",
            AgentActivityEventOutput {
                event_id: event_id.to_string(),
                event,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_output(
        tool: &str,
        citations: Vec<runtime::rag::Citation>,
        tree_nodes: Vec<runtime::rag::RetrievalTraceTreeNode>,
    ) -> runtime::rag::RagToolExecutionOutput {
        runtime::rag::RagToolExecutionOutput {
            citations,
            trace_candidates: Vec::new(),
            tree_nodes,
            tool_call: runtime::rag::RetrievalTraceToolCall {
                tool: tool.to_string(),
                status: "ok".to_string(),
                input: serde_json::json!({}),
                result_count: 0,
                error: None,
            },
        }
    }

    fn citation(page: u32, section: Option<&str>, quote: &str) -> runtime::rag::Citation {
        runtime::rag::Citation {
            id: "rag-c-1".to_string(),
            label: "[1]".to_string(),
            page,
            block_id: "b-1".to_string(),
            section_title: section.map(str::to_string),
            quote: quote.to_string(),
            bbox_list: serde_json::json!([]),
            document_id: "doc-1".to_string(),
            source: "search_chunks".to_string(),
        }
    }

    #[test]
    fn build_openai_tools_maps_specs_to_function_shape() {
        let tools = build_openai_tools(false);
        assert!(!tools.is_empty());
        // Every entry is a function tool with a name + parameters object.
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].is_string());
            assert!(tool["function"]["parameters"].is_object());
        }
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"search_chunks"));
        // Vision-only tools are excluded when vision is disabled.
        assert!(!names.contains(&"analyze_visual"));
    }

    #[test]
    fn render_tool_output_handles_empty_and_populated() {
        let empty = tool_output("search_chunks", Vec::new(), Vec::new());
        assert_eq!(
            render_tool_output("search_chunks", &empty),
            "Tool search_chunks returned no new evidence."
        );

        let populated = tool_output(
            "open_section",
            vec![citation(4, Some("Method"), "We propose X.")],
            Vec::new(),
        );
        let rendered = render_tool_output("open_section", &populated);
        assert!(rendered.contains("returned 1 evidence snippet"));
        assert!(rendered.contains("p.4"));
        assert!(rendered.contains("Method"));
        assert!(rendered.contains("We propose X."));
    }

    #[test]
    fn render_tool_output_lists_tree_nodes_when_no_citations() {
        let nodes = vec![runtime::rag::RetrievalTraceTreeNode {
            id: "n-1".to_string(),
            title: "Introduction".to_string(),
            page: 1,
            block_index: 0,
            score: 1.0,
        }];
        let output = tool_output("inspect_tree", Vec::new(), nodes);
        let rendered = render_tool_output("inspect_tree", &output);
        assert!(rendered.contains("Introduction"));
        assert!(rendered.contains("[id: n-1]"));
    }

    #[test]
    fn assistant_tool_call_message_nulls_empty_content_and_echoes_calls() {
        let calls = vec![ToolCallEntry {
            id: "call_1".to_string(),
            function: ToolCallFunction {
                name: "search_chunks".to_string(),
                arguments: "{\"query\":\"x\"}".to_string(),
            },
        }];
        let message = assistant_tool_call_message(&serde_json::Value::String("  ".to_string()), &calls);
        assert_eq!(message["role"], "assistant");
        assert!(message["content"].is_null());
        assert_eq!(message["tool_calls"][0]["id"], "call_1");
        assert_eq!(message["tool_calls"][0]["type"], "function");
        assert_eq!(message["tool_calls"][0]["function"]["name"], "search_chunks");
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            "{\"query\":\"x\"}"
        );
    }

    #[test]
    fn tool_result_message_has_tool_role_and_id() {
        let message = tool_result_message("call_1", "evidence text");
        assert_eq!(message["role"], "tool");
        assert_eq!(message["tool_call_id"], "call_1");
        assert_eq!(message["content"], "evidence text");
    }
}
