//! Inbox folder: Markdown/text notes and images put into a folder you pick are imported
//! automatically. Point it at a folder in OneDrive, Dropbox or Google Drive and you can send
//! notes from your phone — through your own cloud app, with no BrainTracker account or server.
//!
//! The folder is checked every few seconds. A file is imported once it has stopped changing
//! (so half-synced files are left alone), then moved into an `Imported` subfolder.

use crate::{db, db::Db, err, import};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

const SETTING: &str = "inbox";
pub const DONE_DIR: &str = "Imported";
const TAG: &str = "inbox";
const POLL: Duration = Duration::from_secs(10);
/// A file that hasn't changed for this long is complete even if we've never seen it before
/// (e.g. it arrived while the app wasn't running).
const SETTLED: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxConfig {
    pub enabled: bool,
    /// None = Documents\BrainTracker Inbox
    pub dir: Option<String>,
}

#[derive(Default)]
struct Report {
    last_import: Option<i64>,
    last_count: usize,
    last_error: Option<String>,
}

#[derive(Default)]
pub struct InboxState {
    /// Size and modified time of each file at the previous check.
    seen: Mutex<HashMap<PathBuf, (u64, SystemTime)>>,
    report: Mutex<Report>,
    /// One check at a time (the timer and "Check now").
    running: Mutex<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxStatus {
    config: InboxConfig,
    dir: String,
    last_import: Option<i64>,
    last_count: usize,
    last_error: Option<String>,
}

pub fn load_config(app: &AppHandle) -> InboxConfig {
    let db = app.state::<Db>();
    let Ok(conn) = db.0.lock() else { return InboxConfig::default() };
    db::get_setting(&conn, SETTING)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(app: &AppHandle, cfg: &InboxConfig) -> Result<(), String> {
    if cfg.enabled {
        std::fs::create_dir_all(inbox_dir(app, cfg)?).map_err(|e| format!("Can’t use that folder: {e}"))?;
    }
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(err)?;
    db::set_setting(&conn, SETTING, &serde_json::to_string(cfg).map_err(err)?).map_err(err)
}

pub fn inbox_dir(app: &AppHandle, cfg: &InboxConfig) -> Result<PathBuf, String> {
    match cfg.dir.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => Ok(app.path().document_dir().map_err(err)?.join("BrainTracker Inbox")),
    }
}

pub fn status(app: &AppHandle) -> Result<InboxStatus, String> {
    let config = load_config(app);
    let dir = inbox_dir(app, &config)?.display().to_string();
    let state = app.state::<InboxState>();
    let r = state.report.lock().map_err(err)?;
    Ok(InboxStatus {
        config,
        dir,
        last_import: r.last_import,
        last_count: r.last_count,
        last_error: r.last_error.clone(),
    })
}

/// Checks the folder every few seconds while the inbox is on.
pub fn start(app: AppHandle) {
    app.manage(InboxState::default());
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        let cfg = load_config(&app);
        if cfg.enabled {
            let _ = check(&app, &cfg, SETTLED);
        }
    });
}

/// "Check now": also takes files that only just arrived, as long as they've stopped changing.
pub fn check_now(app: &AppHandle) -> Result<usize, String> {
    let cfg = load_config(app);
    if !cfg.enabled {
        return Ok(0);
    }
    check(app, &cfg, Duration::from_secs(3))
}

/// Skips files that are hidden, temporary or still being written by a sync app.
fn ignorable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with('~')
        || [".tmp", ".part", ".partial", ".crdownload", ".download"].iter().any(|s| lower.ends_with(s))
}

fn check(app: &AppHandle, cfg: &InboxConfig, settled: Duration) -> Result<usize, String> {
    let state = app.state::<InboxState>();
    let _one = state.running.lock().map_err(err)?;
    let result = import_ready(app, cfg, settled, &state);
    if let Ok(mut r) = state.report.lock() {
        match &result {
            Ok(0) => r.last_error = None,
            Ok(n) => {
                r.last_import = Some(db::now_ms());
                r.last_count = *n;
                r.last_error = None;
            }
            Err(e) => r.last_error = Some(e.clone()),
        }
    }
    if result.as_ref().is_ok_and(|n| *n > 0) {
        let _ = app.emit("entries-changed", ());
    }
    result
}

