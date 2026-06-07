//! Trending Papers: an optional online discovery feature. Fetches the Hugging
//! Face "trending" papers list and lets the user download one into a managed
//! "Trending Papers" workspace folder (which then behaves like any other folder:
//! indexed, reopenable, chat/notes/translation). local-first: nothing is fetched
//! unless the user opens the discovery view, and a PDF is downloaded only on an
//! explicit "add" action.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::{documents, AppDatabase, PdfRegistry, WorkspaceRoot, WorkspaceRootSnapshot};

const DEFAULT_TRENDING_LIMIT: u32 = 30;

// ---- Hugging Face daily_papers API shapes ---------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HfDailyPaperItem {
    paper: HfPaper,
    #[serde(default)]
    upvotes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HfPaper {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    authors: Vec<HfAuthor>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    media_urls: Vec<String>,
}

#[derive(Deserialize)]
struct HfAuthor {
    #[serde(default)]
    name: String,
}

// ---- Output to the frontend ------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrendingPaper {
    arxiv_id: String,
    title: String,
    authors: Vec<String>,
    summary: String,
    upvotes: i64,
    published_at: String,
    thumbnail_url: Option<String>,
    hf_url: String,
    pdf_url: String,
}

fn map_item(item: HfDailyPaperItem) -> TrendingPaper {
    let id = item.paper.id.trim().to_string();
    let authors = item
        .paper
        .authors
        .into_iter()
        .map(|author| author.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    TrendingPaper {
        title: item.paper.title.trim().to_string(),
        authors,
        summary: item.paper.summary.trim().to_string(),
        upvotes: item.upvotes,
        published_at: item.paper.published_at.unwrap_or_default(),
        thumbnail_url: item
            .paper
            .media_urls
            .into_iter()
            .find(|url| !url.trim().is_empty()),
        hf_url: format!("https://huggingface.co/papers/{id}"),
        pdf_url: format!("https://arxiv.org/pdf/{id}.pdf"),
        arxiv_id: id,
    }
}

/// Fetch the HF trending papers (recency + upvotes). Online-only; returns an
/// error when offline so the frontend can show an offline state.
#[tauri::command]
pub(crate) async fn fetch_trending_papers(limit: Option<u32>) -> Result<Vec<TrendingPaper>, String> {
    let limit = limit.unwrap_or(DEFAULT_TRENDING_LIMIT).clamp(1, 100);
    let url = format!("https://huggingface.co/api/daily_papers?sort=trending&limit={limit}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("Failed to create trending client: {err}"))?;
    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "Lumenfolio")
        .send()
        .await
        .map_err(|err| format!("Trending request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Trending request returned {}",
            response.status()
        ));
    }
    let items = response
        .json::<Vec<HfDailyPaperItem>>()
        .await
        .map_err(|err| format!("Failed to decode trending response: {err}"))?;
    Ok(items
        .into_iter()
        .filter(|item| !item.paper.id.trim().is_empty())
        .map(map_item)
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddTrendingResult {
    /// The (re)scanned "Trending Papers" workspace root, so the frontend can
    /// upsert it into the sidebar.
    snapshot: WorkspaceRootSnapshot,
    /// The document id of the just-added paper, so the frontend can open it.
    document_id: String,
}

/// Download a trending paper's PDF into the managed "Trending Papers" folder (if
/// not already there), (re)scan + index that folder, and return the folder
/// snapshot plus the new document id.
#[tauri::command]
pub(crate) async fn add_trending_paper(
    arxiv_id: String,
    title: Option<String>,
    app: tauri::AppHandle,
    registry: State<'_, PdfRegistry>,
    database: State<'_, AppDatabase>,
) -> Result<AddTrendingResult, String> {
    let arxiv_id = arxiv_id.trim().to_string();
    if arxiv_id.is_empty() {
        return Err("No paper id was provided".to_string());
    }

    let dir = trending_dir(&app)?;

    // Dedup by arXiv id regardless of the title in the filename.
    let file_path = existing_paper_path(&dir, &arxiv_id)
        .unwrap_or_else(|| dir.join(trending_file_name(&arxiv_id, title.as_deref())));

    if !file_path.exists() {
        download_pdf(
            &format!("https://arxiv.org/pdf/{arxiv_id}.pdf"),
            &file_path,
        )
        .await?;
    }

    // (Re)scan the whole folder — reuses the directory-import pipeline, so the new
    // file is indexed and every previously-added paper is preserved.
    let workspace_root_id = documents::stable_path_id("root", &dir);
    let mut docs = Vec::new();
    documents::collect_pdfs(&dir, &workspace_root_id, &mut docs)?;
    docs.sort_by(|left, right| {
        left.short_title
            .to_lowercase()
            .cmp(&right.short_title.to_lowercase())
    });
    documents::persist_workspace_scan(&database, &workspace_root_id, &dir, &docs)?;
    let snapshot_docs = documents::load_documents_for_root(&database, &workspace_root_id)?;
    documents::upsert_registry_paths(&registry, &snapshot_docs)?;

    let canonical = file_path.canonicalize().unwrap_or(file_path);
    let document_id = documents::stable_path_id("pdf", &canonical);

    Ok(AddTrendingResult {
        snapshot: WorkspaceRootSnapshot {
            root: WorkspaceRoot {
                id: workspace_root_id,
                path: dir.to_string_lossy().to_string(),
            },
            documents: snapshot_docs,
        },
        document_id,
    })
}

/// The managed folder: `<Documents>/Lumenfolio/Trending Papers/` (created on
/// demand). User-visible so PDFs are accessible in the file manager.
fn trending_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .document_dir()
        .map_err(|err| format!("Failed to resolve the Documents folder: {err}"))?;
    let dir = base.join("Lumenfolio").join("Trending Papers");
    fs::create_dir_all(&dir)
        .map_err(|err| format!("Failed to create the Trending Papers folder: {err}"))?;
    Ok(dir)
}

/// An existing file for this arXiv id (named `<id>.pdf` or `<id> <title>.pdf`).
fn existing_paper_path(dir: &Path, arxiv_id: &str) -> Option<PathBuf> {
    let exact = format!("{arxiv_id}.pdf");
    let prefix = format!("{arxiv_id} ");
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == exact || name.starts_with(&prefix))
        })
}

