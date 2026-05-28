use rusqlite::Connection;

use crate::runtime::rag::{self, Citation, RetrievalRequest, RetrievalRun};

use super::compact::{compact_session, CompactResult};
use super::context::build_chat_context;
use super::finalize::{finalize_citations, FinalizeDecision};
use super::memory::{preview_text, summarize_citations, RecentTurnSummary};
use super::protocol::{AgentIntent, AgentStepKind};
use super::session::AgentSessionStore;
#[cfg(test)]
use super::tools::AgentToolName;
use super::trace::{event, skipped_event, AgentTrace, AgentTraceEvent, AgentTracePreview};

const DEFAULT_MAX_RETRIEVAL_STEPS: u32 = 20;
const MAX_RETRIEVAL_STEPS: u32 = 20;

pub struct AgentRunRequest<'a> {
    pub document_id: &'a str,
    pub question: &'a str,
    pub provider_id: Option<&'a str>,
    pub context_budget: crate::model_catalog::ModelContextBudget,
    pub selected_text: Option<&'a str>,
    pub selected_block_id: Option<&'a str>,
    pub selected_bbox_list: Option<serde_json::Value>,
    pub page: Option<u32>,
    pub page_mode: Option<&'a str>,
    pub page_source: Option<&'a str>,
    pub max_retrieval_steps: Option<u32>,
    pub retrieval_attempt_offset: u32,
}

pub struct AgentRunResult {
    pub retrieval_run: RetrievalRun,
    pub trace: AgentTrace,
    pub session_context: String,
}

pub fn run_turn_with_activity<F>(
    conn: &Connection,
    sessions: &AgentSessionStore,
    request: AgentRunRequest<'_>,
    mut on_event: F,
) -> Result<AgentRunResult, String>
where
    F: FnMut(&AgentTraceEvent),
{
    let snapshot = sessions.snapshot(request.document_id);
    let compacted = snapshot.as_ref().map(compact_session).unwrap_or_else(|| {
        compact_session(&empty_snapshot(request.document_id, request.provider_id))
    });
    let intent = AgentIntent::infer(request.question);
    let max_retrieval_attempts = request
        .max_retrieval_steps
        .unwrap_or(DEFAULT_MAX_RETRIEVAL_STEPS)
        .clamp(1, MAX_RETRIEVAL_STEPS);
    let mut loop_events = Vec::new();
    let (mut retrieval_run, finalize) = run_answerability_loop(
        conn,
        &request,
        intent,
        max_retrieval_attempts,
        request.retrieval_attempt_offset,
        &mut loop_events,
        &mut on_event,
    )?;

    let session_context = build_chat_context(snapshot.as_ref(), &compacted, &retrieval_run);
    let mut finalize_gate = serde_json::to_value(&finalize)
        .map_err(|err| format!("Failed to encode finalize decision: {err}"))?;
    if let Some(object) = finalize_gate.as_object_mut() {
        object.insert(
            "contextBudget".to_string(),
            serde_json::to_value(&request.context_budget)
                .map_err(|err| format!("Failed to encode context budget: {err}"))?,
        );
    }
    retrieval_run.trace.finalize_gate = finalize_gate;
    let events = build_agent_events(
        &retrieval_run,
        intent,
        compacted.compact.clone(),
        !session_context.trim().is_empty(),
        loop_events,
    );
    let trace = AgentTrace::from_retrieval(
        retrieval_run.trace.clone(),
        &retrieval_run.citations,
        events,
        if session_context.trim().is_empty() {
            None
        } else {
            Some(session_context.clone())
        },
        Some(compacted.compact.clone()),
    );

    Ok(AgentRunResult {
        retrieval_run,
        trace,
        session_context,
    })
}

