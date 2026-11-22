mod cli;
// Parts of these only the desktop uses (the command line, backups, Windows OCR).
#[cfg_attr(mobile, allow(dead_code))]
mod db;
#[cfg_attr(mobile, allow(dead_code))]
mod ocr;
mod prefs;
mod selection;
#[cfg_attr(mobile, allow(dead_code))]
mod sync;

#[cfg(desktop)]
mod backup;
#[cfg(desktop)]
mod desktop;
#[cfg(desktop)]
mod import;
#[cfg(desktop)]
mod inbox;
#[cfg(desktop)]
mod updates;

use db::{Db, Entry, EntrySummary, TagCount};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{
    http::{Response, StatusCode},
    ipc::{InvokeBody, Request},
    AppHandle, Emitter, Manager, Runtime, State,
};

pub(crate) type CmdResult<T> = Result<T, String>;

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}


// ---------- data location (installed vs portable) ----------

/// Portable mode: a `portable.txt` next to the exe keeps everything in `<exe folder>\data`.
fn portable_dir() -> Option<&'static PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        exe_dir.join("portable.txt").exists().then(|| exe_dir.join("data"))
    })
    .as_ref()
}

/// The data folder without a running app (for the `bt` command line): the same place Tauri's
/// app_data_dir() uses — %APPDATA%, ~/.local/share or ~/Library/Application Support.
pub(crate) fn standalone_data_dir() -> Option<PathBuf> {
    if let Some(d) = portable_dir() {
        return Some(d.clone());
    }
    const ID: &str = "com.braintracker.app";
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))?
    };
    Some(base.join(ID))
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    match portable_dir() {
        Some(d) => Ok(d.clone()),
        None => app.path().app_data_dir().map_err(err),
    }
}

fn images_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("images");
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

/// Only plain file names we generated ourselves: no paths, no "..".
fn safe_image_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn image_mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

// ---------- entry commands ----------

#[tauri::command]
fn list_entries(
    db: State<Db>,
    query: Option<String>,
    tag: Option<String>,
) -> CmdResult<Vec<EntrySummary>> {
    let conn = db.0.lock().map_err(err)?;
    db::list_entries(&conn, query.as_deref(), tag.as_deref()).map_err(err)
}

/// Entries matching any of the words, best first — what quick search shows when no entry
/// has all of them (e.g. when searching for a whole error message).
#[tauri::command]
fn search_any(db: State<Db>, query: String) -> CmdResult<Vec<EntrySummary>> {
    let conn = db.0.lock().map_err(err)?;
    db::search_any(&conn, &query).map_err(err)
}

#[tauri::command]
fn get_entry(db: State<Db>, id: i64) -> CmdResult<Option<Entry>> {
    let conn = db.0.lock().map_err(err)?;
    db::get_entry(&conn, id).map_err(err)
}

#[tauri::command]
fn create_entry(
    db: State<Db>,
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
) -> CmdResult<Entry> {
    let conn = db.0.lock().map_err(err)?;
    db::create_entry(
        &conn,
        title.as_deref().unwrap_or(""),
        body.as_deref().unwrap_or(""),
        &tags.unwrap_or_default(),
    )
    .map_err(err)
}

/// Saves an entry from the editor. `base_updated_at` is the version the window last loaded:
/// if the entry changed since (e.g. `bt edit`), nothing is written and the current entry
/// comes back as a conflict for the window to merge. `keep_previous` puts the version being
/// replaced in the edit history whatever the timing.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_entry(
    app: AppHandle,
    db: State<Db>,
    id: i64,
    title: String,
    body: String,
    tags: Vec<String>,
    base_updated_at: Option<i64>,
    keep_previous: Option<bool>,
) -> CmdResult<db::Save> {
    let saved = {
        let conn = db.0.lock().map_err(err)?;
        db::save_entry(&conn, id, &title, &body, &tags, base_updated_at, keep_previous.unwrap_or(false)).map_err(err)?
    };
    if matches!(saved, db::Save::Saved(_)) {
        let _ = app.emit("entry-updated", id); // popped-out copies refresh
    }
    Ok(saved)
}

#[tauri::command]
fn set_pinned(db: State<Db>, id: i64, pinned: bool) -> CmdResult<()> {
    let conn = db.0.lock().map_err(err)?;
    db::set_pinned(&conn, id, pinned).map_err(err)
}

/// Moves an entry to the trash (kept for 30 days).
#[tauri::command]
fn delete_entry(db: State<Db>, id: i64) -> CmdResult<()> {
    let conn = db.0.lock().map_err(err)?;
    db::trash_entry(&conn, id).map_err(err)
}

/// Deletes for good — used for blank entries you created and abandoned.
#[tauri::command]
fn discard_entry(db: State<Db>, id: i64) -> CmdResult<()> {
    let conn = db.0.lock().map_err(err)?;
    db::purge_entry(&conn, id).map_err(err)
}

#[tauri::command]
fn list_tags(db: State<Db>) -> CmdResult<Vec<TagCount>> {
    let conn = db.0.lock().map_err(err)?;
    db::list_tags(&conn).map_err(err)
}

