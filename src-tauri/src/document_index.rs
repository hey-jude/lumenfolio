use rusqlite::{params, OptionalExtension};
use std::{any::Any, panic::AssertUnwindSafe, path::PathBuf, time::Instant};
use tauri::{Emitter, Manager, State};

use crate::{
    diagnostics, indexing, pdf_index, runtime, visual_index, AgentSessionState, AppDatabase,
    DocumentIndexEventOutput, DocumentIndexResult, PageIndexInput, UpsertDocumentIndexInput,
    CURRENT_INDEX_VERSION, TEXT_INDEX_JOB_TYPE,
};

const TEXT_INDEX_STALE_HEARTBEAT_AFTER_SECS: i64 = 30 * 60;

#[tauri::command]
pub(crate) fn upsert_document_index(
    input: UpsertDocumentIndexInput,
    database: State<'_, AppDatabase>,
) -> Result<DocumentIndexResult, String> {
    upsert_document_index_job(input, &database)
}

fn upsert_document_index_job(
    input: UpsertDocumentIndexInput,
    database: &AppDatabase,
) -> Result<DocumentIndexResult, String> {
    upsert_document_index_job_with_progress(input, database, |_, _| {})
}

fn upsert_document_index_job_with_progress(
    input: UpsertDocumentIndexInput,
    database: &AppDatabase,
    mut on_page_written: impl FnMut(u32, u32),
) -> Result<DocumentIndexResult, String> {
    let document_id = input.document_id.clone();
    let page_count = input.page_count;
    let mut conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Failed to start index transaction: {err}"))?;

    let updated = tx
        .execute(
            "UPDATE documents
         SET page_count = ?2,
             index_status = 'indexed',
             index_version = ?3,
             updated_at = unixepoch()
         WHERE id = ?1",
            params![
                document_id.as_str(),
                input.page_count,
                CURRENT_INDEX_VERSION
            ],
        )
        .map_err(|err| format!("Failed to update document index status: {err}"))?;
    if updated == 0 {
        return Err("Document is no longer available for index upsert".to_string());
    }

    tx.execute(
        "DELETE FROM document_pages WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document pages: {err}"))?;
    tx.execute(
        "DELETE FROM document_blocks WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document blocks: {err}"))?;
    tx.execute(
        "DELETE FROM document_lines WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document lines: {err}"))?;
    tx.execute(
        "DELETE FROM document_text_units WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document text units: {err}"))?;
    tx.execute(
        "DELETE FROM document_layout_lines WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document layout lines: {err}"))?;
    tx.execute(
        "DELETE FROM document_layout_regions WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document layout regions: {err}"))?;
    tx.execute(
        "DELETE FROM document_outlines WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document outlines: {err}"))?;
    tx.execute(
        "DELETE FROM document_chunks WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document chunks: {err}"))?;
    tx.execute(
        "DELETE FROM document_chunks_fts WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document FTS rows: {err}"))?;
    tx.execute(
        "DELETE FROM document_table_facts_fts WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear table fact FTS rows: {err}"))?;
    tx.execute(
        "DELETE FROM document_table_facts WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear table facts: {err}"))?;
    tx.execute(
        "DELETE FROM document_table_cells
         WHERE table_id IN (SELECT id FROM document_tables WHERE document_id = ?1)",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear table cells: {err}"))?;
    tx.execute(
        "DELETE FROM document_tables WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear document tables: {err}"))?;
    tx.execute(
        "DELETE FROM document_visual_assets WHERE document_id = ?1",
        params![document_id.as_str()],
    )
    .map_err(|err| format!("Failed to clear visual assets: {err}"))?;

    let mut structure_blocks = Vec::new();
    for page in input.pages {
        let PageIndexInput {
            page_no,
            width,
            height,
            text: _raw_page_text,
            blocks,
            lines,
            units,
            regions,
        } = page;
        let line_inputs = lines.unwrap_or_default();
        for unit in units.unwrap_or_default() {
            let text = unit.text.trim();
            if text.is_empty() {
                continue;
            }
            let bbox_json = serde_json::to_string(&unit.bbox)
                .map_err(|err| format!("Failed to encode text unit bbox: {err}"))?;
            let unit_id = format!("{document_id}-p{page_no}-u{}", unit.source_order);
            tx.execute(
                "INSERT INTO document_text_units
                    (id, document_id, page_no, source_order, text, bbox_json, font_size, font_name, font_flags)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    unit_id.as_str(),
                    document_id.as_str(),
                    page_no,
                    unit.source_order,
                    text,
                    bbox_json.as_str(),
                    unit.font_size.max(0.0),
                    unit.font_name.trim(),
                    unit.font_flags
                ],
            )
            .map_err(|err| format!("Failed to insert document text unit: {err}"))?;
        }
        let normalized_blocks =
            indexing::normalize_page_blocks(&document_id, page_no, blocks, &line_inputs);
        let page_text = normalized_blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        tx.execute(
            "INSERT INTO document_pages (document_id, page_no, width, height, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![document_id.as_str(), page_no, width, height, page_text],
        )
        .map_err(|err| format!("Failed to insert document page: {err}"))?;

        for line in line_inputs {
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            let (block_id, block_index) = resolve_line_block_reference(
                &document_id,
                page_no,
                &line.bbox_list,
                &normalized_blocks,
            );
            let bbox_json = serde_json::to_string(&line.bbox_list)
                .map_err(|err| format!("Failed to encode line bbox: {err}"))?;
            let source_order_json =
                serde_json::to_string(&line.source_order_list.clone().unwrap_or_default())
                    .map_err(|err| format!("Failed to encode line source order: {err}"))?;
            let line_id = format!("{document_id}-p{page_no}-l{}", line.line_no);
            let line_region_index = line.region_index.unwrap_or(0);
            let line_region_id = layout_region_id(&document_id, page_no, line_region_index);
            let role_hint = line.role_hint.as_deref().unwrap_or_default().trim();
            let baseline = line.baseline.unwrap_or(0.0);
            tx.execute(
                "INSERT INTO document_layout_lines
                    (id, document_id, page_no, line_no, text, bbox_json, source_order_json, region_index, region_id, role_hint, baseline)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    line_id.as_str(),
                    document_id.as_str(),
                    page_no,
                    line.line_no,
                    text,
                    bbox_json.as_str(),
                    source_order_json.as_str(),
                    line_region_index,
                    line_region_id.as_str(),
                    role_hint,
                    if baseline.is_finite() { baseline } else { 0.0 }
                ],
            )
            .map_err(|err| format!("Failed to insert document layout line: {err}"))?;
            tx.execute(
                "INSERT INTO document_lines
                    (id, document_id, page_no, line_no, block_id, block_index, text, bbox_json, source_order_json, region_index, region_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    line_id.as_str(),
                    document_id.as_str(),
                    page_no,
                    line.line_no,
                    block_id.as_str(),
                    block_index,
                    text,
                    bbox_json.as_str(),
                    source_order_json.as_str(),
                    line_region_index,
                    line_region_id.as_str()
                ],
            )
            .map_err(|err| format!("Failed to insert document line: {err}"))?;
        }

        for region in regions.unwrap_or_default() {
            if region.region_index == 0 {
                continue;
            }
            let bbox_json = serde_json::to_string(&region.bbox)
                .map_err(|err| format!("Failed to encode layout region bbox: {err}"))?;
            let line_numbers_json = serde_json::to_string(&region.line_numbers)
                .map_err(|err| format!("Failed to encode layout region line numbers: {err}"))?;
            let region_id = layout_region_id(&document_id, page_no, region.region_index);
            tx.execute(
                "INSERT INTO document_layout_regions
                    (id, document_id, page_no, region_index, kind, bbox_json, line_numbers_json, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    region_id.as_str(),
                    document_id.as_str(),
                    page_no,
                    region.region_index,
                    region.kind.trim(),
                    bbox_json.as_str(),
                    line_numbers_json.as_str(),
                    region.confidence.clamp(0.0, 1.0),
                ],
            )
            .map_err(|err| format!("Failed to insert document layout region: {err}"))?;
        }

        for block in normalized_blocks {
            let bbox_json = serde_json::to_string(&block.bbox_list)
                .map_err(|err| format!("Failed to encode block bbox: {err}"))?;
            let region_id = layout_region_id(&document_id, page_no, block.region_index);
            structure_blocks.push(runtime::rag::StructureBlockSeed {
                page_no,
                block_index: block.block_index,
                text: block.text.clone(),
                bbox_list: block.bbox_list.clone(),
                role: block.role.clone(),
                region_index: block.region_index,
                region_id: region_id.clone(),
            });
            tx.execute(
                "INSERT INTO document_blocks
                    (id, document_id, page_no, block_index, text, bbox_json, block_role, region_index, region_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    block.id.as_str(),
                    document_id.as_str(),
                    page_no,
                    block.block_index,
                    block.text.as_str(),
                    bbox_json.as_str(),
                    block.role.as_str(),
                    block.region_index,
                    region_id.as_str()
                ],
            )
            .map_err(|err| format!("Failed to insert document block: {err}"))?;

            let chunk_id = format!("chunk-{}", block.id);
            tx.execute(
                "INSERT INTO document_chunks
                    (id, document_id, page_no, block_ids_json, text, bbox_refs_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    chunk_id.as_str(),
                    document_id.as_str(),
                    page_no,
                    serde_json::json!([block.id]).to_string(),
                    block.text.as_str(),
                    bbox_json.as_str()
                ],
            )
            .map_err(|err| format!("Failed to insert document chunk: {err}"))?;
            tx.execute(
                "INSERT INTO document_chunks_fts (chunk_id, document_id, text)
                 VALUES (?1, ?2, ?3)",
                params![chunk_id.as_str(), document_id.as_str(), block.text.as_str()],
            )
            .map_err(|err| format!("Failed to insert FTS row: {err}"))?;
        }
        on_page_written(page_no, page_count);
    }
    let outlines = input
        .outlines
        .unwrap_or_default()
        .into_iter()
        .filter_map(|outline| {
            let title = outline.title.trim();
            if title.is_empty() || outline.page_start == 0 {
                return None;
            }
            Some(runtime::rag::OutlineSeed {
                title: title.to_string(),
                level: outline.level.max(1),
                page_no: outline.page_start,
                order_index: outline.order_index,
            })
        })
        .collect::<Vec<_>>();
    let outline_ranges =
        runtime::rag::outline_ranges(&outlines, input.page_count, &structure_blocks);
    for outline in &outline_ranges {
        let outline_id = format!(
            "{document_id}-outline-{}",
            outline.order_index.saturating_add(1)
        );
        tx.execute(
            "INSERT INTO document_outlines
                (id, document_id, title, level, page_start, page_end, order_index, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), unixepoch())",
            params![
                outline_id.as_str(),
                document_id.as_str(),
                outline.title.as_str(),
                outline.level,
                outline.page_no,
                outline.page_end,
                outline.order_index,
            ],
        )
        .map_err(|err| format!("Failed to insert document outline: {err}"))?;
    }
    runtime::rag::rebuild_structure_tree(&tx, &document_id, &structure_blocks, &outlines)?;
    visual_index::queue_visual_index_job(&tx, &document_id)?;

    let tree_ready = visual_index::document_tree_ready(&tx, &document_id)?;
    let visual_index = visual_index::visual_index_job_state(&tx, &document_id)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit document index: {err}"))?;
    Ok(DocumentIndexResult {
        page_count,
        index_version: CURRENT_INDEX_VERSION,
        tree_ready,
        visual_index_status: visual_index.status,
        visual_index_version: visual_index.version,
        visual_index_error: visual_index.error,
    })
}