fn run_answerability_loop<F>(
    conn: &Connection,
    request: &AgentRunRequest<'_>,
    intent: AgentIntent,
    max_attempts: u32,
    attempt_offset: u32,
    loop_events: &mut Vec<super::trace::AgentTraceEvent>,
    on_event: &mut F,
) -> Result<(RetrievalRun, FinalizeDecision), String>
where
    F: FnMut(&AgentTraceEvent),
{
    let mut attempt = attempt_offset;
    let max_attempts = attempt_offset.saturating_add(max_attempts);
    let mut accumulated_citations = Vec::new();
    let mut attempted_tool_signatures = std::collections::HashSet::new();
    let start_event = AgentTraceEvent::new(
        "retrieval_round",
        AgentStepKind::SearchChunks.as_str(),
        "running",
        "Gathering initial evidence",
        "Inspecting structure, sections, text search, and nearby pages",
        format!(
            "attempt={} query={}",
            attempt.saturating_add(1),
            request.question
        ),
    );
    on_event(&start_event);

    let mut retrieval_run = rag::build_retrieval_run(
        conn,
        RetrievalRequest {
            document_id: request.document_id,
            question: request.question,
            retrieval_query: None,
            selected_text: request.selected_text,
            selected_block_id: request.selected_block_id,
            selected_bbox_list: request.selected_bbox_list.clone(),
            client_context: None,
            page: request.page,
            page_mode: request.page_mode,
            page_source: request.page_source,
            context_budget: request.context_budget.clone(),
            force_document_start: false,
        },
    )?;
    for call in retrieval_run.trace.tool_calls.iter().cloned() {
        let result_event = AgentTraceEvent::new(
            "tool_result",
            call.tool.clone(),
            trace_status_from_tool_status(&call.status),
            format!("{} result", call.tool),
            format!("{} returned {} results", call.tool, call.result_count),
            format!(
                "initial retrieval tool={} status={} results={}",
                call.tool, call.status, call.result_count
            ),
        )
        .with_tool(call.tool.clone(), call.input.clone())
        .with_result(call.result_count, Vec::new(), call.error.clone());
        on_event(&result_event);
        loop_events.push(result_event);
    }
    let round_event = AgentTraceEvent::new(
        "retrieval_round",
        AgentStepKind::SearchChunks.as_str(),
        "completed",
        "Initial evidence gathered",
        format!(
            "Prepared {} citations from {} candidates",
            retrieval_run.citations.len(),
            retrieval_run.trace.candidates.len()
        ),
        format!(
            "initial retrieval citations={} candidates={}",
            retrieval_run.citations.len(),
            retrieval_run.trace.candidates.len()
        ),
    );
    on_event(&round_event);
    loop_events.push(round_event);
    rag::merge_retrieval_citations_with_budget(
        &mut accumulated_citations,
        &retrieval_run.citations,
        &request.context_budget,
    );

    loop {
        rag::apply_retrieval_citations(&mut retrieval_run, accumulated_citations.clone());
        let decision = finalize_citations(
            request.question,
            intent.as_str(),
            &retrieval_run.citations,
            attempt,
            max_attempts,
        );
        let detail = format!(
            "attempt={} status={} reason={} next_tool={}",
            attempt + 1,
            decision.status,
            decision.reason,
            decision
                .next_tool_call
                .as_ref()
                .map(|call| call.tool.as_str())
                .or(decision.next_tool.as_deref())
                .unwrap_or("none")
        );
        let finalize_event = AgentTraceEvent::new(
            "judge_result",
            AgentStepKind::FinalizeAnswer.as_str(),
            "completed",
            "M3 rule evidence check",
            format!(
                "status={} next_tool={}",
                decision.status,
                decision
                    .next_tool_call
                    .as_ref()
                    .map(|call| call.tool.as_str())
                    .or(decision.next_tool.as_deref())
                    .unwrap_or("none")
            ),
            detail,
        )
        .with_judge(serde_json::to_value(&decision).unwrap_or_else(|_| serde_json::json!({})));
        on_event(&finalize_event);
        loop_events.push(finalize_event);

        if !decision.needs_more_evidence {
            return Ok((retrieval_run, decision));
        }

        let fallback_query = broad_context_query(intent, attempt);
        let Some((tool, args)) = next_tool_parts(&decision) else {
            return Ok((retrieval_run, decision));
        };
        let signature = tool_signature(tool, &args);
        let (tool, args, repeated_tool) = if attempted_tool_signatures.insert(signature) {
            (tool, args, false)
        } else if is_header_lookup(tool, &args) {
            let mut terminal_decision = decision.clone();
            terminal_decision.status = "insufficient".to_string();
            terminal_decision.needs_more_evidence = false;
            terminal_decision.next_tool = None;
            terminal_decision.next_tool_call = None;
            terminal_decision.reason = format!(
                "{} Header lookup already ran without producing header evidence; the document index likely needs to be rebuilt with header roles.",
                terminal_decision.reason.trim()
            );
            let skipped = skipped_event(
                AgentStepKind::OpenPages,
                "header lookup already attempted; stopping instead of broad-search fallback"
                    .to_string(),
            );
            on_event(&skipped);
            loop_events.push(skipped);
            return Ok((retrieval_run, terminal_decision));
        } else {
            (
                "search_chunks",
                serde_json::json!({ "query": fallback_query, "limit": 8 }),
                true,
            )
        };
        let call_event = AgentTraceEvent::new(
            "tool_call",
            tool,
            "running",
            format!("Calling {tool}"),
            decision
                .next_tool_call
                .as_ref()
                .map(|call| call.reason.as_str())
                .unwrap_or("Need more evidence"),
            format!("tool={tool} args={args} fallback_query={fallback_query}"),
        )
        .with_tool(tool, args.clone());
        on_event(&call_event);

        let output = rag::execute_rag_tool_call_for_capabilities(
            conn,
            request.document_id,
            tool,
            &args,
            fallback_query,
            rag::RagToolCapabilities {
                vision_enabled: false,
                max_quote_chars: request.context_budget.max_quote_chars,
            },
        );
        let tool_event = AgentTraceEvent::new(
            "tool_result",
            step_kind_for_tool(&output.tool_call.tool).as_str(),
            trace_status_from_tool_status(&output.tool_call.status),
            format!("{} result", output.tool_call.tool),
            format!(
                "{} returned {} results{}",
                output.tool_call.tool,
                output.tool_call.result_count,
                if repeated_tool { " after fallback" } else { "" }
            ),
            format!(
                "tool={} status={} results={} reason={}{}",
                output.tool_call.tool,
                output.tool_call.status,
                output.tool_call.result_count,
                decision
                    .next_tool_call
                    .as_ref()
                    .map(|call| call.reason.as_str())
                    .unwrap_or("legacy fallback"),
                if repeated_tool {
                    " repeated_tool_fallback=true"
                } else {
                    ""
                }
            ),
        )
        .with_tool(
            output.tool_call.tool.clone(),
            output.tool_call.input.clone(),
        )
        .with_result(
            output.tool_call.result_count,
            trace_preview_from_output(&output),
            output.tool_call.error.clone(),
        );
        on_event(&tool_event);
        loop_events.push(tool_event);
        rag::merge_retrieval_citations_with_budget(
            &mut accumulated_citations,
            &output.citations,
            &request.context_budget,
        );
        rag::apply_tool_execution(&mut retrieval_run, &output);
        if let Some(page) = m3_table_page_fallback_page(&output, &accumulated_citations) {
            let page_args = serde_json::json!({
                "page": page,
                "mode": "full",
                "limit": 40
            });
            let page_call_event = AgentTraceEvent::new(
                "tool_call",
                "open_pages",
                "running",
                "Calling open_pages",
                "Opening the source page because structured table extraction looked sparse or noisy",
                format!("tool=open_pages args={page_args} fallback_query={fallback_query}"),
            )
            .with_tool("open_pages", page_args.clone());
            on_event(&page_call_event);

            let page_output = rag::execute_rag_tool_call_for_capabilities(
                conn,
                request.document_id,
                "open_pages",
                &page_args,
                fallback_query,
                rag::RagToolCapabilities {
                    vision_enabled: false,
                    max_quote_chars: request.context_budget.max_quote_chars,
                },
            );
            let page_tool_event = AgentTraceEvent::new(
                "tool_result",
                AgentStepKind::OpenPages.as_str(),
                trace_status_from_tool_status(&page_output.tool_call.status),
                format!("{} result", page_output.tool_call.tool),
                format!(
                    "{} returned {} results",
                    page_output.tool_call.tool, page_output.tool_call.result_count
                ),
                format!(
                    "tool={} status={} results={} reason=structured table evidence was incomplete",
                    page_output.tool_call.tool,
                    page_output.tool_call.status,
                    page_output.tool_call.result_count
                ),
            )
            .with_tool(
                page_output.tool_call.tool.clone(),
                page_output.tool_call.input.clone(),
            )
            .with_result(
                page_output.tool_call.result_count,
                trace_preview_from_output(&page_output),
                page_output.tool_call.error.clone(),
            );
            on_event(&page_tool_event);
            loop_events.push(page_tool_event);
            rag::merge_retrieval_citations_with_budget(
                &mut accumulated_citations,
                &page_output.citations,
                &request.context_budget,
            );
            rag::apply_tool_execution(&mut retrieval_run, &page_output);
        }
        attempt += 1;
    }
}

