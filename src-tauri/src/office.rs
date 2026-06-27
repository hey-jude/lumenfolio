//! Knowledge-base pivot (P3): text extraction from Office formats.
//!
//! Office files are ZIP archives of XML. We pull readable text only (for the
//! chunk → graph → claims pipeline); fidelity rendering is the frontend
//! preview's job (docx/xlsx), not this extractor. Returns `(text, role)` blocks
//! in document order, matching the shape the text index path expects.

use std::io::Read;
use std::path::Path;

use calamine::{Data, Reader as _};
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// Cap to keep a runaway spreadsheet/deck from flooding the index.
const MAX_BLOCKS: usize = 5000;

pub(crate) fn extract_office_blocks(
    path: &Path,
    content_type: &str,
) -> Result<Vec<(String, String)>, String> {
    let blocks = match content_type {
        "docx" => extract_docx(path)?,
        "pptx" => extract_pptx(path)?,
        "xlsx" => extract_xlsx(path)?,
        other => return Err(format!("Unsupported office content type '{other}'")),
    };
    Ok(blocks.into_iter().take(MAX_BLOCKS).collect())
}

/// Whether a content type is an Office source indexed via this extractor.
pub(crate) fn is_office_source(content_type: &str) -> bool {
    matches!(content_type, "docx" | "xlsx" | "pptx")
}

/// Map a lowercase file extension to an Office content type, if supported.
pub(crate) fn office_content_type_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "docx" => Some("docx"),
        "xlsx" => Some("xlsx"),
        "pptx" => Some("pptx"),
        _ => None,
    }
}

fn read_zip_entry(path: &Path, entry: &str) -> Result<Option<String>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("Failed to open file: {err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("Not a valid Office (ZIP) file: {err}"))?;
    let mut zf = match archive.by_name(entry) {
        Ok(zf) => zf,
        Err(_) => return Ok(None),
    };
    let mut contents = String::new();
    zf.read_to_string(&mut contents)
        .map_err(|err| format!("Failed to read {entry}: {err}"))?;
    Ok(Some(contents))
}

fn zip_entry_names(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("Failed to open file: {err}"))?;
    let archive =
        zip::ZipArchive::new(file).map_err(|err| format!("Not a valid Office (ZIP) file: {err}"))?;
    Ok(archive.file_names().map(|name| name.to_string()).collect())
}

// ---------------------------------------------------------------------------
// DOCX — word/document.xml: <w:p> paragraphs, <w:t> text runs, heading via
// <w:pStyle w:val="Heading…">.
// ---------------------------------------------------------------------------

fn extract_docx(path: &Path) -> Result<Vec<(String, String)>, String> {
    let Some(xml) = read_zip_entry(path, "word/document.xml")? else {
        return Err("docx is missing word/document.xml".to_string());
    };
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut role = "body".to_string();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:p" => {
                    paragraph.clear();
                    role = "body".to_string();
                }
                b"w:t" => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"w:pStyle" {
                    if let Some(style) = attr_value(&e, b"w:val") {
                        if style.starts_with("Heading") || style.starts_with("Title") {
                            role = "heading".to_string();
                        }
                    }
                }
            }
            Ok(Event::Text(e)) if in_text => {
                if let Ok(text) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                    paragraph.push_str(&text);
                }
            }
            // Entities (&amp; &lt; …) and char refs arrive as separate events.
            Ok(Event::GeneralRef(e)) if in_text => {
                if let Some(text) = resolve_entity(&e) {
                    paragraph.push_str(&text);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => {
                    let trimmed = paragraph.trim();
                    if !trimmed.is_empty() {
                        blocks.push((trimmed.to_string(), role.clone()));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("Failed to parse docx XML: {err}")),
            _ => {}
        }
    }
    Ok(blocks)
}

// ---------------------------------------------------------------------------
// PPTX — ppt/slides/slideN.xml: <a:p> paragraphs, <a:t> text runs. One heading
// per slide ("Slide N") followed by its paragraph blocks.
// ---------------------------------------------------------------------------

fn extract_pptx(path: &Path) -> Result<Vec<(String, String)>, String> {
    let mut slide_names: Vec<String> = zip_entry_names(path)?
        .into_iter()
        .filter(|name| {
            name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
        })
        .collect();
    // Sort by the numeric suffix so slides come out in deck order (slide2 < slide10).
    slide_names.sort_by_key(|name| slide_number(name));

    let mut blocks = Vec::new();
    for (index, name) in slide_names.iter().enumerate() {
        let Some(xml) = read_zip_entry(path, name)? else {
            continue;
        };
        let paragraphs = parse_pptx_slide(&xml)?;
        blocks.push((format!("Slide {}", index + 1), "heading".to_string()));
        for paragraph in paragraphs {
            let trimmed = paragraph.trim();
            if !trimmed.is_empty() {
                blocks.push((trimmed.to_string(), "body".to_string()));
            }
        }
    }
    Ok(blocks)
}