fn layout_region_id(document_id: &str, page_no: u32, region_index: u32) -> String {
    if region_index == 0 {
        String::new()
    } else {
        format!("{document_id}-p{page_no}-r{region_index}")
    }
}

#[tauri::command]
pub(crate) fn repair_document_index_from_cache(
    document_id: String,
    database: State<'_, AppDatabase>,
    agent_sessions: State<'_, AgentSessionState>,
) -> Result<DocumentIndexResult, String> {
    repair_document_index_from_cache_job(&document_id, &database, &agent_sessions)
}

#[tauri::command]
pub(crate) fn enqueue_document_reindex(
    document_id: String,
    app: tauri::AppHandle,
) -> Result<DocumentIndexEventOutput, String> {
    let document_id = document_id.trim().to_string();
    if document_id.is_empty() {
        return Err("No document selected for reindex".to_string());
    }

    let database = app.state::<AppDatabase>();
    let queued_now = queue_text_index_job(&document_id, &database)?;
    let queued = DocumentIndexEventOutput {
        document_id: document_id.clone(),
        status: "queued".to_string(),
        progress_percent: 0,
        stage: "queued".to_string(),
        stage_label: "Queued".to_string(),
        page_count: 0,
        index_version: 0,
        tree_ready: false,
        visual_index_status: "pending".to_string(),
        visual_index_version: 0,
        visual_index_error: String::new(),
        error: String::new(),
    };
    if !queued_now {
        return Ok(queued);
    }

    emit_document_index_event(&app, queued.clone());
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let started_at = Instant::now();
        diagnostics::append_index_debug(
            &task_app,
            "reindex_start",
            serde_json::json!({
                "documentId": document_id.as_str(),
                "mode": "pdfium_full_reindex",
                "indexVersion": CURRENT_INDEX_VERSION,
            }),
        );
        emit_document_index_event(
            &task_app,
            DocumentIndexEventOutput {
                document_id: document_id.clone(),
                status: "indexing".to_string(),
                progress_percent: 1,
                stage: "starting".to_string(),
                stage_label: "Starting".to_string(),
                page_count: 0,
                index_version: 0,
                tree_ready: false,
                visual_index_status: "pending".to_string(),
                visual_index_version: 0,
                visual_index_error: String::new(),
                error: String::new(),
            },
        );
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let database = task_app.state::<AppDatabase>();
            let agent_sessions = task_app.state::<AgentSessionState>();
            run_document_reindex_job(&document_id, &database, &agent_sessions, &task_app)
        }))
        .unwrap_or_else(|panic| {
            let error = format!(
                "Document reindex worker panicked: {}",
                panic_payload_message(panic.as_ref())
            );
            let database = task_app.state::<AppDatabase>();
            if let Err(mark_err) = mark_text_index_job_failed(&document_id, &database, &error) {
                log::warn!("Failed to mark panicked text index job failed: {mark_err}");
            }
            diagnostics::append_index_debug(
                &task_app,
                "reindex_panic",
                serde_json::json!({
                    "documentId": document_id.as_str(),
                    "error": error.as_str(),
                }),
            );
            Err(error)
        });
        let event = match result {
            Ok(result) => DocumentIndexEventOutput {
                document_id,
                status: "indexed".to_string(),
                progress_percent: 100,
                stage: "complete".to_string(),
                stage_label: "Complete".to_string(),
                page_count: result.page_count,
                index_version: result.index_version,
                tree_ready: result.tree_ready,
                visual_index_status: result.visual_index_status,
                visual_index_version: result.visual_index_version,
                visual_index_error: result.visual_index_error,
                error: String::new(),
            },
            Err(err) => DocumentIndexEventOutput {
                document_id,
                status: "failed".to_string(),
                progress_percent: 100,
                stage: "failed".to_string(),
                stage_label: "Failed".to_string(),
                page_count: 0,
                index_version: 0,
                tree_ready: false,
                visual_index_status: "pending".to_string(),
                visual_index_version: 0,
                visual_index_error: String::new(),
                error: err,
            },
        };
        diagnostics::append_index_debug(
            &task_app,
            "reindex_finish",
            serde_json::json!({
                "documentId": event.document_id.as_str(),
                "status": event.status.as_str(),
                "pageCount": event.page_count,
                "indexVersion": event.index_version,
                "treeReady": event.tree_ready,
                "elapsedMs": started_at.elapsed().as_millis(),
                "error": event.error.as_str(),
            }),
        );
        emit_document_index_event(&task_app, event);
    });
    Ok(queued)
}