fn m3_table_page_fallback_page(
    output: &rag::RagToolExecutionOutput,
    accumulated_citations: &[Citation],
) -> Option<u32> {
    if output.tool_call.tool != "open_table" {
        return None;
    }
    let page = output
        .citations
        .iter()
        .find(|citation| citation.source == "open_table")
        .map(|citation| citation.page)?;
    if accumulated_citations.iter().any(|citation| {
        citation.source == "open_pages"
            && citation.page == page
            && citation
                .section_title
                .as_deref()
                .is_some_and(|title| title.contains(" lines "))
    }) {
        return None;
    }
    let combined = output
        .citations
        .iter()
        .map(|citation| citation.quote.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let sparse = output.citations.len() < 6 || combined.chars().count() < 700;
    let placeholder_columns = ["column 1", "column 2", "column 3", "column 4"]
        .iter()
        .any(|needle| combined.contains(needle));
    (sparse || placeholder_columns).then_some(page)
}

fn tool_signature(tool: &str, args: &serde_json::Value) -> String {
    format!("{tool}:{}", args)
}

fn is_header_lookup(tool: &str, args: &serde_json::Value) -> bool {
    tool == "open_pages"
        && args
            .get("page")
            .and_then(|value| value.as_u64())
            .is_some_and(|page| page == 1)
        && args
            .get("mode")
            .and_then(|value| value.as_str())
            .is_some_and(|mode| mode == "header")
}

fn next_tool_parts(decision: &FinalizeDecision) -> Option<(&str, serde_json::Value)> {
    if let Some(call) = decision.next_tool_call.as_ref() {
        return rag::is_registered_rag_tool(&call.tool)
            .then_some((call.tool.as_str(), call.args.clone()));
    }
    match decision.next_tool.as_deref()? {
        "open_document_start" => Some((
            "open_pages",
            serde_json::json!({ "page": 1, "mode": "overview" }),
        )),
        "search_broad_context" => Some((
            "search_chunks",
            serde_json::json!({ "query": "broad_context", "page": 1 }),
        )),
        tool if rag::is_registered_rag_tool(tool) => Some((tool, serde_json::json!({}))),
        _ => None,
    }
}

fn step_kind_for_tool(tool: &str) -> AgentStepKind {
    match tool {
        "inspect_tree" => AgentStepKind::InspectTree,
        "open_section" => AgentStepKind::OpenSection,
        "open_pages" => AgentStepKind::OpenPages,
        "search_chunks" => AgentStepKind::SearchChunks,
        _ => AgentStepKind::SearchChunks,
    }
}

fn trace_status_from_tool_status(status: &str) -> &'static str {
    if status == "error" {
        "error"
    } else {
        "completed"
    }
}

