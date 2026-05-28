use serde::Serialize;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolName {
    InspectTree,
    OpenSection,
    SearchChunks,
    OpenPages,
    ExpandNeighbors,
    FinalizeAnswer,
}

#[cfg(test)]
impl AgentToolName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InspectTree => "inspect_tree",
            Self::OpenSection => "open_section",
            Self::SearchChunks => "search_chunks",
            Self::OpenPages => "open_pages",
            Self::ExpandNeighbors => "expand_neighbors",
            Self::FinalizeAnswer => "finalize_answer",
        }
    }
}