// ---------- trash ----------

#[tauri::command]
fn list_trash(db: State<Db>) -> CmdResult<Vec<db::TrashItem>> {
    let conn = db.0.lock().map_err(err)?;
    db::list_trash(&conn).map_err(err)
}

#[tauri::command]
fn trash_count(db: State<Db>) -> CmdResult<i64> {
    let conn = db.0.lock().map_err(err)?;
    db::trash_count(&conn).map_err(err)
}

#[tauri::command]
fn restore_entry(db: State<Db>, id: i64) -> CmdResult<()> {
    let conn = db.0.lock().map_err(err)?;
    db::restore_entry(&conn, id).map_err(err)
}

#[tauri::command]
fn purge_entry(db: State<Db>, id: i64) -> CmdResult<()> {
    let conn = db.0.lock().map_err(err)?;
    db::purge_entry(&conn, id).map_err(err)
}

#[tauri::command]
fn empty_trash(db: State<Db>) -> CmdResult<usize> {
    let conn = db.0.lock().map_err(err)?;
    db::purge_trash(&conn, 0).map_err(err)
}

// ---------- history ----------

#[tauri::command]
fn list_versions(db: State<Db>, entry_id: i64) -> CmdResult<Vec<db::VersionSummary>> {
    let conn = db.0.lock().map_err(err)?;
    db::list_versions(&conn, entry_id).map_err(err)
}

#[tauri::command]
fn get_version(db: State<Db>, id: i64) -> CmdResult<Option<db::Version>> {
    let conn = db.0.lock().map_err(err)?;
    db::get_version(&conn, id).map_err(err)
}

#[tauri::command]
fn restore_version(app: AppHandle, db: State<Db>, entry_id: i64, version_id: i64) -> CmdResult<Option<Entry>> {
    let restored = {
        let conn = db.0.lock().map_err(err)?;
        db::restore_version(&conn, entry_id, version_id).map_err(err)?
    };
    let _ = app.emit("entry-updated", entry_id);
    Ok(restored)
}

// ---------- images ----------

static IMAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Saves image bytes into the images folder under a fresh unique name.
fn store_image(app: &AppHandle, bytes: &[u8], ext: &str) -> Result<String, String> {
    let ext = ext.to_ascii_lowercase();
    let ext = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => ext,
        _ => "png".into(),
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = IMAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    // A few random letters too: the phone and the PC both make names, and synced images
    // must never clash.
    let name = format!("{ms}-{n}-{}.{ext}", &sync::random_id()[..6]);
    std::fs::write(images_dir(app)?.join(&name), bytes).map_err(err)?;
    ocr::queue(app, &name); // read its text in the background, for search
    Ok(name)
}

/// Receives raw image bytes (header `x-ext` = file extension) and returns the stored file name.
#[tauri::command]
fn save_image(app: AppHandle, request: Request<'_>) -> CmdResult<String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw image bytes".into());
    };
    let ext = request
        .headers()
        .get("x-ext")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("png")
        .to_string();
    store_image(&app, bytes, &ext)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnusedImages {
    count: usize,
    bytes: u64,
}