pub(super) fn queue_text_index_job(
    document_id: &str,
    database: &AppDatabase,
) -> Result<bool, String> {
    let mut conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Failed to start text index queue transaction: {err}"))?;
    let existing = tx
        .query_row(
            "SELECT status,
                    CASE
                      WHEN status IN ('queued', 'running')
                       AND COALESCE(NULLIF(updated_at, 0), NULLIF(started_at, 0), created_at)
                           <= unixepoch() - ?3
                      THEN 1
                      ELSE 0
                    END AS is_stale
             FROM document_index_jobs
             WHERE document_id = ?1 AND job_type = ?2",
            params![
                document_id,
                TEXT_INDEX_JOB_TYPE,
                TEXT_INDEX_STALE_HEARTBEAT_AFTER_SECS
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()
        .map_err(|err| format!("Failed to inspect text index job: {err}"))?;
    if let Some((status, is_stale)) = existing {
        if matches!(status.as_str(), "queued" | "running") && !is_stale {
            return Ok(false);
        }
        if matches!(status.as_str(), "queued" | "running") {
            tx.execute(
                "UPDATE document_index_jobs
                 SET status = 'failed',
                     error = 'No index heartbeat before completion; reindex can be retried',
                     updated_at = unixepoch(),
                     finished_at = unixepoch()
                 WHERE document_id = ?1 AND job_type = ?2",
                params![document_id, TEXT_INDEX_JOB_TYPE],
            )
            .map_err(|err| format!("Failed to expire stale text index job: {err}"))?;
        }
    }

    let document_exists = tx
        .query_row(
            "SELECT 1 FROM documents WHERE id = ?1 LIMIT 1",
            params![document_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| format!("Failed to inspect document before queueing index: {err}"))?
        .is_some();
    if !document_exists {
        return Err("Document is no longer available for reindex".to_string());
    }
    tx.execute(
        "INSERT INTO document_index_jobs
            (id, document_id, job_type, status, version, attempts, error, created_at, updated_at, started_at, finished_at)
         VALUES (?1, ?2, ?3, 'queued', ?4, 0, '', unixepoch(), unixepoch(), 0, 0)
         ON CONFLICT(document_id, job_type) DO UPDATE SET
           status = 'queued',
           version = excluded.version,
           error = '',
           updated_at = unixepoch(),
           started_at = 0,
           finished_at = 0",
        params![
            format!("index-job-{document_id}-text-pdf"),
            document_id,
            TEXT_INDEX_JOB_TYPE,
            CURRENT_INDEX_VERSION,
        ],
    )
    .map_err(|err| format!("Failed to queue text index job: {err}"))?;
    tx.commit()
        .map_err(|err| format!("Failed to commit text index queue: {err}"))?;
    Ok(true)
}

fn run_document_reindex_job(
    document_id: &str,
    database: &AppDatabase,
    agent_sessions: &AgentSessionState,
    app: &tauri::AppHandle,
) -> Result<DocumentIndexResult, String> {
    emit_reindex_progress(
        app,
        database,
        document_id,
        2,
        "mark_running",
        "Preparing index job",
    );
    append_reindex_stage(
        app,
        document_id,
        "mark_running_start",
        serde_json::json!({}),
    );
    mark_text_index_job_running(document_id, database)?;
    emit_reindex_progress(
        app,
        database,
        document_id,
        5,
        "mark_running",
        "Preparing index job",
    );
    append_reindex_stage(app, document_id, "mark_running_done", serde_json::json!({}));
    let pdf_path = document_pdf_path_from_db(document_id, database)?;
    emit_reindex_progress(
        app,
        database,
        document_id,
        8,
        "pdfium_extract",
        "Extracting PDF text",
    );
    append_reindex_stage(
        app,
        document_id,
        "pdfium_extract_start",
        serde_json::json!({
            "pdfPath": pdf_path.display().to_string(),
        }),
    );
    let input = pdf_index::build_document_index_from_pdfium_with_progress(
        document_id,
        &pdf_path,
        |progress| {
            let percent = progress_percent(10, 65, progress.page_done, progress.page_total);
            let stage_label = if progress.page_finished {
                format!(
                    "Extracted PDF text page {}/{}",
                    progress.page_done, progress.page_total
                )
            } else {
                format!(
                    "Extracting PDF text page {}/{}",
                    progress.page_current, progress.page_total
                )
            };
            emit_reindex_progress(
                app,
                database,
                document_id,
                percent,
                "pdfium_extract",
                &stage_label,
            );
            append_reindex_stage(
                app,
                document_id,
                if progress.page_finished {
                    "pdfium_extract_page_done"
                } else {
                    "pdfium_extract_page_start"
                },
                serde_json::json!({
                    "pageCurrent": progress.page_current,
                    "pageDone": progress.page_done,
                    "pageTotal": progress.page_total,
                    "blockCount": progress.block_count,
                    "lineCount": progress.line_count,
                    "layoutConfidence": progress.layout_confidence,
                    "regionCount": progress.region_count,
                    "twoColumnDetected": progress.two_column_detected,
                }),
            );
            if progress.page_finished
                && progress.layout_confidence.is_some_and(|value| value < 0.65)
            {
                append_reindex_stage(
                    app,
                    document_id,
                    "pdfium_layout_low_confidence",
                    serde_json::json!({
                        "pageCurrent": progress.page_current,
                        "pageDone": progress.page_done,
                        "pageTotal": progress.page_total,
                        "layoutConfidence": progress.layout_confidence,
                        "regionCount": progress.region_count,
                        "twoColumnDetected": progress.two_column_detected,
                    }),
                );
            }
        },
    )?;
    let (page_count, block_count, line_count) = index_input_counts(&input);
    append_reindex_stage(
        app,
        document_id,
        "pdfium_extract_done",
        serde_json::json!({
            "pageCount": page_count,
            "blockCount": block_count,
            "lineCount": line_count,
        }),
    );
    append_reindex_stage(
        app,
        document_id,
        "upsert_start",
        serde_json::json!({
            "pageCount": page_count,
            "blockCount": block_count,
            "lineCount": line_count,
        }),
    );
    emit_reindex_progress(
        app,
        database,
        document_id,
        70,
        "upsert",
        "Writing local index",
    );
    let result =
        upsert_document_index_job_with_progress(input, database, |page_done, page_total| {
            let percent = progress_percent(72, 94, page_done, page_total);
            emit_reindex_progress_event(
                app,
                document_id,
                percent,
                "upsert",
                &format!("Writing local index page {page_done}/{page_total}"),
            );
        });
    if result.is_ok() {
        agent_sessions.clear_document(document_id);
    }

    match result {
        Ok(result) => {
            append_reindex_stage(
                app,
                document_id,
                "upsert_done",
                serde_json::json!({
                    "pageCount": result.page_count,
                    "indexVersion": result.index_version,
                    "treeReady": result.tree_ready,
                }),
            );
            emit_reindex_progress(
                app,
                database,
                document_id,
                97,
                "finalizing",
                "Finalizing index",
            );
            mark_text_index_job_succeeded(document_id, database)?;
            append_reindex_stage(
                app,
                document_id,
                "mark_succeeded_done",
                serde_json::json!({}),
            );
            Ok(result)
        }
        Err(err) => {
            mark_text_index_job_failed(document_id, database, &err)?;
            Err(err)
        }
    }
}

fn append_reindex_stage(
    app: &tauri::AppHandle,
    document_id: &str,
    stage: &str,
    extra: serde_json::Value,
) {
    let mut payload = serde_json::Map::from_iter([
        (
            "documentId".to_string(),
            serde_json::Value::String(document_id.to_string()),
        ),
        (
            "stage".to_string(),
            serde_json::Value::String(stage.to_string()),
        ),
    ]);
    if let serde_json::Value::Object(extra) = extra {
        payload.extend(extra);
    }
    diagnostics::append_index_debug(app, "reindex_stage", serde_json::Value::Object(payload));
}

fn emit_reindex_progress(
    app: &tauri::AppHandle,
    database: &AppDatabase,
    document_id: &str,
    progress_percent: u32,
    stage: &str,
    stage_label: &str,
) {
    if let Err(err) = mark_text_index_job_heartbeat(document_id, database) {
        log::warn!("Failed to update text index heartbeat: {err}");
    }
    emit_reindex_progress_event(app, document_id, progress_percent, stage, stage_label);
}

fn emit_reindex_progress_event(
    app: &tauri::AppHandle,
    document_id: &str,
    progress_percent: u32,
    stage: &str,
    stage_label: &str,
) {
    emit_document_index_event(
        app,
        DocumentIndexEventOutput {
            document_id: document_id.to_string(),
            status: "indexing".to_string(),
            progress_percent: progress_percent.min(99),
            stage: stage.to_string(),
            stage_label: stage_label.to_string(),
            page_count: 0,
            index_version: 0,
            tree_ready: false,
            visual_index_status: "pending".to_string(),
            visual_index_version: 0,
            visual_index_error: String::new(),
            error: String::new(),
        },
    );
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "unknown panic payload".to_string()
}

fn progress_percent(start: u32, end: u32, done: u32, total: u32) -> u32 {
    if total == 0 || done == 0 {
        return start.min(end);
    }
    let span = end.saturating_sub(start);
    let clamped_done = done.min(total);
    start + ((span as u64 * clamped_done as u64) / total as u64) as u32
}

fn index_input_counts(input: &UpsertDocumentIndexInput) -> (u32, usize, usize) {
    let block_count = input.pages.iter().map(|page| page.blocks.len()).sum();
    let line_count = input
        .pages
        .iter()
        .map(|page| page.lines.as_ref().map_or(0, Vec::len))
        .sum();
    (input.page_count, block_count, line_count)
}

fn document_pdf_path_from_db(document_id: &str, database: &AppDatabase) -> Result<PathBuf, String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let path = conn
        .query_row(
            "SELECT path FROM documents WHERE id = ?1 LIMIT 1",
            params![document_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| format!("Failed to load document path for reindex: {err}"))?
        .ok_or_else(|| "Document is no longer available for reindex".to_string())?;
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!("PDF is no longer available at {}", path.display()));
    }
    Ok(path)
}

fn mark_text_index_job_running(document_id: &str, database: &AppDatabase) -> Result<(), String> {
    let mut conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Failed to start text index running transaction: {err}"))?;
    let document_exists = tx
        .query_row(
            "SELECT 1 FROM documents WHERE id = ?1 LIMIT 1",
            params![document_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| format!("Failed to inspect document before running index: {err}"))?
        .is_some();
    if !document_exists {
        return Err("Document is no longer available for reindex".to_string());
    }
    tx.execute(
        "INSERT INTO document_index_jobs
            (id, document_id, job_type, status, version, attempts, error, created_at, updated_at, started_at, finished_at)
         VALUES (?1, ?2, ?3, 'running', ?4, 1, '', unixepoch(), unixepoch(), unixepoch(), 0)
         ON CONFLICT(document_id, job_type) DO UPDATE SET
           status = 'running',
           version = excluded.version,
           attempts = attempts + 1,
           error = '',
           updated_at = unixepoch(),
           started_at = unixepoch(),
           finished_at = 0",
        params![
            format!("index-job-{document_id}-text-pdf"),
            document_id,
            TEXT_INDEX_JOB_TYPE,
            CURRENT_INDEX_VERSION,
        ],
    )
    .map_err(|err| format!("Failed to mark text index job running: {err}"))?;
    tx.commit()
        .map_err(|err| format!("Failed to commit text index running state: {err}"))
}

fn mark_text_index_job_succeeded(document_id: &str, database: &AppDatabase) -> Result<(), String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    conn.execute(
        "UPDATE document_index_jobs
         SET status = 'succeeded',
             version = ?3,
             error = '',
             updated_at = unixepoch(),
             finished_at = unixepoch()
         WHERE document_id = ?1 AND job_type = ?2",
        params![document_id, TEXT_INDEX_JOB_TYPE, CURRENT_INDEX_VERSION],
    )
    .map_err(|err| format!("Failed to mark text index job succeeded: {err}"))?;
    Ok(())
}

fn mark_text_index_job_heartbeat(document_id: &str, database: &AppDatabase) -> Result<(), String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    conn.execute(
        "UPDATE document_index_jobs
         SET updated_at = unixepoch()
         WHERE document_id = ?1
           AND job_type = ?2
           AND status IN ('queued', 'running')",
        params![document_id, TEXT_INDEX_JOB_TYPE],
    )
    .map_err(|err| format!("Failed to update text index heartbeat: {err}"))?;
    Ok(())
}

