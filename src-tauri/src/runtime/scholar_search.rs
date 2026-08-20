//! Scholar search for the chat agent.
//!
//! Searches Semantic Scholar and CrossRef for academic papers. Results are
//! returned as [`WebHit`](super::web_search::WebHit) (same as `web_search`) so
//! they integrate seamlessly with the existing RAG citation pipeline.

use std::time::Duration;

use super::web_search::WebHit;

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Lumenfolio/1.0";

/// Search academic paper databases (Semantic Scholar + CrossRef).
///
/// Returns results in the same [`WebHit`] format as `web_search` so citations
/// flow through the existing RAG pipeline without changes. Extra metadata
/// (year, authors, citation count) is included in the snippet.
pub fn scholar_search(query: &str, limit: usize) -> Result<Vec<WebHit>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("scholar_search requires a non-empty query".to_string());
    }

    let mut hits = Vec::new();

    // Semantic Scholar first (richer data, no API key needed)
    match semantic_scholar_search(query, limit) {
        Ok(s2_hits) => hits.extend(s2_hits),
        Err(err) => log::warn!("scholar_search: Semantic Scholar failed: {err}"),
    }

    // CrossRef as supplement / fallback
    let remaining = limit.saturating_sub(hits.len());
    if remaining > 0 {
        if let Ok(cr_hits) = crossref_search(query, remaining) {
            let seen_urls: std::collections::HashSet<String> =
                hits.iter().map(|h| h.url.clone()).collect();
            for hit in cr_hits {
                if !seen_urls.contains(&hit.url) {
                    hits.push(hit);
                }
            }
        }
    }

    hits.truncate(limit);
    Ok(hits)
}

// ---------------------------------------------------------------------------
// Semantic Scholar
// ---------------------------------------------------------------------------

fn semantic_scholar_search(query: &str, limit: usize) -> Result<Vec<WebHit>, String> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit={}&fields=title,abstract,year,authors,citationCount,url,externalIds",
        urlencoding::encode(query),
        limit.min(10),
    );
    let body = http_get_json(&url)?;
    let papers = body
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let hits = papers
        .into_iter()
        .take(limit)
        .filter_map(|paper| {
            let title = paper.get("title")?.as_str()?.trim().to_string();
            if title.is_empty() {
                return None;
            }

            let paper_url = paper
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    let doi = paper
                        .get("externalIds")?
                        .get("DOI")?
                        .as_str()?;
                    Some(format!("https://doi.org/{doi}"))
                })
                .unwrap_or_default();

            let mut snippet_parts = Vec::new();
            if let Some(year) = paper.get("year").and_then(|v| v.as_i64()) {
                snippet_parts.push(format!("Year: {year}"));
            }
            if let Some(authors) = paper.get("authors").and_then(|v| v.as_array()) {
                let names: Vec<&str> = authors
                    .iter()
                    .filter_map(|a| a.get("name")?.as_str())
                    .take(5)
                    .collect();
                if !names.is_empty() {
                    snippet_parts.push(format!("Authors: {}", names.join(", ")));
                }
            }
            if let Some(citations) = paper.get("citationCount").and_then(|v| v.as_i64()) {
                snippet_parts.push(format!("Citations: {citations}"));
            }
            if let Some(abstract_text) = paper.get("abstract").and_then(|v| v.as_str()) {
                let truncated = truncate_chars(abstract_text, 400);
                snippet_parts.push(truncated);
            }
            let snippet = snippet_parts.join("\n");

            Some(WebHit {
                title,
                url: paper_url,
                snippet,
            })
        })
        .collect();

    Ok(hits)
}

// ---------------------------------------------------------------------------
// CrossRef
// ---------------------------------------------------------------------------

fn crossref_search(query: &str, limit: usize) -> Result<Vec<WebHit>, String> {
    let url = format!(
        "https://api.crossref.org/works?query={}&rows={}&select=DOI,title,author,published-print,abstract,is-referenced-by-count",
        urlencoding::encode(query),
        limit.min(10),
    );
    let body = http_get_json(&url)?;
    let items = body
        .get("message")
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let hits = items
        .into_iter()
        .take(limit)
        .filter_map(|item| {
            let title = item
                .get("title")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                return None;
            }

            let doi = item.get("DOI").and_then(|v| v.as_str()).unwrap_or("");
            let paper_url = if doi.is_empty() {
                String::new()
            } else {
                format!("https://doi.org/{doi}")
            };

            let mut snippet_parts = Vec::new();

            if let Some(pub_date) = item.get("published-print").and_then(|v| v.get("date-parts")) {
                if let Some(parts) = pub_date.as_array().and_then(|arr| arr.first()) {
                    if let Some(year) = parts.as_array().and_then(|arr| arr.first()) {
                        if let Some(y) = year.as_i64() {
                            snippet_parts.push(format!("Year: {y}"));
                        }
                    }
                }
            }

            if let Some(authors) = item.get("author").and_then(|v| v.as_array()) {
                let names: Vec<String> = authors
                    .iter()
                    .filter_map(|a| {
                        let given = a.get("given").and_then(|v| v.as_str()).unwrap_or("");
                        let family = a.get("family").and_then(|v| v.as_str()).unwrap_or("");
                        if given.is_empty() && family.is_empty() {
                            None
                        } else {
                            Some(format!("{given} {family}"))
                        }
                    })
                    .take(5)
                    .collect();
                if !names.is_empty() {
                    snippet_parts.push(format!("Authors: {}", names.join(", ")));
                }
            }

            if let Some(count) = item.get("is-referenced-by-count").and_then(|v| v.as_i64()) {
                snippet_parts.push(format!("Citations: {count}"));
            }

            if let Some(abstract_text) = item.get("abstract").and_then(|v| v.as_str()) {
                let clean = strip_html_tags(abstract_text);
                let truncated = truncate_chars(&clean, 400);
                snippet_parts.push(truncated);
            }

            let snippet = snippet_parts.join("\n");

            Some(WebHit {
                title,
                url: paper_url,
                snippet,
            })
        })
        .collect();

    Ok(hits)
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let url = url.to_string();
    let text = run_http(move || async move {
        let client = http_client()?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|err| format!("request failed: {err}"))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|err| format!("read failed: {err}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {}", truncate_chars(&text, 200)));
        }
        Ok(text)
    })?;
    serde_json::from_str(&text).map_err(|err| format!("JSON decode failed: {err}"))
}

fn http_client() -> Result<reqwest::Client, String> {
    crate::net::client_builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|err| format!("scholar client build failed: {err}"))
}

/// Run an async HTTP future on a dedicated scoped thread (same pattern as
/// `web_search::run_http`).
fn run_http<F, Fut, T>(make_future: F) -> Result<T, String>
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = Result<T, String>>,
    T: Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| format!("scholar runtime build failed: {err}"))?;
                rt.block_on(make_future())
            })
            .join()
            .map_err(|_| "scholar request thread panicked".to_string())?
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

fn strip_html_tags(html: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"(?s)<[^>]+>").unwrap());
    let without_tags = tag_re.replace_all(html, " ");
    without_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scholar_search_empty_query_errors() {
        assert!(scholar_search("", 5).is_err());
        assert!(scholar_search("  ", 5).is_err());
    }

    #[test]
    fn strip_html_tags_removes_tags() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
    }
}