/// Screenshots in the images folder that no entry, trashed entry or old version uses.
fn unused_image_files(app: &AppHandle) -> Result<Vec<(PathBuf, u64)>, String> {
    let used = {
        let db = app.state::<Db>();
        let conn = db.0.lock().map_err(err)?;
        db::referenced_images(&conn).map_err(err)?
    };
    let mut out = Vec::new();
    for e in std::fs::read_dir(images_dir(app)?).map_err(err)?.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if path.is_file() && safe_image_name(&name) && !used.contains(&name) {
            out.push((path, e.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
    Ok(out)
}

#[tauri::command]
fn find_unused_images(app: AppHandle) -> CmdResult<UnusedImages> {
    let files = unused_image_files(&app)?;
    Ok(UnusedImages { count: files.len(), bytes: files.iter().map(|f| f.1).sum() })
}

#[tauri::command]
fn delete_unused_images(app: AppHandle) -> CmdResult<UnusedImages> {
    let files = unused_image_files(&app)?;
    let mut done = UnusedImages { count: 0, bytes: 0 };
    let mut removed = Vec::new();
    for (path, size) in files {
        if std::fs::remove_file(&path).is_ok() {
            done.count += 1;
            done.bytes += size;
            removed.extend(path.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
    }
    if let Ok(conn) = app.state::<Db>().0.lock() {
        let _ = db::forget_image_text(&conn, &removed);
    }
    Ok(done)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    data_dir: String,
    portable: bool,
    version: String,
}

#[tauri::command]
fn app_info(app: AppHandle) -> CmdResult<AppInfo> {
    Ok(AppInfo {
        data_dir: data_dir(&app)?.display().to_string(),
        portable: portable_dir().is_some(),
        version: app.package_info().version.to_string(),
    })
}

// ---------- offline mode, command placeholders ----------

#[tauri::command]
fn get_offline(app: AppHandle) -> bool {
    prefs::is_offline(&app)
}

#[tauri::command]
fn set_offline(app: AppHandle, enabled: bool) -> CmdResult<bool> {
    prefs::set_offline(&app, enabled)?;
    #[cfg(desktop)]
    sync::server::refresh(&app); // offline mode stops phone sync too
    let _ = app.emit("offline-changed", enabled);
    Ok(enabled)
}

#[tauri::command]
fn get_placeholders(app: AppHandle) -> prefs::PlaceholderConfig {
    prefs::placeholders(&app)
}

#[tauri::command]
fn set_placeholders(app: AppHandle, config: prefs::PlaceholderConfig) -> CmdResult<prefs::PlaceholderConfig> {
    prefs::set_placeholders(&app, config)
}

#[tauri::command]
fn remember_placeholders(app: AppHandle, values: std::collections::HashMap<String, String>) -> CmdResult<()> {
    prefs::remember(&app, values)
}

#[cfg(desktop)]
/// Opens (and if needed creates or upgrades) the database in the data folder: the same
/// folder app_data_dir() gives, or `data` beside the exe in portable mode.
fn open_database() -> Result<rusqlite::Connection, String> {
    let dir = standalone_data_dir().ok_or("can’t find the data folder")?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    db::open(&dir.join("braintracker.db")).map_err(err)
}

/// Entry point of the `bt` console program (src/bin/bt.rs).
pub fn console_main() -> i32 {
    cli::console_main()
}

/// Serves pasted images to the webview at http://btimg.localhost/<name>.
fn serve_image<R: Runtime>(ctx: tauri::UriSchemeContext<'_, R>, request: tauri::http::Request<Vec<u8>>) -> Response<Vec<u8>> {
    let name = request.uri().path().trim_start_matches('/').to_string();
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap();
    if !safe_image_name(&name) {
        return not_found();
    }
    let dir = match portable_dir() {
        Some(d) => d.clone(),
        None => match ctx.app_handle().path().app_data_dir() {
            Ok(d) => d,
            Err(_) => return not_found(),
        },
    };
    match std::fs::read(dir.join("images").join(&name)) {
        Ok(bytes) => Response::builder()
            .header("Content-Type", image_mime(&name))
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(bytes)
            .unwrap(),
        Err(_) => not_found(),
    }
}

#[cfg(desktop)]
pub fn run() {
    // Commands like `braintracker search docker` run in the terminal and exit; only a plain
    // launch or `--open` starts the app.
    let args: Vec<String> = std::env::args().collect();
    let gui_action = match cli::parse(&args) {
        cli::Parsed::Info(text) => return cli::print_out(&text),
        cli::Parsed::Error(e) => {
            cli::print_err(&e);
            std::process::exit(2);
        }
        cli::Parsed::Headless(cmd, json) => std::process::exit(cli::execute(cmd, json)),
        cli::Parsed::Gui(action) => action,
    };

    // Portable mode also keeps WebView2's cache beside the exe instead of in %LOCALAPPDATA%
    // (unless something driving the app, e.g. an automated test, already chose a folder).
    let chosen = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some()
        || std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_ok_and(|a| a.contains("--user-data-dir"));
    if let Some(dir) = portable_dir().filter(|_| !chosen) {
        let webview = dir.join("webview");
        let _ = std::fs::create_dir_all(&webview);
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", webview);
    }

    let db = match open_database() {
        Ok(conn) => Db::new(conn),
        Err(e) => {
            cli::print_err(&format!("can’t open your notes: {e}"));
            std::process::exit(1);
        }
    };
    desktop::run(gui_action, db);
}

/// The phone app: the same notes, a layout for touch (src/mobile/), and sync with the PC over
/// Wi-Fi instead of the desktop's tray, shortcuts and popups.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_barcode_scanner::init())
        .register_uri_scheme_protocol("btimg", serve_image)
        .setup(|app| {
            // On a phone the data folder is only known once the app is running.
            let dir = data_dir(app.handle())?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("braintracker.db"))?;
            let _ = db::purge_expired_trash(&conn);
            app.manage(Db::new(conn));
            sync::client::start(app.handle().clone());
            let _ = app.emit("db-ready", ());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            db_ready,
            list_entries,
            search_any,
            get_entry,
            create_entry,
            update_entry,
            set_pinned,
            delete_entry,
            discard_entry,
            list_tags,
            list_trash,
            trash_count,
            restore_entry,
            purge_entry,
            empty_trash,
            list_versions,
            get_version,
            restore_version,
            find_unused_images,
            delete_unused_images,
            save_image,
            app_info,
            get_offline,
            set_offline,
            get_placeholders,
            set_placeholders,
            remember_placeholders,
            sync::commands::sync_status,
            sync::commands::pair_with,
            sync::commands::sync_now,
            sync::commands::unpair
        ])
        .run(tauri::generate_context!())
        .expect("error while running BrainTracker");
}

/// Whether the notes are open yet (on a phone they open a moment after the window).
#[cfg(mobile)]
#[tauri::command]
fn db_ready(app: AppHandle) -> bool {
    app.try_state::<Db>().is_some()
}