fn trace_preview_from_output(output: &rag::RagToolExecutionOutput) -> Vec<AgentTracePreview> {
    output
        .trace_candidates
        .iter()
        .take(3)
        .map(|candidate| AgentTracePreview {
            page: candidate.page,
            section_title: candidate.section_title.clone(),
            quote: candidate.quote.clone(),
            source: candidate.source.clone(),
        })
        .collect()
}

fn broad_context_query(intent: AgentIntent, attempt: u32) -> &'static str {
    let queries: &[&str] = match intent {
        AgentIntent::Compare => &[
            "comparison difference evaluation results",
            "baseline ablation performance limitation",
            "experiment table metric benchmark",
        ],
        AgentIntent::Locate => &[
            "definition location section page",
            "method term introduced described",
            "figure table algorithm reference",
        ],
        AgentIntent::Summarize | AgentIntent::Explain => &[
            "abstract introduction contribution method approach experiment result conclusion",
            "problem motivation proposed method contribution",
            "system architecture design implementation evaluation",
            "limitations discussion conclusion future work",
            "results experiments benchmark findings",
        ],
    };
    queries[(attempt as usize).saturating_sub(1) % queries.len()]
}

pub struct CompletedTurnRecord<'a> {
    pub document_id: &'a str,
    pub provider_id: Option<&'a str>,
    pub question: &'a str,
    pub answer: &'a str,
    pub selected_text: Option<&'a str>,
    pub citations: &'a [Citation],
    pub trace: &'a AgentTrace,
}

