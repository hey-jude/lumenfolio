use std::collections::HashMap;
use std::sync::Mutex;

use super::memory::{
    preview_text, RecentTurnSummary, SectionContextMemory, SelectionContextMemory,
    SessionMemorySnapshot,
};

#[derive(Default)]
pub struct AgentSessionStore {
    sessions: Mutex<HashMap<String, AgentSessionState>>,
}

#[derive(Clone, Debug, Default)]
struct AgentSessionState {
    provider_id: Option<String>,
    turns: Vec<RecentTurnSummary>,
    last_selection: Option<SelectionContextMemory>,
    last_sections: Option<SectionContextMemory>,
}

impl AgentSessionStore {
    pub fn snapshot(&self, document_id: &str) -> Option<SessionMemorySnapshot> {
        let sessions = self.sessions.lock().ok()?;
        let state = sessions.get(document_id)?.clone();
        Some(SessionMemorySnapshot {
            document_id: document_id.to_string(),
            provider_id: state.provider_id,
            recent_turns: state.turns,
            selection: state.last_selection,
            sections: state.last_sections,
        })
    }

    pub fn record_turn(
        &self,
        document_id: &str,
        provider_id: Option<String>,
        mut turn: RecentTurnSummary,
    ) {
        if let Some(selected_text_preview) = &turn.selected_text_preview {
            turn.selected_text_preview = Some(preview_text(selected_text_preview, 200));
        }

        if let Ok(mut sessions) = self.sessions.lock() {
            let state = sessions.entry(document_id.to_string()).or_default();
            state.provider_id = provider_id;
            state.last_selection =
                turn.selected_text_preview
                    .as_ref()
                    .map(|text| SelectionContextMemory {
                        text_preview: text.clone(),
                        page: turn.citations.first().map(|citation| citation.page),
                    });
            state.last_sections = if turn.tree_titles.is_empty() {
                None
            } else {
                Some(SectionContextMemory {
                    titles: turn.tree_titles.clone(),
                })
            };
            state.turns.push(turn);
            if state.turns.len() > 12 {
                let drain_count = state.turns.len().saturating_sub(12);
                state.turns.drain(0..drain_count);
            }
        }
    }

    pub fn clear_document(&self, document_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(document_id);
        }
    }
}
