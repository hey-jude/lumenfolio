use std::{path::Path, time::Duration};

use rusqlite::Connection;

/// Shared pragmas for every connection to the database. `busy_timeout` is opt-in:
/// the app connection deliberately leaves it at 0 (fail fast) so that a write which
/// loses the race to the dedicated index writer releases the shared `Mutex<Connection>`
/// immediately instead of holding it — and freezing reads — for the whole index.
fn configure_connection(conn: &Connection, busy_timeout: Option<Duration>) -> Result<(), String> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| format!("Failed to enable SQLite WAL: {err}"))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| format!("Failed to enable SQLite foreign keys: {err}"))?;
    if let Some(timeout) = busy_timeout {
        conn.busy_timeout(timeout)
            .map_err(|err| format!("Failed to set SQLite busy timeout: {err}"))?;
    }
    Ok(())
}

pub(crate) fn open_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|err| format!("Failed to open SQLite database {}: {err}", path.display()))?;
    // No busy_timeout: an app write contending with the index writer fails fast and
    // frees the shared mutex rather than blocking reads (reads never contend — WAL).
    configure_connection(&conn, None)?;
    migrate_database(&conn)?;
    reset_interrupted_index_jobs(&conn)?;
    Ok(conn)
}

/// A second connection to the same on-disk database used for the long-running
/// document index write. Keeping the heavy transaction off the app's shared
/// `Mutex<Connection>` lets concurrent reads (page loads, clicks, scroll) proceed
/// under WAL while the index runs, instead of freezing the UI. Schema already
/// exists (the primary connection migrated it), so this neither migrates nor
/// resets jobs. It waits (busy_timeout) at transaction start for any brief
/// in-flight app write to finish.
pub(crate) fn open_index_writer(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|err| format!("Failed to open index writer for {}: {err}", path.display()))?;
    configure_connection(&conn, Some(Duration::from_secs(60)))?;
    Ok(conn)
}

fn reset_interrupted_index_jobs(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE document_index_jobs
         SET status = 'failed',
             error = 'Interrupted by app shutdown',
             updated_at = unixepoch(),
             finished_at = unixepoch()
         WHERE job_type IN ('text_pdf', 'visual_tsr')
           AND status IN ('queued', 'running')",
        [],
    )
    .map_err(|err| format!("Failed to reset interrupted index jobs: {err}"))?;
    conn.execute(
        "UPDATE documents
         SET index_status = 'stale',
             updated_at = unixepoch()
         WHERE index_status = 'indexing'",
        [],
    )
    .map_err(|err| format!("Failed to reset interrupted document index states: {err}"))?;
    conn.execute(
        "UPDATE translation_jobs
         SET status = CASE
             WHEN translated_blocks > 0 THEN 'partial'
             ELSE 'failed'
           END,
           error = CASE
             WHEN translated_blocks > 0 THEN error
             ELSE 'Interrupted by app shutdown'
           END,
           updated_at = unixepoch(),
           finished_at = unixepoch()
         WHERE status IN ('queued', 'running')",
        [],
    )
    .map_err(|err| format!("Failed to reset interrupted translation jobs: {err}"))?;
    conn.execute(
        "UPDATE pdf_translation_jobs
         SET status = 'failed',
             phase = 'failed',
             error = 'Interrupted by app shutdown',
             updated_at = unixepoch(),
             finished_at = unixepoch()
         WHERE status IN ('queued', 'running')",
        [],
    )
    .map_err(|err| format!("Failed to reset interrupted PDF translation jobs: {err}"))?;
    Ok(())
}

