//! Shared retrieval ledger — the single "what have I already looked at" record
//! for one agent turn.
//!
//! Before Loop V2 the M3 rule loop (`turn_runner`) and the M4 LLM-judge loop
//! (`agent_judge`) each kept their own ad-hoc dedupe state (a `HashSet` of tool
//! signatures, plus a `Vec<String>` of judge feedback). That duplicated the same
//! concept twice and meant neither loop knew what the other had already read.
//!
//! `RetrievalLedger` unifies that state:
//! - tool-call de-duplication (signature set),
//! - coverage tracking (which pages / sections / tables / visuals were read).
//!
//! Used per-loop it is behavior-preserving (same signature format, same dedupe
//! semantics as the old `HashSet`s). When Loop V2 is enabled the *same* ledger is
//! threaded from the M3 loop into the M4 loop so the judge inherits the M3
//! coverage and never re-requests an already-seen tool.

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
                match citation.source.as_str() {
                    "open_table" | "current_view_table" => {
                        self.tables.insert(section.to_string());
                    }
                    "open_visual" | "inspect_visuals" => {
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
}