fn trending_file_name(arxiv_id: &str, title: Option<&str>) -> String {
    match title.map(sanitize_title).filter(|slug| !slug.is_empty()) {
        Some(slug) => format!("{arxiv_id} {slug}.pdf"),
        None => format!("{arxiv_id}.pdf"),
    }
}

/// Filesystem-safe, readable slug for the title (alnum/space/-/_; collapsed).
fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' {
                ch
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

async fn download_pdf(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|err| format!("Failed to create download client: {err}"))?;
    let response = client
        .get(url)
        .header("User-Agent", "Lumenfolio")
        .send()
        .await
        .map_err(|err| format!("PDF download failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("PDF download returned {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("Failed to read the downloaded PDF: {err}"))?;
    if !bytes.starts_with(b"%PDF") {
        return Err("The downloaded file is not a valid PDF (the paper may not be on arXiv yet).".to_string());
    }
    fs::write(dest, &bytes).map_err(|err| {
        format!("Failed to save the PDF to {}: {err}", dest.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_hf_item_to_trending_paper() {
        let json = r#"{
            "paper": {
                "id": "2606.05515",
                "title": "  BRepCLIP  ",
                "summary": "An abstract.",
                "authors": [{"name": "Alice"}, {"name": " "}, {"name": "Bob"}],
                "publishedAt": "2026-06-03T00:00:00.000Z",
                "mediaUrls": ["https://img/x.png"]
            },
            "upvotes": 42
        }"#;
        let item: HfDailyPaperItem = serde_json::from_str(json).unwrap();
        let paper = map_item(item);
        assert_eq!(paper.arxiv_id, "2606.05515");
        assert_eq!(paper.title, "BRepCLIP");
        assert_eq!(paper.authors, vec!["Alice".to_string(), "Bob".to_string()]);
        assert_eq!(paper.upvotes, 42);
        assert_eq!(paper.hf_url, "https://huggingface.co/papers/2606.05515");
        assert_eq!(paper.pdf_url, "https://arxiv.org/pdf/2606.05515.pdf");
        assert_eq!(paper.thumbnail_url.as_deref(), Some("https://img/x.png"));
    }

    #[test]
    fn file_name_dedup_and_sanitize() {
        assert_eq!(
            trending_file_name("2606.05515", Some("Deep: Research/Models?")),
            "2606.05515 Deep Research Models.pdf"
        );
        assert_eq!(trending_file_name("2606.05515", None), "2606.05515.pdf");
        assert_eq!(trending_file_name("2606.05515", Some("   ")), "2606.05515.pdf");
    }
}
