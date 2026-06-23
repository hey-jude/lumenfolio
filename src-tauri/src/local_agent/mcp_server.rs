//! P2 (Mode B): an in-process, loopback HTTP MCP server that re-exposes
//! Lumenfolio's RAG tools so the user's local agent (Codex/Claude) can call them
//! for multi-step, grounded retrieval. The server is the same process that owns
//! the data, so it observes every tool call + result and collects the citations
//! it served (consumed by the agentic dispatch in a later step).
//!
//! Transport: streamable HTTP on 127.0.0.1 with a random per-run bearer token
//! (both Codex `mcp add --url … --bearer-token-env-var` and Claude `--mcp-config`
//! support it). Tools are read-only, so the server uses its own READ-ONLY SQLite
//! connection to the same DB file rather than contending the main `AppDatabase`.
//!
//! Wired into the chat dispatch in P2-4 (`generate_answer_agentic`): the focused-
//! document reader path drives the local CLI against this server.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use http_body_util::{combinators::BoxBody, BodyExt, Empty};
use hyper::body::Bytes;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::{
        streamable_http_server::session::local::LocalSessionManager, StreamableHttpServerConfig,
        StreamableHttpService,
    },
    RoleServer, ServerHandler,
};

use crate::runtime::rag::{self, Citation, RagToolCapabilities, RetrievalTraceCandidate};

/// Local agents have long context windows, so let each tool call return fuller
/// passages than the token-budgeted API path (which clamps to ≤8k). Bigger quotes
/// mean the agent reads more per call and loops the tools less.
const MCP_MAX_QUOTE_CHARS: usize = 16_000;

/// The document-scoped MCP server handler. One per server (= per turn).
#[derive(Clone)]
pub(crate) struct LumenfolioMcpServer {
    db_path: PathBuf,
    document_id: String,
    /// Whether the chat's "联网" toggle is on — exposes web_search/web_fetch.
    web_enabled: bool,
    /// Citations served this run, for the agentic dispatch to surface as evidence.
    citations: Arc<Mutex<Vec<Citation>>>,
    /// Trace candidates served this run — they carry `section_title` keyed by
    /// block, so the dispatch can label the evidence chips it rebuilds.
    candidates: Arc<Mutex<Vec<RetrievalTraceCandidate>>>,
}

impl LumenfolioMcpServer {
    fn read_only_conn(&self) -> Result<rusqlite::Connection, McpError> {
        rusqlite::Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|err| McpError::internal_error(format!("open db failed: {err}"), None))
    }
}

impl ServerHandler for LumenfolioMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Lumenfolio document tools: retrieve evidence from the user's open PDF \
             (search passages, open pages/sections/tables, inspect structure). Use them \
             to gather grounded evidence, then answer.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = rag::rag_tool_specs_for_capabilities(false, self.web_enabled)
            .into_iter()
            .map(|spec| {
                let schema = spec
                    .input_schema
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                Tool::new(spec.name, spec.description, Arc::new(schema))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool = request.name.to_string();
        let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let fallback_query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Synchronous DB work, no await held across the connection (keeps the
        // future Send despite rusqlite::Connection being !Sync).
        let (output, image_paths) = {
            let conn = self.read_only_conn()?;
            let mut output = rag::execute_rag_tool_call_for_capabilities(
                &conn,
                &self.document_id,
                &[],
                &tool,
                &args,
                &fallback_query,
                RagToolCapabilities {
                    vision_enabled: false,
                    web_enabled: self.web_enabled,
                    max_quote_chars: MCP_MAX_QUOTE_CHARS,
                },
            );
            // FTS / page chunks come back without a section label; fill it from the
            // page's enclosing structure node so the dispatch's evidence chips still
            // show a section (Introduction, Methodology, …) rather than just a page.
            for citation in &mut output.citations {
                if citation.section_title.is_none() && citation.page > 0 {
                    citation.section_title =
                        rag::section_title_for_page(&conn, &citation.document_id, citation.page);
                }
            }
            // For visual tools, resolve the actual figure crops so a vision-capable
            // agent can SEE them. A visual citation's block_id is its asset id.
            let image_paths: Vec<String> = if VISUAL_TOOLS.contains(&tool.as_str()) {
                let mut seen = std::collections::HashSet::new();
                output
                    .citations
                    .iter()
                    .filter(|c| !c.block_id.is_empty() && seen.insert(c.block_id.clone()))
                    .filter_map(|c| {
                        rag::visual_asset_image_path(&conn, &c.document_id, &c.block_id)
                    })
                    .take(MAX_TOOL_IMAGES)
                    .collect()
            } else {
                Vec::new()
            };
            (output, image_paths)
        };

        let rendered = render_citations(&output.citations);
        if let Ok(mut sink) = self.citations.lock() {
            sink.extend(output.citations);
        }
        if let Ok(mut sink) = self.candidates.lock() {
            sink.extend(output.trace_candidates);
        }

        let mut contents = vec![Content::text(rendered)];
        for path in image_paths {
            if let Some(image) = read_image_content(&path) {
                contents.push(image);
            }
        }
        Ok(CallToolResult::success(contents))
    }
}

/// Tools that surface figures/charts — their citations carry a visual asset id in
/// `block_id`, so we attach the actual crop image to the result.
const VISUAL_TOOLS: [&str; 4] = [
    "open_visual",
    "inspect_visuals",
    "inspect_objects",
    "resolve_visual_anchor",
];
/// Cap images per tool call so a single call can't return a huge payload.
const MAX_TOOL_IMAGES: usize = 3;
/// Skip crops larger than this (defensive against a pathological asset).
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Read a crop file into an MCP image content block (base64 + mime). Returns None
/// if the file is missing (crops live in a temp dir), empty, or too large.
fn read_image_content(path: &str) -> Option<Content> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mime = match std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/png",
    };
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(Content::image(data, mime.to_string()))
}

