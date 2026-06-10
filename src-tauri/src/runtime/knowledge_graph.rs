//! Cross-document knowledge graph (P2) — read-side aggregation over the
//! precipitated artifacts (Stream 1) and conversation edges (Stream 2).
//!
//! `get_knowledge_graph` returns the raw material (documents + entity/concept
//! artifacts + doc<->doc links); the frontend builds nodes/edges, relevance and
//! communities (mirrors llm_wiki's front-end graph construction).
//!
//! `get_related_documents` is the reading-time "related papers" query: rank other
//! documents by shared concepts/entities + co-citation, with the shared concept
//! names as the human-readable reason.

use rusqlite::params;
use serde::Serialize;

use crate::AppDatabase;

// Relevance weights (adapted from llm_wiki's graph-relevance.ts, tuned for our
// concept-overlap domain). co_citation is the strongest signal (a real reasoning
// link); Adamic-Adar rewards RARE shared concepts; raw overlap is the baseline.
const W_OVERLAP: f64 = 1.0; // per shared concept/entity (sourceOverlap-like)
const W_ADAMIC_ADAR: f64 = 2.0; // rare shared concepts weigh more (common-neighbor)
const W_CO_CITATION: f64 = 3.0; // conversation co-citation (directLink-like)
const ENTITY_AFFINITY: f64 = 1.3; // a shared entity (method/dataset) > a shared generic concept
const RELATED_LIMIT: usize = 12;
const MAX_SHARED_NAMES: usize = 6;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphDocument {
    id: String,
    title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphArtifact {
    document_id: String,
    kind: String,
    name: String,
    normalized: String,
    salience: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphLink {
    doc_a: String,
    doc_b: String,
    basis: String,
    weight: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeGraph {
    documents: Vec<GraphDocument>,
    artifacts: Vec<GraphArtifact>,
    links: Vec<GraphLink>,
}

/// Whole-library graph material. Only documents that carry knowledge artifacts
/// are included (a doc with no precipitated concepts has nothing to connect on).
#[tauri::command]
pub(crate) fn get_knowledge_graph(
    database: tauri::State<'_, AppDatabase>,
) -> Result<KnowledgeGraph, String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;

    // Documents that have at least one entity/concept artifact.
    let documents = {
        let mut stmt = conn
            .prepare(
                "SELECT d.id, d.title FROM documents d
                 WHERE EXISTS (
                   SELECT 1 FROM document_artifacts a
                   WHERE a.document_id = d.id AND a.kind IN ('entity','concept')
                 )
                 ORDER BY d.title",
            )
            .map_err(|err| format!("Failed to prepare documents query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GraphDocument {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })
            .map_err(|err| format!("Failed to query documents: {err}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };

    let artifacts = {
        let mut stmt = conn
            .prepare(
                "SELECT document_id, kind, name, normalized, salience
                 FROM document_artifacts
                 WHERE kind IN ('entity','concept') AND normalized != ''
                 ORDER BY salience DESC",
            )
            .map_err(|err| format!("Failed to prepare artifacts query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GraphArtifact {
                    document_id: row.get(0)?,
                    kind: row.get(1)?,
                    name: row.get(2)?,
                    normalized: row.get(3)?,
                    salience: row.get(4)?,
                })
            })
            .map_err(|err| format!("Failed to query artifacts: {err}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };

    let links = {
        let mut stmt = conn
            .prepare(
                "SELECT doc_a, doc_b, basis, weight FROM document_links
                 ORDER BY weight DESC",
            )
            .map_err(|err| format!("Failed to prepare links query: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GraphLink {
                    doc_a: row.get(0)?,
                    doc_b: row.get(1)?,
                    basis: row.get(2)?,
                    weight: row.get(3)?,
                })
            })
            .map_err(|err| format!("Failed to query links: {err}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };

    Ok(KnowledgeGraph {
        documents,
        artifacts,
        links,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RelatedDocument {
    pub document_id: String,
    pub title: String,
    pub score: f64,
    pub shared_concepts: Vec<String>,
    pub co_citation: f64,
}

/// Documents related to `document_id`, ranked by shared concepts/entities +
/// co-citation. The shared concept names are the "why related" reason shown in
/// the reading-time panel.
#[tauri::command]
pub(crate) fn get_related_documents(
    document_id: String,
    database: tauri::State<'_, AppDatabase>,
) -> Result<Vec<RelatedDocument>, String> {
    let document_id = document_id.trim().to_string();
    if document_id.is_empty() {
        return Ok(Vec::new());
    }
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    related_documents(&conn, &document_id)
}

/// Core ranking used by both the command and the `query_knowledge_graph` RAG tool.
/// Caller holds the connection lock.
pub(crate) fn related_documents(
    conn: &rusqlite::Connection,
    document_id: &str,
) -> Result<Vec<RelatedDocument>, String> {
    let document_id = document_id.trim();
    if document_id.is_empty() {
        return Ok(Vec::new());
    }

    use std::collections::{HashMap, HashSet};
    struct Acc {
        title: String,
        seen: HashSet<String>, // normalized keys already counted for this doc
        names: Vec<String>,    // display names of shared concepts
        overlap: f64,          // weighted shared-concept count (entity affinity)
        adamic_adar: f64,      // sum 1/ln(docFreq): rare shared concepts weigh more
        co_citation: f64,
    }
    impl Acc {
        fn new(title: String) -> Self {
            Acc {
                title,
                seen: HashSet::new(),
                names: Vec::new(),
                overlap: 0.0,
                adamic_adar: 0.0,
                co_citation: 0.0,
            }
        }
    }
    let mut acc: HashMap<String, Acc> = HashMap::new();

    // Document frequency per normalized concept/entity (for Adamic-Adar rarity).
    let doc_freq: HashMap<String, i64> = {
        let mut stmt = conn
            .prepare(
                "SELECT normalized, COUNT(DISTINCT document_id)
                 FROM document_artifacts
                 WHERE kind IN ('entity','concept') AND normalized <> ''
                 GROUP BY normalized",
            )
            .map_err(|err| format!("Failed to prepare doc-frequency query: {err}"))?;
        let map = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|err| format!("Failed to query doc frequency: {err}"))?
            .filter_map(Result::ok)
            .collect::<HashMap<_, _>>();
        map
    };

    // Shared concepts/entities: another document is related when it shares a
    // normalized artifact with the focus document. Each shared item contributes
    // an overlap term + an Adamic-Adar term (rarer concept → larger).
    {
        let mut stmt = conn
            .prepare(
                "SELECT b.document_id, d.title, a.name, a.normalized, a.kind
                 FROM document_artifacts a
                 JOIN document_artifacts b
                   ON a.normalized = b.normalized
                  AND a.kind = b.kind
                  AND b.document_id <> a.document_id
                 JOIN documents d ON d.id = b.document_id
                 WHERE a.document_id = ?1
                   AND a.kind IN ('entity','concept')
                   AND a.normalized <> ''",
            )
            .map_err(|err| format!("Failed to prepare related query: {err}"))?;
        let rows = stmt
            .query_map(params![document_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|err| format!("Failed to query related: {err}"))?;
        for row in rows.filter_map(Result::ok) {
            let (other_id, title, name, normalized, kind) = row;
            let entry = acc.entry(other_id).or_insert_with(|| Acc::new(title));
            if !entry.seen.insert(normalized.clone()) {
                continue; // already counted this shared concept for this doc
            }
            let affinity = if kind == "entity" { ENTITY_AFFINITY } else { 1.0 };
            entry.overlap += affinity;
            let df = (*doc_freq.get(&normalized).unwrap_or(&2)).max(2) as f64;
            entry.adamic_adar += affinity / df.ln();
            if entry.names.len() < 64 {
                entry.names.push(name);
            }
        }
    }

    // Co-citation edges that involve the focus document.
    {
        let mut stmt = conn
            .prepare(
                "SELECT CASE WHEN l.doc_a = ?1 THEN l.doc_b ELSE l.doc_a END AS other_id,
                        d.title, l.weight
                 FROM document_links l
                 JOIN documents d
                   ON d.id = CASE WHEN l.doc_a = ?1 THEN l.doc_b ELSE l.doc_a END
                 WHERE l.basis = 'co_citation' AND (l.doc_a = ?1 OR l.doc_b = ?1)",
            )
            .map_err(|err| format!("Failed to prepare co-citation query: {err}"))?;
        let rows = stmt
            .query_map(params![document_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|err| format!("Failed to query co-citation: {err}"))?;
        for row in rows.filter_map(Result::ok) {
            let (other_id, title, weight) = row;
            let entry = acc.entry(other_id).or_insert_with(|| Acc::new(title));
            entry.co_citation += weight;
        }
    }

    let mut related: Vec<RelatedDocument> = acc
        .into_iter()
        .map(|(document_id, item)| {
            let score = item.overlap * W_OVERLAP
                + item.adamic_adar * W_ADAMIC_ADAR
                + item.co_citation * W_CO_CITATION;
            let mut shared = item.names;
            shared.truncate(MAX_SHARED_NAMES);
            RelatedDocument {
                document_id,
                title: item.title,
                score,
                shared_concepts: shared,
                co_citation: item.co_citation,
            }
        })
        .filter(|item| item.score > 0.0)
        .collect();

    related.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    related.truncate(RELATED_LIMIT);
    Ok(related)
}