fn slide_number(name: &str) -> u32 {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(u32::MAX)
}

fn parse_pptx_slide(xml: &str) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"a:p" => current.clear(),
                b"a:t" => in_text = true,
                _ => {}
            },
            Ok(Event::Text(e)) if in_text => {
                if let Ok(text) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                    current.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(e)) if in_text => {
                if let Some(text) = resolve_entity(&e) {
                    current.push_str(&text);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"a:t" => in_text = false,
                b"a:p" => {
                    if !current.trim().is_empty() {
                        paragraphs.push(current.clone());
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("Failed to parse pptx slide XML: {err}")),
            _ => {}
        }
    }
    Ok(paragraphs)
}

// ---------------------------------------------------------------------------
// XLSX — calamine reads cells directly. One heading per sheet, then one block
// per non-empty row (cells joined with " | ").
// ---------------------------------------------------------------------------

fn extract_xlsx(path: &Path) -> Result<Vec<(String, String)>, String> {
    let mut workbook: calamine::Xlsx<_> =
        calamine::open_workbook(path).map_err(|err| format!("Failed to open xlsx: {err}"))?;
    let mut blocks = Vec::new();
    let sheet_names = workbook.sheet_names().to_vec();
    for name in sheet_names {
        let Ok(range) = workbook.worksheet_range(&name) else {
            continue;
        };
        blocks.push((format!("Sheet: {name}"), "heading".to_string()));
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .map(cell_to_string)
                .filter(|cell| !cell.is_empty())
                .collect();
            if cells.is_empty() {
                continue;
            }
            blocks.push((cells.join(" | "), "body".to_string()));
        }
    }
    Ok(blocks)
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            // Render integers without a trailing ".0".
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{e:?}"),
    }
}

/// Resolve a `&ref;` entity event: numeric char refs (`&#123;`) and the five
/// predefined XML entities. Unknown named entities are dropped.
fn resolve_entity(e: &quick_xml::events::BytesRef<'_>) -> Option<String> {
    if let Ok(Some(ch)) = e.resolve_char_ref() {
        return Some(ch.to_string());
    }
    match e.decode().ok()?.as_ref() {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        _ => None,
    }
}

fn attr_value(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.as_ref() == key {
            attr.unescape_value().ok().map(|value| value.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extract_docx_reads_paragraphs_and_headings() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lumenfolio-docx-{}-{suffix}.docx",
            std::process::id()
        ));
        let file = std::fs::File::create(&path).expect("create");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("entry");
        let xml = r#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>My Title</w:t></w:r></w:p>
            <w:p><w:r><w:t xml:space="preserve">Hello </w:t></w:r><w:r><w:t>world &amp; more.</w:t></w:r></w:p>
            <w:p><w:r><w:t>  </w:t></w:r></w:p>
        </w:body></w:document>"#;
        writer.write_all(xml.as_bytes()).expect("write");
        writer.finish().expect("finish");

        let blocks = extract_office_blocks(&path, "docx").expect("extract");
        assert_eq!(
            blocks,
            vec![
                ("My Title".to_string(), "heading".to_string()),
                // Runs concatenate within a paragraph; entity is unescaped; the
                // whitespace-only paragraph is dropped.
                ("Hello world & more.".to_string(), "body".to_string()),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn xlsx_cell_renders_integer_floats_without_decimal() {
        assert_eq!(cell_to_string(&Data::Float(42.0)), "42");
        assert_eq!(cell_to_string(&Data::Float(3.5)), "3.5");
        assert_eq!(cell_to_string(&Data::String("  hi ".to_string())), "hi");
        assert_eq!(cell_to_string(&Data::Empty), "");
    }

    #[test]
    fn slide_number_sorts_numerically() {
        assert!(slide_number("ppt/slides/slide2.xml") < slide_number("ppt/slides/slide10.xml"));
    }

    #[test]
    fn parse_pptx_slide_collects_text_runs_per_paragraph() {
        let xml = r#"<p:sld xmlns:a="x"><a:p><a:r><a:t>Hello </a:t></a:r><a:r><a:t>world</a:t></a:r></a:p><a:p><a:t>Second</a:t></a:p></p:sld>"#;
        let paragraphs = parse_pptx_slide(xml).expect("parse");
        assert_eq!(paragraphs, vec!["Hello world".to_string(), "Second".to_string()]);
    }
}
