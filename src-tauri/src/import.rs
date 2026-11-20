//! Import Markdown / text files as entries.
//!
//! Understands BrainTracker's own export format (title, `Tags:`, `Created: … · Updated: …`,
//! `---`), simple YAML front matter (`title`, `tags`, `date`/`created`), and plain files
//! (first `# heading` or the file name becomes the title). Local images referenced from the
//! Markdown are copied in as screenshots.

use crate::{db, db::Db, err, store_image};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter, Manager};

pub const EXTENSIONS: [&str; 3] = ["md", "markdown", "txt"];
const MAX_FILES: usize = 5000;
pub const IMAGE_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// What happened to one imported file.
pub enum Outcome {
    Imported,
    /// Empty, or already imported before.
    Skipped,
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn is_note_file(path: &Path) -> bool {
    has_ext(path, &EXTENSIONS)
}

pub fn is_image_file(path: &Path) -> bool {
    has_ext(path, &IMAGE_EXTS)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    imported: usize,
    skipped: usize,
    failed: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

fn split_tags(s: &str) -> Vec<String> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

pub fn parse_markdown(text: &str, fallback_title: &str) -> Parsed {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines: Vec<&str> = text.lines().collect();
    let mut p = Parsed::default();
    let mut i = 0;

    // YAML front matter
    if lines.first().map(|l| l.trim()) == Some("---") {
        if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") {
            for l in &lines[1..=end] {
                if let Some((k, v)) = l.split_once(':') {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    match k.trim().to_ascii_lowercase().as_str() {
                        "title" => p.title = v.to_string(),
                        "tags" | "keywords" => p.tags.extend(split_tags(v)),
                        "date" | "created" => p.created = Some(v.replace('T', " ")),
                        "updated" | "modified" => p.updated = Some(v.replace('T', " ")),
                        _ => {}
                    }
                }
            }
            i = end + 2;
        }
    }

    let skip_blank = |i: &mut usize, lines: &[&str]| {
        while *i < lines.len() && lines[*i].trim().is_empty() {
            *i += 1;
        }
    };

    skip_blank(&mut i, &lines);
    if i < lines.len() {
        if let Some(h) = lines[i].strip_prefix("# ") {
            if p.title.is_empty() {
                p.title = h.trim().to_string();
            }
            i += 1;
        }
    }

    // BrainTracker export metadata block
    let mut saw_meta = false;
    loop {
        skip_blank(&mut i, &lines);
        let Some(l) = lines.get(i).map(|l| l.trim()) else { break };
        if let Some(t) = l.strip_prefix("Tags:") {
            p.tags.extend(split_tags(t));
        } else if let Some(rest) = l.strip_prefix("Created:") {
            let (c, u) = rest.split_once('·').unwrap_or((rest, ""));
            p.created = Some(c.trim().to_string());
            if let Some(u) = u.trim().strip_prefix("Updated:") {
                p.updated = Some(u.trim().to_string());
            }
        } else if l == "---" && saw_meta {
            i += 1;
            break;
        } else {
            break;
        }
        saw_meta = true;
        i += 1;
    }

    lines.drain(..i.min(lines.len()));
    p.body = lines.join("\n").trim().to_string();
    if p.title.is_empty() {
        p.title = fallback_title.to_string();
    }
    p
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES {
        return;
    }
    if path.is_dir() {
        let hidden = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
        if hidden {
            return;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            collect_files(&p, out);
        }
    } else if is_note_file(path) {
        out.push(path.to_path_buf());
    }
}

/// Copies local images referenced as `](relative/path.png)` and rewrites them to `](img:…)`.
/// Also returns the image files it copied in.
fn import_images(app: &AppHandle, body: &str, base: &Path) -> (String, Vec<PathBuf>) {
    let mut used = Vec::new();
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(pos) = rest.find("](") {
        out.push_str(&rest[..pos + 2]);
        rest = &rest[pos + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = &rest[..end];
        let clean = target.split_whitespace().next().unwrap_or("").trim_matches(|c| c == '<' || c == '>');
        let is_remote = clean.contains("://") || clean.starts_with("img:") || clean.starts_with("data:");
        let ext = Path::new(clean).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        let mut replaced = false;
        if !is_remote && ext.as_deref().is_some_and(|e| IMAGE_EXTS.contains(&e)) {
            let file = base.join(clean.replace("%20", " "));
            if let Ok(bytes) = std::fs::read(&file) {
                if let Ok(name) = store_image(app, &bytes, ext.as_deref().unwrap_or("png")) {
                    out.push_str("img:");
                    out.push_str(&name);
                    replaced = true;
                    used.push(file);
                }
            }
        }
        if !replaced {
            out.push_str(target);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    (out, used)
}

fn modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(db::now_ms)
}

/// Imports one Markdown/text file, adding `extra_tags`. Also returns the image files it
/// copied in (so the inbox can tidy them away too). Err is a reason to show the user.
pub fn import_file(app: &AppHandle, file: &Path, extra_tags: &[String]) -> Result<(Outcome, Vec<PathBuf>), String> {
    let text = String::from_utf8_lossy(&std::fs::read(file).map_err(err)?).into_owned();
    let stem = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut parsed = parse_markdown(&text, &stem);
    if parsed.title.trim().is_empty() && parsed.body.trim().is_empty() {
        return Ok((Outcome::Skipped, Vec::new()));
    }
    parsed.tags.extend(extra_tags.iter().cloned());
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let (body, images) = import_images(app, &parsed.body, &base);
    let mtime = modified_ms(file);

    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(err)?;
    let created = parsed.created.as_deref().and_then(|s| db::local_to_ms(&conn, s)).unwrap_or(mtime);
    let updated = parsed
        .updated
        .as_deref()
        .and_then(|s| db::local_to_ms(&conn, s))
        .unwrap_or(created);
    if db::entry_exists(&conn, &parsed.title, created).map_err(err)? {
        return Ok((Outcome::Skipped, images));
    }
    db::import_entry(&conn, &parsed.title, &body, &parsed.tags, created, updated).map_err(err)?;
    Ok((Outcome::Imported, images))
}

/// Turns a lone image file into an entry showing it (the screenshot's text becomes
/// searchable once OCR has read it).
pub fn import_image_file(app: &AppHandle, file: &Path, extra_tags: &[String]) -> Result<Outcome, String> {
    let bytes = std::fs::read(file).map_err(err)?;
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let title = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(['_', '-'], " ")
        .trim()
        .to_string();
    let title = if title.is_empty() { "Image".to_string() } else { title };
    let created = modified_ms(file);
    {
        let db = app.state::<Db>();
        let conn = db.0.lock().map_err(err)?;
        if db::entry_exists(&conn, &title, created).map_err(err)? {
            return Ok(Outcome::Skipped);
        }
    }
    let name = store_image(app, &bytes, &ext)?;
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(err)?;
    db::import_entry(&conn, &title, &format!("![](img:{name})"), extra_tags, created, created)
        .map_err(err)?;
    Ok(Outcome::Imported)
}

pub fn import_paths(app: &AppHandle, paths: &[String]) -> Result<ImportResult, String> {
    let mut files = Vec::new();
    for p in paths {
        collect_files(Path::new(p), &mut files);
    }
    let mut result = ImportResult::default();

    for file in files {
        match import_file(app, &file, &[]) {
            Ok((Outcome::Imported, _)) => result.imported += 1,
            Ok((Outcome::Skipped, _)) => result.skipped += 1,
            Err(e) => {
                if result.failed.len() < 20 {
                    let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    result.failed.push(format!("{name}: {e}"));
                }
            }
        }
    }
    let _ = app.emit("entries-changed", ());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_braintracker_export() {
        let md = "# Fixed docker DNS\n\nTags: #docker #networking\nCreated: 2026-11-17 14:32 · Updated: 2026-11-18 09:00\n\n---\n\nEdited resolv.conf\n\n---\nmore";
        let p = parse_markdown(md, "file");
        assert_eq!(p.title, "Fixed docker DNS");
        assert_eq!(p.tags, vec!["docker", "networking"]);
        assert_eq!(p.created.as_deref(), Some("2026-11-17 14:32"));
        assert_eq!(p.updated.as_deref(), Some("2026-11-18 09:00"));
        assert_eq!(p.body, "Edited resolv.conf\n\n---\nmore");
    }

    #[test]
    fn parses_front_matter_and_plain_files() {
        let p = parse_markdown("---\ntitle: \"VPN setup\"\ntags: [vpn, home]\ndate: 2025-01-02\n---\nSteps here", "x");
        assert_eq!((p.title.as_str(), p.body.as_str()), ("VPN setup", "Steps here"));
        assert_eq!(p.tags, vec!["vpn", "home"]);
        assert_eq!(p.created.as_deref(), Some("2025-01-02"));

        let p = parse_markdown("just some notes\nline two", "my-notes");
        assert_eq!(p.title, "my-notes");
        assert_eq!(p.body, "just some notes\nline two");

        // A horizontal rule in a normal note isn't mistaken for export metadata
        let p = parse_markdown("# T\n\n---\nbody", "x");
        assert_eq!(p.body, "---\nbody");
    }
}
