//! Knowledge-base pivot (Collections): user-authored logical folders.
//!
//! Collections are a pure metadata layer — nestable, user-named, decoupled from
//! disk directories. A source's home is `documents.collection_id` (single
//! membership, folder metaphor; NULL = unfiled "Inbox"). Nothing here touches
//! files on disk: importing records a real path, filing only moves the pointer.

use rusqlite::params;
use tauri::State;

use crate::AppDatabase;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectionNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub position: i64,
}

fn new_collection_id() -> String {
    format!(
        "col-{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

fn normalize_parent(parent_id: Option<String>) -> Option<String> {
    parent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Load the whole collection forest (flat; the frontend nests by parent_id).
#[tauri::command]
pub(crate) fn load_collections(
    database: State<'_, AppDatabase>,
) -> Result<Vec<CollectionNode>, String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name, position
             FROM collections
             ORDER BY position ASC, lower(name) ASC",
        )
        .map_err(|err| format!("Failed to load collections: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CollectionNode {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                position: row.get(3)?,
            })
        })
        .map_err(|err| format!("Failed to load collections: {err}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("Failed to load collections: {err}"))?;
    Ok(rows)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateCollectionInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[tauri::command]
pub(crate) fn create_collection(
    input: CreateCollectionInput,
    database: State<'_, AppDatabase>,
) -> Result<CollectionNode, String> {
    let name = {
        let trimmed = input.name.trim();
        if trimmed.is_empty() {
            "Untitled".to_string()
        } else {
            trimmed.to_string()
        }
    };
    let parent_id = normalize_parent(input.parent_id);
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    if let Some(parent) = &parent_id {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM collections WHERE id = ?1",
                params![parent],
                |row| row.get(0),
            )
            .map_err(|err| format!("Failed to check parent collection: {err}"))?;
        if exists == 0 {
            return Err("Parent collection no longer exists".to_string());
        }
    }
    let position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM collections WHERE parent_id IS ?1",
            params![parent_id],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to compute collection position: {err}"))?;
    let id = new_collection_id();
    conn.execute(
        "INSERT INTO collections (id, parent_id, name, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, unixepoch(), unixepoch())",
        params![id, parent_id, name, position],
    )
    .map_err(|err| format!("Failed to create collection: {err}"))?;
    Ok(CollectionNode {
        id,
        parent_id,
        name,
        position,
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameCollectionInput {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[tauri::command]
pub(crate) fn rename_collection(
    input: RenameCollectionInput,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Collection name cannot be empty".to_string());
    }
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let affected = conn
        .execute(
            "UPDATE collections SET name = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![input.id.trim(), name],
        )
        .map_err(|err| format!("Failed to rename collection: {err}"))?;
    if affected == 0 {
        return Err("Collection not found".to_string());
    }
    Ok(())
}

/// Delete a collection and its whole subtree. Documents anywhere in that subtree
/// drop to unfiled (collection_id → NULL); the source rows and the files on disk
/// are never touched. Runs the subtree walk explicitly so behavior is identical
/// whether or not the SQLite build enforces the FKs.
#[tauri::command]
pub(crate) fn delete_collection(
    id: String,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    let id = id.trim().to_string();
    let mut conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Failed to start delete transaction: {err}"))?;
    const SUBTREE_CTE: &str = "WITH RECURSIVE sub(id) AS (
            SELECT ?1
            UNION ALL
            SELECT c.id FROM collections c JOIN sub ON c.parent_id = sub.id
        )";
    tx.execute(
        &format!(
            "UPDATE documents SET collection_id = NULL
             WHERE collection_id IN ({SUBTREE_CTE} SELECT id FROM sub)"
        ),
        params![id],
    )
    .map_err(|err| format!("Failed to unfile documents: {err}"))?;
    tx.execute(
        &format!("DELETE FROM collections WHERE id IN ({SUBTREE_CTE} SELECT id FROM sub)"),
        params![id],
    )
    .map_err(|err| format!("Failed to delete collection: {err}"))?;
    tx.commit()
        .map_err(|err| format!("Failed to commit collection delete: {err}"))?;
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveDocumentInput {
    pub document_id: String,
    #[serde(default)]
    pub collection_id: Option<String>,
}

#[tauri::command]
pub(crate) fn move_document_to_collection(
    input: MoveDocumentInput,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    let collection_id = normalize_parent(input.collection_id);
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    if let Some(target) = &collection_id {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM collections WHERE id = ?1",
                params![target],
                |row| row.get(0),
            )
            .map_err(|err| format!("Failed to check target collection: {err}"))?;
        if exists == 0 {
            return Err("Target collection no longer exists".to_string());
        }
    }
    let affected = conn
        .execute(
            "UPDATE documents SET collection_id = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![input.document_id.trim(), collection_id],
        )
        .map_err(|err| format!("Failed to move document: {err}"))?;
    if affected == 0 {
        return Err("Document not found".to_string());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveCollectionInput {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Reparent a collection. Rejects moving a collection under itself or one of its
/// own descendants (which would orphan a cycle).
#[tauri::command]
pub(crate) fn move_collection(
    input: MoveCollectionInput,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    let id = input.id.trim().to_string();
    let parent_id = normalize_parent(input.parent_id);
    if parent_id.as_deref() == Some(id.as_str()) {
        return Err("A collection cannot be its own parent".to_string());
    }
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    if let Some(target) = &parent_id {
        // Reject if `target` is inside `id`'s own subtree.
        let in_subtree: i64 = conn
            .query_row(
                "WITH RECURSIVE sub(id) AS (
                    SELECT ?1
                    UNION ALL
                    SELECT c.id FROM collections c JOIN sub ON c.parent_id = sub.id
                 )
                 SELECT COUNT(*) FROM sub WHERE id = ?2",
                params![id, target],
                |row| row.get(0),
            )
            .map_err(|err| format!("Failed to check for a cycle: {err}"))?;
        if in_subtree > 0 {
            return Err("Cannot move a collection into its own descendant".to_string());
        }
    }
    let affected = conn
        .execute(
            "UPDATE collections SET parent_id = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, parent_id],
        )
        .map_err(|err| format!("Failed to move collection: {err}"))?;
    if affected == 0 {
        return Err("Collection not found".to_string());
    }
    Ok(())
}