fn render_citations(citations: &[Citation]) -> String {
    if citations.is_empty() {
        return "No matching evidence found.".to_string();
    }
    citations
        .iter()
        .map(|c| {
            let page = if c.page > 0 {
                format!(" (p{})", c.page)
            } else {
                String::new()
            };
            let section = c
                .section_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" — {s}"))
                .unwrap_or_default();
            format!("{}{page}{section}: {}", c.label, c.quote.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A running loopback MCP server for one turn. Drop (or `shutdown`) to stop it.
pub(crate) struct RunningMcpServer {
    pub url: String,
    pub token: String,
    pub citations: Arc<Mutex<Vec<Citation>>>,
    pub candidates: Arc<Mutex<Vec<RetrievalTraceCandidate>>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Drop for RunningMcpServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

fn random_token() -> String {
    // 128-bit loopback bearer; rand is already in the tree via rmcp.
    format!("{:016x}{:016x}", rand::random::<u64>(), rand::random::<u64>())
}

fn unauthorized() -> hyper::Response<BoxBody<Bytes, std::convert::Infallible>> {
    hyper::Response::builder()
        .status(hyper::StatusCode::UNAUTHORIZED)
        .body(Empty::<Bytes>::new().boxed())
        .expect("static response")
}

/// Start a loopback MCP server for the given document. Returns its URL + bearer
/// token + the live citation collector. The accept loop runs on the Tauri async
/// runtime until the returned handle is dropped.
pub(crate) async fn start_mcp_server(
    db_path: PathBuf,
    document_id: String,
    web_enabled: bool,
) -> Result<RunningMcpServer, String> {
    let token = random_token();
    let citations: Arc<Mutex<Vec<Citation>>> = Arc::new(Mutex::new(Vec::new()));
    let candidates: Arc<Mutex<Vec<RetrievalTraceCandidate>>> = Arc::new(Mutex::new(Vec::new()));

    let factory_path = db_path.clone();
    let factory_doc = document_id.clone();
    let factory_citations = citations.clone();
    let factory_candidates = candidates.clone();
    let http_service = StreamableHttpService::new(
        move || {
            Ok(LumenfolioMcpServer {
                db_path: factory_path.clone(),
                document_id: factory_doc.clone(),
                web_enabled,
                citations: factory_citations.clone(),
                candidates: factory_candidates.clone(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|err| format!("Failed to bind MCP server: {err}"))?;
    let port = listener
        .local_addr()
        .map_err(|err| format!("Failed to read MCP server port: {err}"))?
        .port();
    let url = format!("http://127.0.0.1:{port}/mcp");

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let token_for_loop = token.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let accepted = tokio::select! {
                _ = shutdown_rx.changed() => break,
                res = listener.accept() => res,
            };
            let Ok((stream, _)) = accepted else { break };
            let svc = http_service.clone();
            let expected = format!("Bearer {token_for_loop}");
            let io = hyper_util::rt::TokioIo::new(stream);
            tauri::async_runtime::spawn(async move {
                let hyper_svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let svc = svc.clone();
                    let expected = expected.clone();
                    async move {
                        let ok = req
                            .headers()
                            .get(hyper::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v == expected)
                            .unwrap_or(false);
                        let resp = if ok {
                            svc.handle(req).await
                        } else {
                            unauthorized()
                        };
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, hyper_svc)
                .await;
            });
        }
    });

    Ok(RunningMcpServer {
        url,
        token,
        citations,
        candidates,
        shutdown: shutdown_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_image_content_handles_valid_missing_and_empty() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("lumenfolio-test-{:016x}.png", rand::random::<u64>()));
        std::fs::write(&path, [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a]).unwrap();
        assert!(
            read_image_content(path.to_str().unwrap()).is_some(),
            "a readable non-empty file yields an image content block"
        );
        std::fs::remove_file(&path).ok();

        // Missing file → None.
        assert!(read_image_content(&path.to_string_lossy()).is_none());

        // Empty file → None.
        let empty = dir.join(format!("lumenfolio-empty-{:016x}.png", rand::random::<u64>()));
        std::fs::write(&empty, []).unwrap();
        assert!(read_image_content(empty.to_str().unwrap()).is_none());
        std::fs::remove_file(&empty).ok();
    }

    #[test]
    fn visual_tools_set_covers_figure_surfacing_tools() {
        assert!(VISUAL_TOOLS.contains(&"open_visual"));
        assert!(VISUAL_TOOLS.contains(&"resolve_visual_anchor"));
        assert!(!VISUAL_TOOLS.contains(&"search_chunks"));
    }
}