fn mark_text_index_job_failed(
    document_id: &str,
    database: &AppDatabase,
    error: &str,
) -> Result<(), String> {
    let conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    conn.execute(
        "UPDATE document_index_jobs
         SET status = 'failed',
             error = ?3,
             updated_at = unixepoch(),
             finished_at = unixepoch()
         WHERE document_id = ?1 AND job_type = ?2",
        params![document_id, TEXT_INDEX_JOB_TYPE, error],
    )
    .map_err(|err| format!("Failed to mark text index job failed: {err}"))?;
    Ok(())
}

fn repair_document_index_from_cache_job(
    document_id: &str,
    database: &AppDatabase,
    agent_sessions: &AgentSessionState,
) -> Result<DocumentIndexResult, String> {
    let mut conn = database
        .conn
        .lock()
        .map_err(|_| "SQLite lock was poisoned".to_string())?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Failed to start cached index repair transaction: {err}"))?;

    let page_count = tx
        .query_row(
            "SELECT COALESCE(MAX(page_no), 0) FROM document_pages WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, u32>(0),
        )
        .map_err(|err| format!("Failed to inspect cached document pages: {err}"))?;
    let cached_index_version = tx
        .query_row(
            "SELECT index_version FROM documents WHERE id = ?1",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|err| format!("Failed to inspect cached document index version: {err}"))?
        .ok_or_else(|| "Document is no longer available for cached index repair".to_string())?;
    let text_unit_count = tx
        .query_row(
            "SELECT COUNT(*) FROM document_text_units WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| format!("Failed to inspect cached document text units: {err}"))?;
    let layout_region_count = tx
        .query_row(
            "SELECT COUNT(*) FROM document_layout_regions WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| format!("Failed to inspect cached document layout regions: {err}"))?;
    let layout_line_count = tx
        .query_row(
            "SELECT COUNT(*) FROM document_layout_lines WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| format!("Failed to inspect cached document layout lines: {err}"))?;
    let line_region_count = tx
        .query_row(
            "SELECT COUNT(*) FROM document_lines WHERE document_id = ?1 AND region_index > 0",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| format!("Failed to inspect cached document line regions: {err}"))?;
    if cached_index_version < CURRENT_INDEX_VERSION
        && (text_unit_count == 0
            || layout_line_count == 0
            || layout_region_count == 0
            || line_region_count == 0)
    {
        return Err(
            "Cached index lacks raw text units, layout lines, layout regions, or line-region links required by the current layout pipeline; reindex is required"
                .to_string(),
        );
    }
    let blocks = {
        let mut stmt = tx
            .prepare(
                "SELECT page_no, block_index, text, bbox_json, block_role, region_index, region_id
                 FROM document_blocks
                 WHERE document_id = ?1
                 ORDER BY page_no, block_index",
            )
            .map_err(|err| format!("Failed to prepare cached block read: {err}"))?;
        let rows = stmt
            .query_map(params![document_id], |row| {
                let bbox_json: String = row.get(3)?;
                Ok(runtime::rag::StructureBlockSeed {
                    page_no: row.get(0)?,
                    block_index: row.get(1)?,
                    text: row.get(2)?,
                    bbox_list: serde_json::from_str(&bbox_json)
                        .unwrap_or_else(|_| serde_json::json!([])),
                    role: row.get(4)?,
                    region_index: row.get(5)?,
                    region_id: row.get(6)?,
                })
            })
            .map_err(|err| format!("Failed to read cached blocks: {err}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("Failed to collect cached blocks: {err}"))?
    };
    if blocks.is_empty() {
        return Err("No cached document blocks are available for index repair".to_string());
    }

    let outlines = {
        let mut stmt = tx
            .prepare(
                "SELECT title, level, page_start, order_index
                 FROM document_outlines
                 WHERE document_id = ?1
                 ORDER BY order_index, page_start, level",
            )
            .map_err(|err| format!("Failed to prepare cached outline read: {err}"))?;
        let rows = stmt
            .query_map(params![document_id], |row| {
                Ok(runtime::rag::OutlineSeed {
                    title: row.get(0)?,
                    level: row.get::<_, u32>(1)?.max(1),
                    page_no: row.get::<_, u32>(2)?.max(1),
                    order_index: row.get(3)?,
                })
            })
            .map_err(|err| format!("Failed to read cached outlines: {err}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("Failed to collect cached outlines: {err}"))?
    };

    runtime::rag::rebuild_structure_tree(&tx, document_id, &blocks, &outlines)?;
    visual_index::queue_visual_index_job(&tx, document_id)?;
    let tree_ready = visual_index::document_tree_ready(&tx, document_id)?;
    let visual_index = visual_index::visual_index_job_state(&tx, document_id)?;
    let updated = tx
        .execute(
            "UPDATE documents
         SET page_count = CASE WHEN ?2 > 0 THEN ?2 ELSE page_count END,
             index_status = 'indexed',
             index_version = ?3,
             updated_at = unixepoch()
         WHERE id = ?1",
            params![document_id, page_count, CURRENT_INDEX_VERSION],
        )
        .map_err(|err| format!("Failed to update repaired document index status: {err}"))?;
    if updated == 0 {
        return Err("Document is no longer available for cached index repair".to_string());
    }
    tx.commit()
        .map_err(|err| format!("Failed to commit cached index repair: {err}"))?;
    agent_sessions.clear_document(document_id);
    Ok(DocumentIndexResult {
        page_count,
        index_version: CURRENT_INDEX_VERSION,
        tree_ready,
        visual_index_status: visual_index.status,
        visual_index_version: visual_index.version,
        visual_index_error: visual_index.error,
    })
}

fn emit_document_index_event(app: &tauri::AppHandle, event: DocumentIndexEventOutput) {
    if let Err(err) = app.emit("lumenfolio://document-index", event) {
        log::warn!("Failed to emit document-index event: {err}");
    }
}

fn resolve_line_block_reference(
    document_id: &str,
    page_no: u32,
    line_bbox_list: &serde_json::Value,
    blocks: &[indexing::NormalizedBlockIndex],
) -> (String, u32) {
    let line_rects = bbox_rects(line_bbox_list);
    if line_rects.is_empty() {
        return (String::new(), 0);
    }

    let mut best_block_index = 0;
    let mut best_score = 0.0_f64;
    for block in blocks {
        let block_rects = bbox_rects(&block.bbox_list);
        let score = line_rects
            .iter()
            .flat_map(|line_rect| {
                block_rects
                    .iter()
                    .map(move |block_rect| rect_overlap_area(line_rect, block_rect))
            })
            .sum::<f64>();
        if score > best_score {
            best_score = score;
            best_block_index = block.block_index;
        }
    }

    if best_score <= 0.0 || best_block_index == 0 {
        return (String::new(), 0);
    }
    (
        format!("{document_id}-p{page_no}-b{best_block_index}"),
        best_block_index,
    )
}

#[derive(Clone, Copy)]
struct BboxRect {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

fn bbox_rects(value: &serde_json::Value) -> Vec<BboxRect> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_array())
                .filter_map(|bbox| {
                    let x1 = bbox.first()?.as_f64()?;
                    let y1 = bbox.get(1)?.as_f64()?;
                    let x2 = bbox.get(2)?.as_f64()?;
                    let y2 = bbox.get(3)?.as_f64()?;
                    (x2 > x1 && y2 > y1).then_some(BboxRect { x1, y1, x2, y2 })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn rect_overlap_area(left: &BboxRect, right: &BboxRect) -> f64 {
    let width = (left.x2.min(right.x2) - left.x1.max(right.x1)).max(0.0);
    let height = (left.y2.min(right.y2) - left.y1.max(right.y1)).max(0.0);
    width * height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;
    use crate::{BlockIndexInput, LayoutRegionIndexInput, LineIndexInput, TextUnitIndexInput};
    use rusqlite::Connection;
    use std::{
        fs,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn text_index_queue_test_database(job_seed_sql: &str) -> AppDatabase {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let sql = format!(
            "CREATE TABLE documents (
              id TEXT PRIMARY KEY,
              index_status TEXT NOT NULL DEFAULT 'pending',
              index_version INTEGER NOT NULL DEFAULT 0,
              updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE document_index_jobs (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL,
              job_type TEXT NOT NULL,
              status TEXT NOT NULL DEFAULT 'pending',
              version INTEGER NOT NULL DEFAULT 0,
              attempts INTEGER NOT NULL DEFAULT 0,
              error TEXT NOT NULL DEFAULT '',
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              started_at INTEGER NOT NULL DEFAULT 0,
              finished_at INTEGER NOT NULL DEFAULT 0,
              UNIQUE(document_id, job_type)
            );
            INSERT INTO documents (id, index_status, index_version, updated_at)
            VALUES ('doc', 'pending', 0, 0);
            {job_seed_sql}"
        );
        conn.execute_batch(&sql).expect("schema");
        AppDatabase {
            conn: Mutex::new(conn),
        }
    }

    #[test]
    fn text_index_job_queue_rejects_duplicate_work() {
        let database = text_index_queue_test_database("");

        assert!(queue_text_index_job("doc", &database).expect("first queue"));
        assert!(!queue_text_index_job("doc", &database).expect("duplicate queue"));

        let conn = database.conn.lock().expect("db lock");
        let (document_status, job_status): (String, String) = conn
            .query_row(
                "SELECT d.index_status, j.status
                 FROM documents d
                 JOIN document_index_jobs j ON j.document_id = d.id
                 WHERE d.id = 'doc' AND j.job_type = ?1",
                params![TEXT_INDEX_JOB_TYPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("queued status");
        assert_eq!(document_status, "pending");
        assert_eq!(job_status, "queued");
    }

    #[test]
    fn text_index_job_queue_expires_stale_running_work() {
        let database = text_index_queue_test_database(
            "INSERT INTO document_index_jobs
              (id, document_id, job_type, status, version, attempts, error,
               created_at, updated_at, started_at, finished_at)
            VALUES ('job', 'doc', 'text_pdf', 'running', 17, 1, '',
                    unixepoch() - 3600, unixepoch() - 3600, unixepoch() - 3600, 0);",
        );

        assert!(queue_text_index_job("doc", &database).expect("stale requeue"));

        let conn = database.conn.lock().expect("db lock");
        let (document_status, job_status, error): (String, String, String) = conn
            .query_row(
                "SELECT d.index_status, j.status, j.error
                 FROM documents d
                 JOIN document_index_jobs j ON j.document_id = d.id
                 WHERE d.id = 'doc' AND j.job_type = ?1",
                params![TEXT_INDEX_JOB_TYPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("requeued status");
        assert_eq!(document_status, "pending");
        assert_eq!(job_status, "queued");
        assert!(error.is_empty());
    }

    #[test]
    fn text_index_job_queue_keeps_active_heartbeat_running() {
        let database = text_index_queue_test_database(
            "INSERT INTO document_index_jobs
              (id, document_id, job_type, status, version, attempts, error,
               created_at, updated_at, started_at, finished_at)
            VALUES ('job', 'doc', 'text_pdf', 'running', 17, 1, '',
                    unixepoch() - 3600, unixepoch(), unixepoch() - 3600, 0);",
        );

        assert!(!queue_text_index_job("doc", &database).expect("active heartbeat"));

        let conn = database.conn.lock().expect("db lock");
        let (job_status, attempts): (String, i64) = conn
            .query_row(
                "SELECT status, attempts
                 FROM document_index_jobs
                 WHERE document_id = 'doc' AND job_type = ?1",
                params![TEXT_INDEX_JOB_TYPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("active job status");
        assert_eq!(job_status, "running");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn progress_percent_clamps_done_to_total() {
        assert_eq!(progress_percent(10, 65, 0, 29), 10);
        assert_eq!(progress_percent(10, 65, 29, 29), 65);
        assert_eq!(progress_percent(10, 65, 30, 29), 65);
        assert_eq!(progress_percent(72, 94, 10, 0), 72);
    }

    #[test]
    fn upsert_document_index_persists_layout_debug_inputs() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "lumenfolio-text-units-{}-{suffix}.sqlite",
            std::process::id()
        ));
        let conn = storage::open_database(&db_path).expect("schema");
        conn.execute(
            "INSERT INTO workspace_roots
               (id, path, name, created_at, updated_at, last_opened_at)
             VALUES ('root', '/tmp/root', 'root', 1, 1, 1)",
            [],
        )
        .expect("root");
        conn.execute(
            "INSERT INTO documents
               (id, workspace_root_id, path, title, short_title, file_size, modified,
                page_count, last_page, index_status, index_version, created_at, updated_at, last_opened_at)
             VALUES ('doc', 'root', '/tmp/root/a.pdf', 'A', 'A', 1, 1, 0, 1, 'pending', 0, 1, 1, 1)",
            [],
        )
        .expect("document");
        let database = AppDatabase {
            conn: Mutex::new(conn),
        };
        let input = UpsertDocumentIndexInput {
            document_id: "doc".to_string(),
            page_count: 1,
            pages: vec![PageIndexInput {
                page_no: 1,
                width: 600.0,
                height: 800.0,
                text: "first second".to_string(),
                blocks: vec![BlockIndexInput {
                    id: "doc-p1-b1".to_string(),
                    block_index: 1,
                    text: "first second".to_string(),
                    bbox_list: serde_json::json!([[0.1, 0.1, 0.3, 0.2]]),
                    role: Some("body".to_string()),
                    region_index: Some(1),
                }],
                lines: Some(vec![LineIndexInput {
                    line_no: 1,
                    text: "first second".to_string(),
                    bbox_list: serde_json::json!([[0.1, 0.1, 0.2, 0.2], [0.2, 0.1, 0.3, 0.2]]),
                    source_order_list: Some(vec![3, 7]),
                    region_index: Some(1),
                    role_hint: Some("body".to_string()),
                    baseline: Some(160.0),
                }]),
                units: Some(vec![
                    TextUnitIndexInput {
                        source_order: 3,
                        text: "first".to_string(),
                        bbox: serde_json::json!([0.1, 0.1, 0.2, 0.2]),
                        font_size: 10.5,
                        font_name: "Times-Roman".to_string(),
                        font_flags: 32,
                    },
                    TextUnitIndexInput {
                        source_order: 7,
                        text: "second".to_string(),
                        bbox: serde_json::json!([0.2, 0.1, 0.3, 0.2]),
                        font_size: 10.5,
                        font_name: "Times-Roman".to_string(),
                        font_flags: 32,
                    },
                ]),
                regions: Some(vec![LayoutRegionIndexInput {
                    region_index: 1,
                    kind: "body_column".to_string(),
                    bbox: serde_json::json!([0.1, 0.1, 0.3, 0.2]),
                    line_numbers: vec![1],
                    confidence: 0.95,
                }]),
            }],
            outlines: None,
        };

        let result = upsert_document_index_job(input, &database).expect("index upsert");
        assert_eq!(result.index_version, CURRENT_INDEX_VERSION);

        let conn = database.conn.lock().expect("db lock");
        let unit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM document_text_units WHERE document_id = 'doc'",
                [],
                |row| row.get(0),
            )
            .expect("unit count");
        assert_eq!(unit_count, 2);
        let first_unit: (f64, String, i64) = conn
            .query_row(
                "SELECT font_size, font_name, font_flags
                 FROM document_text_units
                 WHERE document_id = 'doc' AND page_no = 1 AND source_order = 3",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("text unit metadata");
        assert_eq!(first_unit.0, 10.5);
        assert_eq!(first_unit.1, "Times-Roman");
        assert_eq!(first_unit.2, 32);
        let line_source_order: String = conn
            .query_row(
                "SELECT source_order_json FROM document_lines WHERE document_id = 'doc' AND page_no = 1 AND line_no = 1",
                [],
                |row| row.get(0),
            )
            .expect("line source order");
        assert_eq!(line_source_order, "[3,7]");
        let layout_line: (String, u32, String, String, f64) = conn
            .query_row(
                "SELECT source_order_json, region_index, region_id, role_hint, baseline
                 FROM document_layout_lines
                 WHERE document_id = 'doc' AND page_no = 1 AND line_no = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("layout line");
        assert_eq!(layout_line.0, "[3,7]");
        assert_eq!(layout_line.1, 1);
        assert_eq!(layout_line.2, "doc-p1-r1");
        assert_eq!(layout_line.3, "body");
        assert_eq!(layout_line.4, 160.0);
        let line_region: (u32, String) = conn
            .query_row(
                "SELECT region_index, region_id
                 FROM document_lines
                 WHERE document_id = 'doc' AND page_no = 1 AND line_no = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("line region");
        assert_eq!(line_region.0, 1);
        assert_eq!(line_region.1, "doc-p1-r1");
        let region: (String, String, f64) = conn
            .query_row(
                "SELECT kind, line_numbers_json, confidence
                 FROM document_layout_regions
                 WHERE document_id = 'doc' AND page_no = 1 AND region_index = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("layout region");
        assert_eq!(region.0, "body_column");
        assert_eq!(region.1, "[1]");
        assert_eq!(region.2, 0.95);
        let block_region: (u32, String) = conn
            .query_row(
                "SELECT region_index, region_id
                 FROM document_blocks
                 WHERE document_id = 'doc' AND page_no = 1 AND block_index = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("block region");
        assert_eq!(block_region.0, 1);
        assert_eq!(block_region.1, "doc-p1-r1");
        drop(conn);
        drop(database);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(db_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(db_path.with_extension("sqlite-shm"));
    }

    #[test]
    fn upsert_document_index_rejects_missing_document() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE documents (
              id TEXT PRIMARY KEY,
              page_count INTEGER NOT NULL DEFAULT 0,
              index_status TEXT NOT NULL DEFAULT 'pending',
              index_version INTEGER NOT NULL DEFAULT 0,
              updated_at INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("schema");
        let database = AppDatabase {
            conn: Mutex::new(conn),
        };
        let input = UpsertDocumentIndexInput {
            document_id: "missing".to_string(),
            page_count: 1,
            pages: Vec::new(),
            outlines: None,
        };

        let err = match upsert_document_index_job(input, &database) {
            Ok(_) => panic!("missing document should reject index upsert"),
            Err(err) => err,
        };

        assert!(err.contains("Document is no longer available"));
    }
}
