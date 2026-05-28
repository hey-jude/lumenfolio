use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::rag::{
    Citation, RetrievalTrace, RetrievalTraceCandidate, RetrievalTraceTreeNode,
};

use super::compact::CompactResult;
use super::protocol::AgentStepKind;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceEvent {
    pub step: String,
    pub status: String,
    pub detail: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub title: String,
    pub summary: String,
    pub ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<AgentTraceTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AgentTraceResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceTool {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceResult {
    pub count: usize,
    pub preview: Vec<AgentTracePreview>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTracePreview {
    pub page: u32,
    pub section_title: Option<String>,
    pub quote: String,
    pub source: String,
}

impl AgentTraceEvent {
    pub fn new(
        event_type: impl Into<String>,
        step: impl Into<String>,
        status: impl Into<String>,
        title: impl Into<String>,
        summary: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            step: step.into(),
            status: status.into(),
            detail: detail.into(),
            event_type: event_type.into(),
            title: title.into(),
            summary: summary.into(),
            ts: unix_millis(),
            tool: None,
            result: None,
            judge: None,
        }
    }

    pub fn with_tool(mut self, name: impl Into<String>, args: serde_json::Value) -> Self {
        self.tool = Some(AgentTraceTool {
            name: name.into(),
            args,
        });
        self
    }

    pub fn with_result(
        mut self,
        count: usize,
        preview: Vec<AgentTracePreview>,
        error: Option<String>,
    ) -> Self {
        self.result = Some(AgentTraceResult {
            count,
            preview,
            error,
        });
        self
    }

    pub fn with_judge(mut self, judge: serde_json::Value) -> Self {
        self.judge = Some(judge);
        self
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvidenceItem {
    pub citation_id: String,
    pub label: String,
    pub page: u32,
    pub block_id: String,
    pub section_title: Option<String>,
    pub source: String,
    pub quote: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTrace {
    pub run_id: String,
    pub intent: String,
    pub tree_nodes: Vec<RetrievalTraceTreeNode>,
    pub candidates: Vec<RetrievalTraceCandidate>,
    pub finalize_gate: serde_json::Value,
    pub evidence_chain: Vec<AgentEvidenceItem>,
    pub events: Vec<AgentTraceEvent>,
    pub session_summary: Option<String>,
    pub compact: Option<CompactResult>,
}

impl AgentTrace {
    pub fn from_retrieval(
        retrieval_trace: RetrievalTrace,
        citations: &[Citation],
        events: Vec<AgentTraceEvent>,
        session_summary: Option<String>,
        compact: Option<CompactResult>,
    ) -> Self {
        Self {
            run_id: retrieval_trace.run_id,
            intent: retrieval_trace.intent,
            tree_nodes: retrieval_trace.tree_nodes,
            evidence_chain: build_evidence_chain(citations, &retrieval_trace.candidates),
            candidates: retrieval_trace.candidates,
            finalize_gate: retrieval_trace.finalize_gate,
            events,
            session_summary,
            compact,
        }
    }

    pub fn sync_retrieval_view(&mut self, retrieval_trace: RetrievalTrace, citations: &[Citation]) {
        self.run_id = retrieval_trace.run_id;
        self.intent = retrieval_trace.intent;
        self.tree_nodes = retrieval_trace.tree_nodes;
        self.evidence_chain = build_evidence_chain(citations, &retrieval_trace.candidates);
        self.candidates = retrieval_trace.candidates;
        self.finalize_gate = retrieval_trace.finalize_gate;
    }
}

fn build_evidence_chain(
    citations: &[Citation],
    candidates: &[RetrievalTraceCandidate],
) -> Vec<AgentEvidenceItem> {
    citations
        .iter()
        .map(|citation| {
            let section_title = citation.section_title.clone().or_else(|| {
                candidates
                    .iter()
                    .find(|candidate| {
                        (!citation.block_id.is_empty() && candidate.block_id == citation.block_id)
                            || (candidate.source == citation.source
                                && normalize_for_match(&candidate.quote)
                                    == normalize_for_match(&citation.quote))
                    })
                    .and_then(|candidate| candidate.section_title.clone())
            });
            AgentEvidenceItem {
                citation_id: citation.id.clone(),
                label: citation.label.clone(),
                page: citation.page,
                block_id: citation.block_id.clone(),
                section_title,
                source: citation.source.clone(),
                quote: citation.quote.clone(),
            }
        })
        .collect()
}

fn normalize_for_match(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn event(step: AgentStepKind, detail: impl Into<String>) -> AgentTraceEvent {
    event_with_status(step, "completed", detail)
}

pub fn skipped_event(step: AgentStepKind, detail: impl Into<String>) -> AgentTraceEvent {
    event_with_status(step, "skipped", detail)
}

pub fn event_with_status(
    step: AgentStepKind,
    status: impl Into<String>,
    detail: impl Into<String>,
) -> AgentTraceEvent {
    let step = step.as_str();
    let detail = detail.into();
    AgentTraceEvent::new(step, step, status, step, detail.clone(), detail)
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}
