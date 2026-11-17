//! Automatic daily backups and one-click restore.
//!
//! Layout of the backup folder:
//!   BrainTracker-2026-11-18_143005\braintracker.db   ← one snapshot per backup
//!   images\                                          ← shared; screenshots never change,
//!                                                       so they're copied once, not per snapshot

use crate::{data_dir, db, db::Db, err, portable_dir};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const PREFIX: &str = "BrainTracker-";
const DB_FILE: &str = "braintracker.db";
const DAY_MS: i64 = 86_400_000;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupConfig {
    pub enabled: bool,
    /// None = the default folder
    pub dir: Option<String>,
    pub keep: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self { enabled: true, dir: None, keep: 30 }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItem {
    name: String,
    created_at: i64,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    config: BackupConfig,
    dir: String,
    default_dir: String,
    last_backup: Option<i64>,
    last_error: Option<String>,
    backups: Vec<BackupItem>,
}

/// Serializes backups/restores and remembers the last failure for the UI.
#[derive(Default)]
pub struct BackupState {
    lock: Mutex<()>,
    last_error: Mutex<Option<String>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn with_conn<T>(app: &AppHandle, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(err)?;
    f(&conn).map_err(err)
}

pub fn load_config(app: &AppHandle) -> BackupConfig {
    with_conn(app, |c| db::get_setting(c, "backup_config"))
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(app: &AppHandle, cfg: &BackupConfig) -> Result<(), String> {
    let json = serde_json::to_string(cfg).map_err(err)?;
    with_conn(app, |c| db::set_setting(c, "backup_config", &json))
}

fn default_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if portable_dir().is_some() {
        Ok(data_dir(app)?.join("backups"))
    } else {
        Ok(app.path().document_dir().map_err(err)?.join("BrainTracker Backups"))
    }
}

fn backup_dir(app: &AppHandle, cfg: &BackupConfig) -> Result<PathBuf, String> {
    match &cfg.dir {
        Some(d) if !d.trim().is_empty() => Ok(PathBuf::from(d)),
        _ => default_dir(app),
    }
}

/// Our snapshot folders, newest first. Anything else in the folder is ignored (and never deleted).
fn snapshots(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(PREFIX))
                && p.join(DB_FILE).is_file()
        })
        .collect();
    out.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    out
}

/// Copies files from `src` that `dst` doesn't have yet.
fn copy_missing(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dst).map_err(err)?;
    for entry in std::fs::read_dir(src).map_err(err)?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let target = dst.join(entry.file_name());
            if !target.exists() {
                std::fs::copy(&path, &target).map_err(err)?;
            }
        }
    }
    Ok(())
}

fn backup_locked(app: &AppHandle, label: Option<&str>) -> Result<String, String> {
    let cfg = load_config(app);
    let dir = backup_dir(app, &cfg)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can’t use backup folder {}: {e}", dir.display()))?;

    let (stamp, fp) = with_conn(app, |c| Ok((db::local_stamp(c)?, db::fingerprint(c)?)))?;
    let mut name = format!("{PREFIX}{stamp}");
    if let Some(l) = label {
        name = format!("{name}-{l}");
    }
    let mut snap = dir.join(&name);
    let mut n = 2;
    while snap.exists() {
        snap = dir.join(format!("{name}-{n}"));
        n += 1;
    }
    std::fs::create_dir_all(&snap).map_err(err)?;
    with_conn(app, |c| db::vacuum_into(c, &snap.join(DB_FILE)))?;

    copy_missing(&data_dir(app)?.join("images"), &dir.join("images"))?;

    for old in snapshots(&dir).into_iter().skip(cfg.keep.max(1) as usize) {
        let _ = std::fs::remove_dir_all(old);
    }

    let now = now_ms().to_string();
    with_conn(app, |c| {
        db::set_setting(c, "backup_last", &now)?;
        db::set_setting(c, "backup_fingerprint", &fp)
    })?;
    Ok(snap.file_name().unwrap_or_default().to_string_lossy().into_owned())
}