pub fn record_completed_turn(sessions: &AgentSessionStore, record: CompletedTurnRecord<'_>) {
    let tree_titles = record
        .trace
        .tree_nodes
        .iter()
        .take(4)
        .map(|node| node.title.clone())
        .collect::<Vec<_>>();
    sessions.record_turn(
        record.document_id,
        record.provider_id.map(str::to_string),
        RecentTurnSummary {
            question: preview_text(record.question, 220),
            answer_preview: preview_text(record.answer, 260),
            citations: summarize_citations(record.citations, 4),
            selected_text_preview: record
                .selected_text
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| preview_text(value, 180)),
            tree_titles,
        },
    );
}

pub struct RestoredTurnRecord<'a> {
    pub document_id: &'a str,
    pub provider_id: Option<&'a str>,
    pub question: &'a str,
    pub answer: &'a str,
    pub selected_text: Option<&'a str>,
    pub citations: &'a [Citation],
    pub tree_titles: Vec<String>,
}

pub fn record_restored_turn(sessions: &AgentSessionStore, record: RestoredTurnRecord<'_>) {
    sessions.record_turn(
        record.document_id,
        record.provider_id.map(str::to_string),
        RecentTurnSummary {
            question: preview_text(record.question, 220),
            answer_preview: preview_text(record.answer, 260),
            citations: summarize_citations(record.citations, 4),
            selected_text_preview: record
                .selected_text
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| preview_text(value, 180)),
            tree_titles: record.tree_titles,
        },
    );
}

fn build_agent_events(
    _retrieval_run: &RetrievalRun,
    intent: AgentIntent,
    compact: CompactResult,
    has_session_context: bool,
    loop_events: Vec<super::trace::AgentTraceEvent>,
) -> Vec<super::trace::AgentTraceEvent> {
    let mut events = Vec::new();
    if compact.summary_generated || compact.summarized_turns > 0 || has_session_context {
        events.push(event(
            AgentStepKind::SessionCompact,
            format!(
                "retained {} recent turns, summarized {} turns",
                compact.retained_recent_turns, compact.summarized_turns
            ),
        ));
    } else {
        events.push(skipped_event(
            AgentStepKind::SessionCompact,
            "no prior turns needed compaction".to_string(),
        ));
    }
    events.push(event(
        AgentStepKind::IntentClassify,
        format!("intent={}", intent.as_str()),
    ));
    events.extend(loop_events);
    events
}