fn migrate_database(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS workspace_roots (
          id TEXT PRIMARY KEY,
          path TEXT NOT NULL UNIQUE,
          name TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          last_opened_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS documents (
          id TEXT PRIMARY KEY,
          workspace_root_id TEXT NOT NULL,
          path TEXT NOT NULL UNIQUE,
          title TEXT NOT NULL,
          short_title TEXT NOT NULL,
          file_size INTEGER NOT NULL,
          modified INTEGER NOT NULL,
          page_count INTEGER NOT NULL DEFAULT 0,
          last_page INTEGER NOT NULL DEFAULT 1,
          zoom REAL NOT NULL DEFAULT 1.18,
          index_status TEXT NOT NULL DEFAULT 'pending',
          index_version INTEGER NOT NULL DEFAULT 0,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          last_opened_at INTEGER NOT NULL,
          FOREIGN KEY(workspace_root_id) REFERENCES workspace_roots(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS document_pages (
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          width REAL NOT NULL,
          height REAL NOT NULL,
          text TEXT NOT NULL,
          PRIMARY KEY(document_id, page_no),
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS document_blocks (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          block_index INTEGER NOT NULL,
          text TEXT NOT NULL,
          bbox_json TEXT NOT NULL,
          block_role TEXT NOT NULL DEFAULT 'body',
          region_index INTEGER NOT NULL DEFAULT 0,
          region_id TEXT NOT NULL DEFAULT '',
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS document_lines (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          line_no INTEGER NOT NULL,
          block_id TEXT NOT NULL DEFAULT '',
          block_index INTEGER NOT NULL DEFAULT 0,
          text TEXT NOT NULL,
          bbox_json TEXT NOT NULL,
          source_order_json TEXT NOT NULL DEFAULT '[]',
          region_index INTEGER NOT NULL DEFAULT 0,
          region_id TEXT NOT NULL DEFAULT '',
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_lines_document_page_line
          ON document_lines(document_id, page_no, line_no);

        CREATE TABLE IF NOT EXISTS document_outlines (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          title TEXT NOT NULL,
          level INTEGER NOT NULL,
          page_start INTEGER NOT NULL,
          page_end INTEGER NOT NULL DEFAULT 0,
          order_index INTEGER NOT NULL DEFAULT 0,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_outlines_document_order
          ON document_outlines(document_id, order_index);

        CREATE TABLE IF NOT EXISTS document_chunks (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          block_ids_json TEXT NOT NULL,
          text TEXT NOT NULL,
          bbox_refs_json TEXT NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS document_chunks_fts
          USING fts5(chunk_id UNINDEXED, document_id UNINDEXED, text);

        CREATE TABLE IF NOT EXISTS document_visual_assets (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          asset_type TEXT NOT NULL,
          caption TEXT NOT NULL DEFAULT '',
          bbox_json TEXT NOT NULL DEFAULT '[]',
          image_path TEXT NOT NULL DEFAULT '',
          ocr_text TEXT NOT NULL DEFAULT '',
          nearby_text TEXT NOT NULL DEFAULT '',
          linked_block_ids_json TEXT NOT NULL DEFAULT '[]',
          source TEXT NOT NULL DEFAULT 'pdf_bbox',
          confidence REAL NOT NULL DEFAULT 0.0,
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_visual_assets_document_page
          ON document_visual_assets(document_id, page_no, asset_type);

        CREATE TABLE IF NOT EXISTS document_tables (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          caption TEXT NOT NULL DEFAULT '',
          bbox_json TEXT NOT NULL DEFAULT '[]',
          visual_asset_id TEXT NOT NULL DEFAULT '',
          source TEXT NOT NULL DEFAULT 'pdf_bbox',
          confidence REAL NOT NULL DEFAULT 0.0,
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_tables_document_page
          ON document_tables(document_id, page_no);

        CREATE TABLE IF NOT EXISTS document_table_cells (
          id TEXT PRIMARY KEY,
          table_id TEXT NOT NULL,
          row_index INTEGER NOT NULL,
          col_index INTEGER NOT NULL,
          row_span INTEGER NOT NULL DEFAULT 1,
          col_span INTEGER NOT NULL DEFAULT 1,
          text TEXT NOT NULL,
          bbox_json TEXT NOT NULL DEFAULT '[]',
          is_header INTEGER NOT NULL DEFAULT 0,
          confidence REAL NOT NULL DEFAULT 0.0,
          FOREIGN KEY(table_id) REFERENCES document_tables(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_table_cells_table_position
          ON document_table_cells(table_id, row_index, col_index);

        CREATE TABLE IF NOT EXISTS document_table_facts (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          table_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          row_label TEXT NOT NULL DEFAULT '',
          column_label TEXT NOT NULL DEFAULT '',
          value_text TEXT NOT NULL DEFAULT '',
          fact_text TEXT NOT NULL,
          bbox_json TEXT NOT NULL DEFAULT '[]',
          source TEXT NOT NULL DEFAULT 'pdf_bbox',
          confidence REAL NOT NULL DEFAULT 0.0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE,
          FOREIGN KEY(table_id) REFERENCES document_tables(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_table_facts_document_page
          ON document_table_facts(document_id, page_no);

        CREATE VIRTUAL TABLE IF NOT EXISTS document_table_facts_fts
          USING fts5(fact_id UNINDEXED, document_id UNINDEXED, table_id UNINDEXED, text);

        CREATE TABLE IF NOT EXISTS document_index_jobs (
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
          UNIQUE(document_id, job_type),
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_index_jobs_document_type
          ON document_index_jobs(document_id, job_type);

        CREATE TABLE IF NOT EXISTS structure_tree_nodes (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          parent_id TEXT,
          title TEXT NOT NULL,
          level INTEGER NOT NULL,
          page_start INTEGER NOT NULL,
          page_end INTEGER NOT NULL,
          block_start_index INTEGER NOT NULL DEFAULT 0,
          block_end_index INTEGER NOT NULL DEFAULT 0,
          keywords_json TEXT NOT NULL DEFAULT '[]',
          visual_hint_json TEXT NOT NULL DEFAULT '{}',
          order_index INTEGER NOT NULL DEFAULT 0,
          tree_version INTEGER NOT NULL DEFAULT 1,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_structure_tree_nodes_document_order
          ON structure_tree_nodes(document_id, order_index);

        -- Knowledge precipitation (Stream 1): per-document derived artifacts.
        CREATE TABLE IF NOT EXISTS document_artifacts (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          kind TEXT NOT NULL,            -- 'summary' | 'entity' | 'concept' | 'keyword'
          name TEXT NOT NULL DEFAULT '',
          normalized TEXT NOT NULL DEFAULT '',
          detail TEXT NOT NULL DEFAULT '',
          salience REAL NOT NULL DEFAULT 0,
          metadata_json TEXT NOT NULL DEFAULT '{}',
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_artifacts_doc
          ON document_artifacts(document_id, kind);
        CREATE INDEX IF NOT EXISTS idx_document_artifacts_norm
          ON document_artifacts(normalized, kind);

        -- Precipitation progress + SHA256 cache (one row per document).
        CREATE TABLE IF NOT EXISTS document_knowledge (
          document_id TEXT PRIMARY KEY,
          status TEXT NOT NULL DEFAULT 'pending', -- pending|running|done|failed|skipped
          source_hash TEXT NOT NULL DEFAULT '',
          artifact_version INTEGER NOT NULL DEFAULT 0,
          summary TEXT NOT NULL DEFAULT '',
          error TEXT NOT NULL DEFAULT '',
          updated_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        -- Knowledge precipitation (Stream 2): claims distilled from chat turns.
        CREATE TABLE IF NOT EXISTS knowledge_claims (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          turn_id TEXT NOT NULL,
          session_id TEXT NOT NULL DEFAULT '',
          claim TEXT NOT NULL,
          question TEXT NOT NULL DEFAULT '',
          doc_ids_json TEXT NOT NULL DEFAULT '[]',
          citations_json TEXT NOT NULL DEFAULT '[]',
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_knowledge_claims_doc
          ON knowledge_claims(document_id);
        CREATE INDEX IF NOT EXISTS idx_knowledge_claims_turn
          ON knowledge_claims(turn_id);

        -- Undirected doc<->doc relationship edges (co_citation from chat, shared_concept from artifacts).
        CREATE TABLE IF NOT EXISTS document_links (
          id TEXT PRIMARY KEY,
          doc_a TEXT NOT NULL,             -- convention: doc_a < doc_b
          doc_b TEXT NOT NULL,
          basis TEXT NOT NULL,             -- 'co_citation' | 'shared_concept'
          weight REAL NOT NULL DEFAULT 0,
          evidence_json TEXT NOT NULL DEFAULT '{}',
          updated_at INTEGER NOT NULL,
          UNIQUE(doc_a, doc_b, basis),
          FOREIGN KEY(doc_a) REFERENCES documents(id) ON DELETE CASCADE,
          FOREIGN KEY(doc_b) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_document_links_a ON document_links(doc_a);
        CREATE INDEX IF NOT EXISTS idx_document_links_b ON document_links(doc_b);

        -- Cached HF trending-papers list per period, so the agent's
        -- list_trending_papers tool can read it synchronously (the live fetch is
        -- async network I/O that doesn't fit the sync tool dispatch). Refreshed
        -- by fetch_trending_papers whenever the user views the Trending feed.
        CREATE TABLE IF NOT EXISTS trending_cache (
          period TEXT PRIMARY KEY,          -- 'daily' | 'weekly' | 'monthly'
          payload_json TEXT NOT NULL,       -- serialized Vec<TrendingPaper>
          fetched_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS translations (
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          block_id TEXT NOT NULL DEFAULT '',
          source_hash TEXT NOT NULL,
          target_lang TEXT NOT NULL,
          provider TEXT NOT NULL,
          source_text TEXT NOT NULL,
          translated_text TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          PRIMARY KEY(document_id, source_hash, target_lang, provider),
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS translation_jobs (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          target_lang TEXT NOT NULL,
          provider_key TEXT NOT NULL,
          prompt_version TEXT NOT NULL DEFAULT 'v1',
          index_version INTEGER NOT NULL DEFAULT 0,
          source_version TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL,
          total_blocks INTEGER NOT NULL DEFAULT 0,
          translated_blocks INTEGER NOT NULL DEFAULT 0,
          failed_blocks INTEGER NOT NULL DEFAULT 0,
          error TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          started_at INTEGER NOT NULL DEFAULT 0,
          finished_at INTEGER NOT NULL DEFAULT 0,
          canceled_at INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_translation_jobs_active_dedup
          ON translation_jobs(document_id, target_lang, provider_key, prompt_version, index_version, source_version)
          WHERE status IN ('queued', 'running');

        CREATE TABLE IF NOT EXISTS translated_blocks (
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          block_id TEXT NOT NULL,
          source_hash TEXT NOT NULL,
          target_lang TEXT NOT NULL,
          provider_key TEXT NOT NULL,
          prompt_version TEXT NOT NULL DEFAULT 'v1',
          index_version INTEGER NOT NULL DEFAULT 0,
          source_text TEXT NOT NULL,
          translated_text TEXT NOT NULL,
          bbox_list TEXT NOT NULL DEFAULT '[]',
          order_index INTEGER NOT NULL DEFAULT 0,
          status TEXT NOT NULL DEFAULT 'succeeded',
          error TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          PRIMARY KEY(document_id, block_id, source_hash, target_lang, provider_key, prompt_version, index_version),
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_translated_blocks_page
          ON translated_blocks(document_id, target_lang, provider_key, page_no, order_index);

        CREATE TABLE IF NOT EXISTS pdf_translation_jobs (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          source_hash TEXT NOT NULL,
          source_size INTEGER NOT NULL DEFAULT 0,
          source_modified INTEGER NOT NULL DEFAULT 0,
          target_lang TEXT NOT NULL,
          source_lang TEXT NOT NULL DEFAULT 'en',
          provider_key TEXT NOT NULL,
          cache_key TEXT NOT NULL UNIQUE,
          status TEXT NOT NULL,
          phase TEXT NOT NULL DEFAULT '',
          progress_percent INTEGER NOT NULL DEFAULT 0,
          current_page INTEGER NOT NULL DEFAULT 0,
          mono_pdf_path TEXT NOT NULL DEFAULT '',
          dual_pdf_path TEXT NOT NULL DEFAULT '',
          error TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          started_at INTEGER NOT NULL DEFAULT 0,
          finished_at INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_pdf_translation_jobs_document_status
          ON pdf_translation_jobs(document_id, status, updated_at);

        CREATE TABLE IF NOT EXISTS pdf_translation_artifacts (
          cache_key TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          mono_pdf_path TEXT NOT NULL DEFAULT '',
          dual_pdf_path TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE,
          FOREIGN KEY(job_id) REFERENCES pdf_translation_jobs(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_pdf_translation_artifacts_document
          ON pdf_translation_artifacts(document_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS pdf_translation_events (
          id TEXT PRIMARY KEY,
          job_id TEXT NOT NULL,
          event_type TEXT NOT NULL,
          payload_json TEXT NOT NULL DEFAULT '{}',
          created_at INTEGER NOT NULL,
          FOREIGN KEY(job_id) REFERENCES pdf_translation_jobs(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_pdf_translation_events_job_created
          ON pdf_translation_events(job_id, created_at);

        CREATE TABLE IF NOT EXISTS model_providers (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          provider_type TEXT NOT NULL,
          base_url TEXT NOT NULL,
          model TEXT NOT NULL,
          models_json TEXT NOT NULL DEFAULT '[]',
          default_model_key TEXT NOT NULL DEFAULT '',
          enabled INTEGER NOT NULL DEFAULT 1,
          is_default INTEGER NOT NULL DEFAULT 0,
          api_key_secret_ref TEXT NOT NULL,
          has_api_key INTEGER NOT NULL DEFAULT 0,
          api_key_local TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL,
          updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS retrieval_evidence_runs (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          question TEXT NOT NULL,
          intent TEXT NOT NULL,
          tree_node_ids_json TEXT NOT NULL DEFAULT '[]',
          fts_candidate_ids_json TEXT NOT NULL DEFAULT '[]',
          wiki_candidate_ids_json TEXT NOT NULL DEFAULT '[]',
          vector_candidate_ids_json TEXT NOT NULL DEFAULT '[]',
          selected_candidate_ids_json TEXT NOT NULL DEFAULT '[]',
          finalize_gate_json TEXT NOT NULL DEFAULT '{}',
          created_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_retrieval_evidence_runs_document_created
          ON retrieval_evidence_runs(document_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS chat_turns (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          provider_id TEXT NOT NULL DEFAULT '',
          model_key TEXT NOT NULL DEFAULT '',
          provider_label TEXT NOT NULL DEFAULT '',
          user_message TEXT NOT NULL,
          assistant_answer TEXT NOT NULL,
          reasoning_content TEXT NOT NULL DEFAULT '',
          selected_text TEXT NOT NULL DEFAULT '',
          image_data_url TEXT NOT NULL DEFAULT '',
          citations_json TEXT NOT NULL DEFAULT '[]',
          claims_json TEXT NOT NULL DEFAULT '[]',
          retrieval_trace_json TEXT NOT NULL DEFAULT '{}',
          referenced_document_ids_json TEXT NOT NULL DEFAULT '[]',
          index_version INTEGER NOT NULL DEFAULT 0,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_chat_turns_document_created
          ON chat_turns(document_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS notes (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page INTEGER NOT NULL,
          bbox_list_json TEXT NOT NULL DEFAULT '[]',
          quote_text TEXT NOT NULL DEFAULT '',
          content TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_notes_document_created
          ON notes(document_id, created_at DESC);

        -- Agent workspace sessions: a session is a first-class conversation that
        -- is independent of any single document. focus_document_id is the
        -- (mutable, nullable) document the session currently centers on; it is
        -- intentionally NOT a cascading foreign key so deleting a document never
        -- destroys the conversation that referenced it.
        CREATE TABLE IF NOT EXISTS chat_sessions (
          id TEXT PRIMARY KEY,
          title TEXT NOT NULL DEFAULT '',
          focus_document_id TEXT,
          referenced_document_ids_json TEXT NOT NULL DEFAULT '[]',
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
          ON chat_sessions(updated_at DESC);
        ",
    )
    .map_err(|err| format!("Failed to migrate SQLite database: {err}"))?;

    ensure_column(
        conn,
        "documents",
        "index_version",
        "ALTER TABLE documents ADD COLUMN index_version INTEGER NOT NULL DEFAULT 0",
    )?;
    // Recognized text inside figure/chart/image crops (OCR). The crop image
    // itself (image_path) is always preserved as the primary evidence; this
    // only ADDS searchable text alongside it.
    ensure_column(
        conn,
        "document_visual_assets",
        "ocr_text",
        "ALTER TABLE document_visual_assets ADD COLUMN ocr_text TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "model_providers",
        "models_json",
        "ALTER TABLE model_providers ADD COLUMN models_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "model_providers",
        "default_model_key",
        "ALTER TABLE model_providers ADD COLUMN default_model_key TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "model_providers",
        "has_api_key",
        "ALTER TABLE model_providers ADD COLUMN has_api_key INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "model_providers",
        "api_key_local",
        "ALTER TABLE model_providers ADD COLUMN api_key_local TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "chat_turns",
        "reasoning_content",
        "ALTER TABLE chat_turns ADD COLUMN reasoning_content TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "chat_turns",
        "index_version",
        "ALTER TABLE chat_turns ADD COLUMN index_version INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "chat_turns",
        "referenced_document_ids_json",
        "ALTER TABLE chat_turns ADD COLUMN referenced_document_ids_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "chat_turns",
        "session_id",
        "ALTER TABLE chat_turns ADD COLUMN session_id TEXT NOT NULL DEFAULT ''",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chat_turns_session_created
          ON chat_turns(session_id, created_at DESC);",
    )
    .map_err(|err| format!("Failed to create chat turn session index: {err}"))?;
    migrate_chat_turns_to_sessions(conn)?;
    ensure_column(
        conn,
        "document_blocks",
        "block_role",
        "ALTER TABLE document_blocks ADD COLUMN block_role TEXT NOT NULL DEFAULT 'body'",
    )?;
    ensure_column(
        conn,
        "document_blocks",
        "region_index",
        "ALTER TABLE document_blocks ADD COLUMN region_index INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_blocks",
        "region_id",
        "ALTER TABLE document_blocks ADD COLUMN region_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "document_lines",
        "block_id",
        "ALTER TABLE document_lines ADD COLUMN block_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "document_lines",
        "block_index",
        "ALTER TABLE document_lines ADD COLUMN block_index INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_lines",
        "source_order_json",
        "ALTER TABLE document_lines ADD COLUMN source_order_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "document_lines",
        "region_index",
        "ALTER TABLE document_lines ADD COLUMN region_index INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_lines",
        "region_id",
        "ALTER TABLE document_lines ADD COLUMN region_id TEXT NOT NULL DEFAULT ''",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_document_lines_document_page_block_line
          ON document_lines(document_id, page_no, block_index, line_no);",
    )
    .map_err(|err| format!("Failed to create document line block index: {err}"))?;
    ensure_layout_debug_schema(conn)?;
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_document_visual_assets_document_page
          ON document_visual_assets(document_id, page_no, asset_type);
        CREATE INDEX IF NOT EXISTS idx_document_tables_document_page
          ON document_tables(document_id, page_no);
        CREATE INDEX IF NOT EXISTS idx_document_table_cells_table_position
          ON document_table_cells(table_id, row_index, col_index);
        CREATE INDEX IF NOT EXISTS idx_document_table_facts_document_page
          ON document_table_facts(document_id, page_no);
        CREATE INDEX IF NOT EXISTS idx_document_index_jobs_document_type
          ON document_index_jobs(document_id, job_type);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_translation_jobs_active_dedup
          ON translation_jobs(document_id, target_lang, provider_key, prompt_version, index_version, source_version)
          WHERE status IN ('queued', 'running');
        CREATE INDEX IF NOT EXISTS idx_translated_blocks_page
          ON translated_blocks(document_id, target_lang, provider_key, page_no, order_index);
        CREATE INDEX IF NOT EXISTS idx_pdf_translation_jobs_document_status
          ON pdf_translation_jobs(document_id, status, updated_at);
        CREATE INDEX IF NOT EXISTS idx_pdf_translation_artifacts_document
          ON pdf_translation_artifacts(document_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_pdf_translation_events_job_created
          ON pdf_translation_events(job_id, created_at);
        ",
    )
    .map_err(|err| format!("Failed to create visual evidence indexes: {err}"))?;

    Ok(())
}

/// One-time, idempotent migration from per-document chat history to the
/// session model. Each distinct `document_id` that still has turns with an
/// empty `session_id` gets exactly one session (deterministic id
/// `migrated-<document_id>` — the SQL form of `crate::migrated_session_id`;
/// keep the two in sync), and that document's orphaned turns are
/// back-filled to point at it. Re-running is a no-op because the second pass
/// finds no rows with `session_id = ''`.
fn migrate_chat_turns_to_sessions(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO chat_sessions
            (id, title, focus_document_id, referenced_document_ids_json, created_at, updated_at)
         SELECT
            'migrated-' || ct.document_id,
            COALESCE(
              NULLIF((SELECT d.short_title FROM documents d WHERE d.id = ct.document_id), ''),
              (SELECT d.title FROM documents d WHERE d.id = ct.document_id),
              ''
            ),
            ct.document_id,
            '[]',
            MIN(ct.created_at),
            MAX(ct.updated_at)
         FROM chat_turns ct
         WHERE ct.session_id = ''
         GROUP BY ct.document_id",
        [],
    )
    .map_err(|err| format!("Failed to seed migrated chat sessions: {err}"))?;
    conn.execute(
        "UPDATE chat_turns
            SET session_id = 'migrated-' || document_id
          WHERE session_id = ''",
        [],
    )
    .map_err(|err| format!("Failed to back-fill chat turn session ids: {err}"))?;
    Ok(())
}

fn ensure_layout_debug_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS document_text_units (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          source_order INTEGER NOT NULL,
          text TEXT NOT NULL,
          bbox_json TEXT NOT NULL,
          font_size REAL NOT NULL DEFAULT 0,
          font_name TEXT NOT NULL DEFAULT '',
          font_flags INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS document_layout_lines (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          line_no INTEGER NOT NULL,
          text TEXT NOT NULL,
          bbox_json TEXT NOT NULL,
          source_order_json TEXT NOT NULL DEFAULT '[]',
          region_index INTEGER NOT NULL DEFAULT 0,
          region_id TEXT NOT NULL DEFAULT '',
          role_hint TEXT NOT NULL DEFAULT '',
          baseline REAL NOT NULL DEFAULT 0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS document_layout_regions (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL,
          page_no INTEGER NOT NULL,
          region_index INTEGER NOT NULL,
          kind TEXT NOT NULL,
          bbox_json TEXT NOT NULL,
          line_numbers_json TEXT NOT NULL DEFAULT '[]',
          confidence REAL NOT NULL DEFAULT 0,
          FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
        );
        ",
    )
    .map_err(|err| format!("Failed to create layout debug tables: {err}"))?;
    ensure_layout_debug_columns(conn)?;
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_document_text_units_document_page_source
          ON document_text_units(document_id, page_no, source_order);
        CREATE INDEX IF NOT EXISTS idx_document_layout_lines_document_page_line
          ON document_layout_lines(document_id, page_no, line_no);
        CREATE INDEX IF NOT EXISTS idx_document_layout_regions_document_page
          ON document_layout_regions(document_id, page_no, region_index);
        ",
    )
    .map_err(|err| format!("Failed to create layout debug indexes: {err}"))?;
    Ok(())
}

fn ensure_layout_debug_columns(conn: &Connection) -> Result<(), String> {
    ensure_column(
        conn,
        "document_text_units",
        "font_size",
        "ALTER TABLE document_text_units ADD COLUMN font_size REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_text_units",
        "font_name",
        "ALTER TABLE document_text_units ADD COLUMN font_name TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "document_text_units",
        "font_flags",
        "ALTER TABLE document_text_units ADD COLUMN font_flags INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_layout_lines",
        "source_order_json",
        "ALTER TABLE document_layout_lines ADD COLUMN source_order_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "document_layout_lines",
        "region_index",
        "ALTER TABLE document_layout_lines ADD COLUMN region_index INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_layout_lines",
        "region_id",
        "ALTER TABLE document_layout_lines ADD COLUMN region_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "document_layout_lines",
        "role_hint",
        "ALTER TABLE document_layout_lines ADD COLUMN role_hint TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "document_layout_lines",
        "baseline",
        "ALTER TABLE document_layout_lines ADD COLUMN baseline REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_layout_regions",
        "region_index",
        "ALTER TABLE document_layout_regions ADD COLUMN region_index INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "document_layout_regions",
        "line_numbers_json",
        "ALTER TABLE document_layout_regions ADD COLUMN line_numbers_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "document_layout_regions",
        "confidence",
        "ALTER TABLE document_layout_regions ADD COLUMN confidence REAL NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    alter_sql: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .map_err(|err| format!("Failed to inspect {table_name} schema: {err}"))?;
    let has_column = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("Failed to inspect {table_name} columns: {err}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("Failed to read {table_name} columns: {err}"))?
        .iter()
        .any(|name| name == column_name);

    if !has_column {
        conn.execute_batch(alter_sql)
            .map_err(|err| format!("Failed to add {column_name} column: {err}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::CURRENT_INDEX_VERSION;

    use super::*;

    #[test]
    fn reset_interrupted_index_jobs_unsticks_startup_state() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        migrate_database(&conn).expect("schema");
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
             VALUES ('doc', 'root', '/tmp/root/a.pdf', 'A', 'A', 1, 1, 0, 1, 'indexing', 0, 1, 1, 1)",
            [],
        )
        .expect("document");
        conn.execute(
            "INSERT INTO document_index_jobs
               (id, document_id, job_type, status, version, attempts, error,
                created_at, updated_at, started_at, finished_at)
             VALUES ('job-text', 'doc', 'text_pdf', 'running', 13, 1, '', 1, 1, 1, 0),
                    ('job-visual', 'doc', 'visual_tsr', 'queued', 13, 0, '', 1, 1, 0, 0)",
            [],
        )
        .expect("jobs");
        conn.execute(
            "INSERT INTO translation_jobs
               (id, document_id, target_lang, provider_key, prompt_version, index_version,
                source_version, status, total_blocks, translated_blocks, failed_blocks, error,
                created_at, updated_at, started_at, finished_at, canceled_at)
             VALUES ('translation-running', 'doc', 'zh', 'provider', 'v1', 13,
                     'index:13:document', 'running', 4, 2, 0, '', 1, 1, 1, 0, 0),
                    ('translation-queued', 'doc', 'ja', 'provider', 'v1', 13,
                     'index:13:document', 'queued', 4, 0, 0, '', 1, 1, 0, 0, 0)",
            [],
        )
        .expect("translation jobs");
        reset_interrupted_index_jobs(&conn).expect("reset jobs");

        let document_status: String = conn
            .query_row(
                "SELECT index_status FROM documents WHERE id = 'doc'",
                [],
                |row| row.get(0),
            )
            .expect("document status");
        assert_eq!(document_status, "stale");

        let failed_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM document_index_jobs
                 WHERE document_id = 'doc'
                   AND status = 'failed'
                   AND error = 'Interrupted by app shutdown'",
                [],
                |row| row.get(0),
            )
            .expect("failed job count");
        assert_eq!(failed_jobs, 2);

        let partial_translation_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM translation_jobs
                 WHERE id = 'translation-running'
                   AND status = 'partial'",
                [],
                |row| row.get(0),
            )
            .expect("partial translation job count");
        assert_eq!(partial_translation_jobs, 1);

        let failed_translation_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM translation_jobs
                 WHERE id = 'translation-queued'
                   AND status = 'failed'
                   AND error = 'Interrupted by app shutdown'",
                [],
                |row| row.get(0),
            )
            .expect("failed translation job count");
        assert_eq!(failed_translation_jobs, 1);
    }

    #[test]
    fn migrate_database_creates_layout_debug_tables() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        migrate_database(&conn).expect("schema");
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
             VALUES ('doc', 'root', '/tmp/root/a.pdf', 'A', 'A', 1, 1, 0, 1, 'indexed', ?1, 1, 1, 1)",
            [CURRENT_INDEX_VERSION],
        )
        .expect("document");

        conn.execute(
            "INSERT INTO document_text_units
               (id, document_id, page_no, source_order, text, bbox_json)
             VALUES ('unit-1', 'doc', 1, 7, 'sample', '[0.1,0.2,0.3,0.4]')",
            [],
        )
        .expect("text unit insert");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_text_units", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 1);

        conn.execute(
            "INSERT INTO document_lines
               (id, document_id, page_no, line_no, text, bbox_json, region_index, region_id)
             VALUES ('line-1', 'doc', 1, 1, 'sample line', '[[0.1,0.2,0.3,0.4]]', 1, 'doc-p1-r1')",
            [],
        )
        .expect("layout line insert");

        let line_region_id: String = conn
            .query_row(
                "SELECT region_id FROM document_lines WHERE id = 'line-1'",
                [],
                |row| row.get(0),
            )
            .expect("line region id");
        assert_eq!(line_region_id, "doc-p1-r1");

        conn.execute(
            "INSERT INTO document_layout_lines
               (id, document_id, page_no, line_no, text, bbox_json, source_order_json, region_index, region_id)
             VALUES ('layout-line-1', 'doc', 1, 1, 'sample line', '[[0.1,0.2,0.3,0.4]]', '[7]', 1, 'doc-p1-r1')",
            [],
        )
        .expect("raw layout line insert");

        let layout_line_region_id: String = conn
            .query_row(
                "SELECT region_id FROM document_layout_lines WHERE id = 'layout-line-1'",
                [],
                |row| row.get(0),
            )
            .expect("layout line region id");
        assert_eq!(layout_line_region_id, "doc-p1-r1");

        conn.execute(
            "INSERT INTO document_layout_regions
               (id, document_id, page_no, region_index, kind, bbox_json, line_numbers_json, confidence)
             VALUES ('region-1', 'doc', 1, 1, 'body_column', '[0.1,0.2,0.3,0.4]', '[1,2]', 0.95)",
            [],
        )
        .expect("layout region insert");

        let region_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_layout_regions", [], |row| {
                row.get(0)
            })
            .expect("region count");
        assert_eq!(region_count, 1);
    }

    #[test]
    fn migrate_database_upgrades_existing_layout_debug_tables() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "
            CREATE TABLE document_text_units (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL,
              page_no INTEGER NOT NULL,
              source_order INTEGER NOT NULL,
              text TEXT NOT NULL,
              bbox_json TEXT NOT NULL
            );
            CREATE TABLE document_layout_lines (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL,
              page_no INTEGER NOT NULL,
              line_no INTEGER NOT NULL,
              text TEXT NOT NULL,
              bbox_json TEXT NOT NULL
            );
            CREATE TABLE document_layout_regions (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL,
              page_no INTEGER NOT NULL,
              kind TEXT NOT NULL,
              bbox_json TEXT NOT NULL
            );
            ",
        )
        .expect("old layout schema");

        migrate_database(&conn).expect("migrate old layout schema");

        for (table, column) in [
            ("document_text_units", "font_size"),
            ("document_text_units", "font_name"),
            ("document_text_units", "font_flags"),
            ("document_layout_lines", "source_order_json"),
            ("document_layout_lines", "region_index"),
            ("document_layout_lines", "region_id"),
            ("document_layout_lines", "role_hint"),
            ("document_layout_lines", "baseline"),
            ("document_layout_regions", "region_index"),
            ("document_layout_regions", "line_numbers_json"),
            ("document_layout_regions", "confidence"),
        ] {
            assert!(
                table_has_column(&conn, table, column),
                "{table} should have migrated column {column}"
            );
        }
    }

    fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("pragma");
        stmt.query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("column names")
            .iter()
            .any(|name| name == column)
    }

    #[test]
    fn migrate_back_fills_per_document_chat_sessions_idempotently() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        // Simulate an upgrade from a pre-session database: a legacy chat_turns
        // table (no session_id) plus a minimal documents table so the migration
        // can resolve titles. `CREATE TABLE IF NOT EXISTS` in migrate_database
        // leaves these pre-existing tables intact.
        conn.execute_batch(
            "
            CREATE TABLE documents (
              id TEXT PRIMARY KEY,
              short_title TEXT NOT NULL DEFAULT '',
              title TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE chat_turns (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL,
              user_message TEXT NOT NULL,
              assistant_answer TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );
            INSERT INTO documents (id, short_title, title) VALUES
              ('doc-a', 'Alpha', 'Alpha Full Title');
            INSERT INTO chat_turns (id, document_id, user_message, assistant_answer, created_at, updated_at) VALUES
              ('t1', 'doc-a', 'q1', 'a1', 10, 11),
              ('t2', 'doc-a', 'q2', 'a2', 20, 25),
              ('t3', 'doc-b', 'q3', 'a3', 30, 31);
            ",
        )
        .expect("legacy schema");

        migrate_database(&conn).expect("migrate legacy chat history");

        // Every legacy turn now points at its per-document session.
        let unmigrated: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_turns WHERE session_id = ''",
                [],
                |row| row.get(0),
            )
            .expect("count unmigrated");
        assert_eq!(unmigrated, 0, "all turns should be back-filled");

        let (title_a, focus_a, created_a, updated_a): (String, String, i64, i64) = conn
            .query_row(
                "SELECT title, focus_document_id, created_at, updated_at
                 FROM chat_sessions WHERE id = 'migrated-doc-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("doc-a session");
        assert_eq!(title_a, "Alpha", "title prefers short_title");
        assert_eq!(focus_a, "doc-a");
        assert_eq!(created_a, 10, "created_at is the earliest turn");
        assert_eq!(updated_a, 25, "updated_at is the latest turn");

        // doc-b has no documents row → title falls back to empty, session still created.
        let title_b: String = conn
            .query_row(
                "SELECT title FROM chat_sessions WHERE id = 'migrated-doc-b'",
                [],
                |row| row.get(0),
            )
            .expect("doc-b session");
        assert_eq!(title_b, "");

        let session_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| row.get(0))
            .expect("session count");
        assert_eq!(session_count, 2, "one session per distinct document");

        // Re-running the migration is a no-op: no new sessions, no churn.
        migrate_chat_turns_to_sessions(&conn).expect("re-run migration");
        let session_count_again: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| row.get(0))
            .expect("session count again");
        assert_eq!(session_count_again, 2, "migration is idempotent");
    }
}