/// Takes a snapshot now. `label` is appended to the folder name (e.g. "before-restore").
pub fn run_backup(app: &AppHandle, label: Option<&str>) -> Result<String, String> {
    let state = app.state::<BackupState>();
    let _guard = state.lock.lock().map_err(err)?;
    let result = backup_locked(app, label);
    if let Ok(mut e) = state.last_error.lock() {
        *e = result.as_ref().err().cloned();
    }
    result
}

/// Backs up if enabled, at least a day has passed, and something changed.
fn maybe_backup(app: &AppHandle) {
    let cfg = load_config(app);
    if !cfg.enabled {
        return;
    }
    let Ok((last, last_fp, fp)) = with_conn(app, |c| {
        Ok((
            db::get_setting(c, "backup_last")?.and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
            db::get_setting(c, "backup_fingerprint")?.unwrap_or_default(),
            db::fingerprint(c)?,
        ))
    }) else {
        return;
    };
    if now_ms() - last < DAY_MS || fp == last_fp {
        return;
    }
    let _ = run_backup(app, None);
}

/// Checks once a minute after launch, then hourly.
pub fn start_scheduler(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            maybe_backup(&app);
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

pub fn status(app: &AppHandle) -> Result<BackupStatus, String> {
    let config = load_config(app);
    let dir = backup_dir(app, &config)?;
    let backups = snapshots(&dir)
        .into_iter()
        .map(|p| {
            let meta = std::fs::metadata(p.join(DB_FILE)).ok();
            BackupItem {
                name: p.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                created_at: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0),
                size: meta.map(|m| m.len()).unwrap_or(0),
            }
        })
        .collect();
    let last_backup = with_conn(app, |c| db::get_setting(c, "backup_last"))?.and_then(|s| s.parse().ok());
    let last_error = app.state::<BackupState>().last_error.lock().ok().and_then(|e| e.clone());
    Ok(BackupStatus {
        config,
        dir: dir.display().to_string(),
        default_dir: default_dir(app)?.display().to_string(),
        last_backup,
        last_error,
        backups,
    })
}

/// Replaces the live database with a snapshot. Takes a safety backup first and keeps
/// the current settings (shortcuts, backup folder, …) rather than the snapshot's.
pub fn restore(app: &AppHandle, name: &str) -> Result<(), String> {
    if !name.starts_with(PREFIX) || name.contains(['/', '\\']) || name.contains("..") {
        return Err("Not a BrainTracker backup".into());
    }
    let cfg = load_config(app);
    let dir = backup_dir(app, &cfg)?;
    let src = dir.join(name).join(DB_FILE);

    // Make sure the snapshot is a readable BrainTracker database before touching anything.
    Connection::open_with_flags(&src, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0)))
        .map_err(|e| format!("That backup can’t be read: {e}"))?;

    // Copy it aside first: the safety backup below may prune old snapshots, including this one.
    let staged = data_dir(app)?.join("restore-staging.db");
    std::fs::copy(&src, &staged).map_err(err)?;
    let src = staged;

    if let Err(e) = run_backup(app, Some("before-restore")) {
        let _ = std::fs::remove_file(&src);
        return Err(format!("Restore cancelled — couldn’t save a safety backup first: {e}"));
    }

    let state = app.state::<BackupState>();
    let _guard = state.lock.lock().map_err(err)?;
    let live = data_dir(app)?.join(DB_FILE);
    {
        let db = app.state::<Db>();
        let mut conn = db.0.lock().map_err(err)?;
        let settings = db::all_settings(&conn).map_err(err)?;

        // Swap in a dummy connection so the live file is closed while we replace it.
        *conn = Connection::open_in_memory().map_err(err)?;
        let copied = (|| -> Result<(), String> {
            for ext in ["-wal", "-shm"] {
                let p = PathBuf::from(format!("{}{ext}", live.display()));
                if p.exists() {
                    std::fs::remove_file(p).map_err(err)?;
                }
            }
            std::fs::copy(&src, &live).map_err(err)?;
            Ok(())
        })();
        *conn = db::open(&live).map_err(err)?;
        let _ = std::fs::remove_file(&src);
        copied?;
        for (k, v) in settings {
            db::set_setting(&conn, &k, &v).map_err(err)?;
        }
    }
    copy_missing(&dir.join("images"), &data_dir(app)?.join("images"))?;
    let _ = app.emit("data-restored", ());
    Ok(())
}
