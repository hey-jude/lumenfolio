//! Shared retrieval ledger — the single "what have I already looked at" record
//! for one agent turn.
//!
//! The M3 rule loop (`turn_runner`) and the M4 LLM-judge loop (`agent_judge`)
//! share one ledger. It records:
//! - tool-call de-duplication (signature set),
//! - coverage tracking (which pages / sections / tables / visuals were read).
//!
//! The M3 loop creates the ledger and stores it on `AgentRunResult`; the M4 loop
//! then inherits the M3 attempt set and coverage, so the judge never re-requests
//! a tool the rule loop already ran and can report an honest "already examined"
//! summary spanning both loops.

use std::collections::BTreeSet;

use crate::runtime::rag::Citation;

/// Mutable "already looked at" record for a single agent turn.
#[derive(Debug, Default, Clone)]
pub struct RetrievalLedger {
    tool_signatures: BTreeSet<String>,
    pages: BTreeSet<u32>,
    sections: BTreeSet<String>,
    tables: BTreeSet<String>,
    visuals: BTreeSet<String>,
}

impl RetrievalLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonical tool-call signature. Matches the legacy format used by both
    /// loops (`format!("{tool}:{args}")`) so dedupe behavior is unchanged.
    pub fn signature(tool: &str, args: &serde_json::Value) -> String {
        format!("{tool}:{args}")
    }

    /// Record a tool call. Returns `true` if it is newly seen (not a repeat),
    /// matching `HashSet::insert` semantics the loops relied on.
    pub fn record_tool_call(&mut self, tool: &str, args: &serde_json::Value) -> bool {
        self.tool_signatures.insert(Self::signature(tool, args))
    }

    /// Whether this exact tool call was already attempted this turn.
    /// The hot loops use `record_tool_call`'s return value for the dedup
    /// decision; this read-only accessor exists for callers that need to peek
    /// without recording (and documents the dedup contract under test).
    #[allow(dead_code)]
    pub fn has_attempted(&self, tool: &str, args: &serde_json::Value) -> bool {
        self.tool_signatures.contains(&Self::signature(tool, args))
    }

    /// All tool-call signatures attempted so far, for seeding another loop's
    /// dedupe state (e.g. handing M3 coverage to the M4 LLM-judge loop).
    pub fn attempted_signatures(&self) -> impl Iterator<Item = &String> {
        self.tool_signatures.iter()
    }

    /// Record the regions covered by a batch of citations so the ledger can later
    /// report "I already looked at pages 1-2, the Method section, Table 3".
    pub fn record_coverage(&mut self, citations: &[Citation]) {
        for citation in citations {
            self.pages.insert(citation.page);
            if let Some(section) = citation
                .section_title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                // Classify by the actual Citation `source` strings the RAG
                // tools emit (see `source:` assignments in runtime/rag/mod.rs
                // and agent_judge.rs), not by tool names.
                match citation.source.as_str() {
                    "open_table" | "open_table_context" | "table_fact" | "inspect_tables"
                    | "table_anchor" => {
                        self.tables.insert(section.to_string());
                    }
                    "open_visual" | "visual_asset" | "visual_anchor" | "inspect_objects"
                    | "caption" | "analyze_visual" | "analyze_page" => {
                        self.visuals.insert(section.to_string());
                    }
                    _ => {
                        self.sections.insert(section.to_string());
                    }
                }
            }
        }
    }

    /// Whether nothing has been covered yet.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
            && self.sections.is_empty()
            && self.tables.is_empty()
            && self.visuals.is_empty()
    }

    /// Human-readable "already looked at" summary for honest insufficiency
    /// messages and judge feedback. Returns `None` when nothing was covered.
    pub fn coverage_summary(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        if !self.pages.is_empty() {
            let pages = self
                .pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("pages {pages}"));
        }
        Self::push_named(&mut parts, "sections", &self.sections);
        Self::push_named(&mut parts, "tables", &self.tables);
        Self::push_named(&mut parts, "visuals", &self.visuals);
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }

    fn push_named(parts: &mut Vec<String>, label: &str, values: &BTreeSet<String>) {
        if values.is_empty() {
            return;
        }
        // Cap to keep the summary short and stable.
        let listed = values
            .iter()
            .take(6)
            .map(|value| value.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if values.len() > 6 {
            format!(" (+{} more)", values.len() - 6)
        } else {
            String::new()
        };
        parts.push(format!("{label}: {listed}{suffix}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citation(source: &str, page: u32, section: Option<&str>) -> Citation {
        Citation {
            id: String::new(),
            label: String::new(),
            page,
            block_id: String::new(),
            section_title: section.map(str::to_string),
            quote: "q".to_string(),
            bbox_list: serde_json::json!([]),
            document_id: "doc".to_string(),
            source: source.to_string(),
        }
    }

    #[test]
    fn record_tool_call_dedupes_like_a_set() {
        let mut ledger = RetrievalLedger::new();
        let args = serde_json::json!({ "query": "method", "limit": 8 });
        assert!(ledger.record_tool_call("open_section", &args));
        assert!(!ledger.record_tool_call("open_section", &args));
        assert!(ledger.has_attempted("open_section", &args));
        // Different args -> different signature -> newly seen.
        assert!(ledger.record_tool_call("open_section", &serde_json::json!({ "query": "x" })));
    }

    #[test]
    fn signature_matches_legacy_format() {
        let args = serde_json::json!({ "page": 1, "mode": "header" });
        assert_eq!(
            RetrievalLedger::signature("open_pages", &args),
            format!("open_pages:{args}")
        );
    }

    #[test]
    fn attempted_signatures_lists_recorded_calls() {
        let mut ledger = RetrievalLedger::new();
        ledger.record_tool_call("open_section", &serde_json::json!({ "query": "a" }));
        ledger.record_tool_call("search_chunks", &serde_json::json!({ "query": "b" }));
        let signatures = ledger.attempted_signatures().cloned().collect::<Vec<_>>();
        assert_eq!(signatures.len(), 2);
        assert!(signatures.contains(&RetrievalLedger::signature(
            "open_section",
            &serde_json::json!({ "query": "a" })
        )));
    }

    #[test]
    fn coverage_summary_groups_by_region_kind() {
        let mut ledger = RetrievalLedger::new();
        assert!(ledger.coverage_summary().is_none());
        ledger.record_coverage(&[
            citation("open_pages", 1, Some("Introduction")),
            citation("open_pages", 2, Some("Introduction")),
            citation("open_table", 3, Some("Table 3")),
            citation("open_visual", 4, Some("Figure 1")),
        ]);
        let summary = ledger.coverage_summary().expect("non-empty coverage");
        assert!(summary.contains("pages 1, 2"), "summary={summary}");
        assert!(summary.contains("sections: Introduction"), "summary={summary}");
        assert!(summary.contains("tables: Table 3"), "summary={summary}");
        assert!(summary.contains("visuals: Figure 1"), "summary={summary}");
    }

    #[test]
    fn coverage_classifies_all_real_table_and_visual_sources() {
        // These are the actual Citation `source` strings the RAG tools emit;
        // they must land under tables/visuals, not be misfiled as sections.
        let mut ledger = RetrievalLedger::new();
        for source in ["table_fact", "open_table_context", "inspect_tables", "table_anchor"] {
            ledger.record_coverage(&[citation(source, 5, Some("Table 2"))]);
        }
        for source in [
            "visual_asset",
            "visual_anchor",
            "inspect_objects",
            "caption",
            "analyze_visual",
            "analyze_page",
        ] {
            ledger.record_coverage(&[citation(source, 6, Some("Figure 4"))]);
        }
        let summary = ledger.coverage_summary().expect("non-empty coverage");
        assert!(summary.contains("tables: Table 2"), "summary={summary}");
        assert!(summary.contains("visuals: Figure 4"), "summary={summary}");
        assert!(
            !summary.contains("sections:"),
            "table/visual sources must not be misfiled as sections; summary={summary}"
        );
    }
}