fn import_ready(app: &AppHandle, cfg: &InboxConfig, settled: Duration, state: &InboxState) -> Result<usize, String> {
    let dir = inbox_dir(app, cfg)?;
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("Can’t read the inbox folder: {e}"))?;
    let now = SystemTime::now();

    // Files that have stopped changing since the last check (or long enough ago).
    let mut ready: Vec<PathBuf> = Vec::new();
    {
        let mut seen = state.seen.lock().map_err(err)?;
        let mut current = HashMap::new();
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() || ignorable(&name) || !(import::is_note_file(&path) || import::is_image_file(&path)) {
                continue;
            }
            let modified = meta.modified().unwrap_or(now);
            let sig = (meta.len(), modified);
            let stable = seen.get(&path) == Some(&sig);
            let old = now.duration_since(modified).unwrap_or_default() >= settled;
            if meta.len() > 0 && (stable || old) {
                ready.push(path.clone());
            }
            current.insert(path, sig);
        }
        *seen = current;
    }
    ready.sort();

    let tags = vec![TAG.to_string()];
    let mut imported = 0;
    let mut errors = Vec::new();
    let mut done: Vec<PathBuf> = Vec::new();

    // Notes first: images they show are copied into the note and tidied away with it,
    // rather than also becoming entries of their own.
    for path in ready.iter().filter(|p| import::is_note_file(p)) {
        match import::import_file(app, path, &tags) {
            Ok((outcome, images)) => {
                if matches!(outcome, import::Outcome::Imported) {
                    imported += 1;
                }
                done.push(path.clone());
                done.extend(images.into_iter().filter(|i| i.parent() == Some(dir.as_path())));
            }
            Err(e) => errors.push(format!("{}: {e}", file_name(path))),
        }
    }
    let shown_in_notes = done.clone();
    for path in ready.iter().filter(|p| import::is_image_file(p) && !shown_in_notes.contains(p)) {
        match import::import_image_file(app, path, &tags) {
            Ok(outcome) => {
                if matches!(outcome, import::Outcome::Imported) {
                    imported += 1;
                }
                done.push(path.clone());
            }
            Err(e) => errors.push(format!("{}: {e}", file_name(path))),
        }
    }

    let done_dir = dir.join(DONE_DIR);
    for path in &done {
        if let Err(e) = move_into(path, &done_dir) {
            errors.push(format!("{}: imported, but couldn’t move it to {DONE_DIR}: {e}", file_name(path)));
        }
    }
    if errors.is_empty() {
        Ok(imported)
    } else {
        Err(errors.join(" · "))
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Moves a file into `dir`, adding " (2)", " (3)"… if that name is taken.
fn move_into(path: &Path, dir: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(()); // e.g. an image two notes both showed
    }
    std::fs::create_dir_all(dir)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut target = dir.join(format!("{stem}{ext}"));
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    if std::fs::rename(path, &target).is_err() {
        // Different drive, or a sync app holding the file: copy, then remove.
        std::fs::copy(path, &target)?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_temporary_and_hidden_files() {
        for name in [".DS_Store", "~$note.md", "note.md.tmp", "photo.jpg.part", "x.crdownload"] {
            assert!(ignorable(name), "{name}");
        }
        for name in ["note.md", "Screenshot 2026-11-21.png", "todo.txt"] {
            assert!(!ignorable(name), "{name}");
        }
    }

    #[test]
    fn moving_never_overwrites() {
        let base = std::env::temp_dir().join(format!("bt-inbox-{}", db::now_ms()));
        let done = base.join(DONE_DIR);
        std::fs::create_dir_all(&base).unwrap();
        for i in 0..2 {
            let f = base.join("note.md");
            std::fs::write(&f, format!("v{i}")).unwrap();
            move_into(&f, &done).unwrap();
            assert!(!f.exists());
        }
        assert_eq!(std::fs::read_to_string(done.join("note.md")).unwrap(), "v0");
        assert_eq!(std::fs::read_to_string(done.join("note (2).md")).unwrap(), "v1");
        let _ = std::fs::remove_dir_all(&base);
    }
}