#[cfg(test)]
fn trace_tool_result_count(retrieval_run: &RetrievalRun, tool: AgentToolName) -> Option<usize> {
    let mut saw_tool = false;
    let count = retrieval_run
        .trace
        .tool_calls
        .iter()
        .filter(|call| call.tool == tool.as_str())
        .map(|call| {
            saw_tool = true;
            call.result_count
        })
        .sum();

    saw_tool.then_some(count)
}

fn empty_snapshot(
    document_id: &str,
    provider_id: Option<&str>,
) -> super::memory::SessionMemorySnapshot {
    super::memory::SessionMemorySnapshot {
        document_id: document_id.to_string(),
        provider_id: provider_id.map(str::to_string),
        recent_turns: Vec::new(),
        selection: None,
        sections: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::rag::{RetrievalTrace, RetrievalTraceToolCall};

    #[test]
    fn trace_tool_result_count_sums_repeated_tool_calls() {
        let run = RetrievalRun {
            id: "run".to_string(),
            intent: "explain".to_string(),
            prompt_context: String::new(),
            citations: Vec::new(),
            trace: RetrievalTrace {
                run_id: "run".to_string(),
                intent: "explain".to_string(),
                tree_nodes: Vec::new(),
                candidates: Vec::new(),
                tool_calls: vec![
                    tool_call("open_section", 0),
                    tool_call("search_chunks", 2),
                    tool_call("open_section", 5),
                ],
                finalize_gate: serde_json::json!({}),
            },
            context_budget: crate::model_catalog::ModelContextBudget::default(),
        };

        assert_eq!(
            trace_tool_result_count(&run, AgentToolName::OpenSection),
            Some(5)
        );
        assert_eq!(
            trace_tool_result_count(&run, AgentToolName::SearchChunks),
            Some(2)
        );
        assert_eq!(
            trace_tool_result_count(&run, AgentToolName::OpenPages),
            None
        );
    }

    #[test]
    fn header_lookup_signature_is_detected() {
        assert!(is_header_lookup(
            "open_pages",
            &serde_json::json!({ "page": 1, "mode": "header" })
        ));
        assert!(!is_header_lookup(
            "open_pages",
            &serde_json::json!({ "page": 1, "mode": "overview" })
        ));
        assert!(!is_header_lookup(
            "search_chunks",
            &serde_json::json!({ "query": "author" })
        ));
    }

    #[test]
    fn next_tool_parts_rejects_unregistered_tools() {
        let mut decision = FinalizeDecision {
            status: "needs_more_evidence".to_string(),
            citation_count: 0,
            needs_more_evidence: true,
            reason: "test".to_string(),
            missing: Vec::new(),
            next_tool: Some("not_a_tool".to_string()),
            next_tool_call: None,
            attempt: 0,
            max_attempts: 3,
            budget_exhausted: false,
            runtime: "test".to_string(),
        };

        assert!(next_tool_parts(&decision).is_none());
        decision.next_tool_call = Some(super::super::finalize::NextToolCall {
            tool: "open_section".to_string(),
            args: serde_json::json!({ "query": "method" }),
            reason: "test".to_string(),
        });

        let (tool, args) = next_tool_parts(&decision).expect("registered tool");
        assert_eq!(tool, "open_section");
        assert_eq!(args["query"], serde_json::json!("method"));
    }

    fn tool_call(tool: &str, result_count: usize) -> RetrievalTraceToolCall {
        RetrievalTraceToolCall {
            tool: tool.to_string(),
            status: "ok".to_string(),
            input: serde_json::json!({}),
            result_count,
            error: None,
        }
    }
}
