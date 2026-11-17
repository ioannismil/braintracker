mod backup;
mod db;
mod import;
mod updates;

use db::{Db, Entry, EntrySummary, TagCount};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{
    http::{Response, StatusCode},
    ipc::{InvokeBody, Request},
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const MAIN: &str = "main";
const CAPTURE: &str = "capture";

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------- shortcuts ----------

/// Every configurable shortcut, stored as "Ctrl+Shift+E"-style strings.
/// `show_main` and `capture` are system-wide; the rest only work inside the app.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Shortcuts {
    show_main: String,
    capture: String,
    new_entry: String,
    search: String,
    preview: String,
    settings: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            show_main: "Ctrl+Shift+Space".into(),
            capture: "Ctrl+Shift+E".into(),
            new_entry: "Ctrl+T".into(),
            search: "Ctrl+K".into(),
            preview: "Ctrl+E".into(),
            settings: "Ctrl+,".into(),
        }
    }
}

#[derive(Default)]
struct Hotkeys {
    config: Shortcuts,
    main: Option<Shortcut>,
    capture: Option<Shortcut>,
}

struct HotkeyState(Mutex<Hotkeys>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutsInfo {
    shortcuts: Shortcuts,
    main_registered: bool,
    capture_registered: bool,
}

fn info_of(h: &Hotkeys) -> ShortcutsInfo {
    ShortcutsInfo {
        shortcuts: h.config.clone(),
        main_registered: h.main.is_some(),
        capture_registered: h.capture.is_some(),
    }
}

fn parse_global(combo: &str) -> Result<Shortcut, String> {
    combo
        .parse::<Shortcut>()
        .map_err(|_| format!("“{combo}” can’t be used as a system-wide shortcut"))
}

fn unregister_globals(app: &AppHandle, h: &mut Hotkeys) {
    let gs = app.global_shortcut();
    if let Some(s) = h.main.take() {
        let _ = gs.unregister(s);
    }
    if let Some(s) = h.capture.take() {
        let _ = gs.unregister(s);
    }
}

/// (Re)registers both system-wide shortcuts. One may fail if another app already owns it.
fn register_globals(app: &AppHandle, h: &mut Hotkeys) {
    unregister_globals(app, h);
    let gs = app.global_shortcut();
    h.main = parse_global(&h.config.show_main)
        .ok()
        .filter(|s| gs.register(*s).is_ok());
    h.capture = parse_global(&h.config.capture)
        .ok()
        .filter(|s| gs.register(*s).is_ok());
}

#[tauri::command]
fn get_shortcuts(hk: State<HotkeyState>) -> CmdResult<ShortcutsInfo> {
    let h = hk.0.lock().map_err(err)?;
    Ok(info_of(&h))
}

#[tauri::command]
fn set_shortcuts(
    app: AppHandle,
    db: State<Db>,
    hk: State<HotkeyState>,
    shortcuts: Shortcuts,
) -> CmdResult<ShortcutsInfo> {
    let main = parse_global(&shortcuts.show_main)?;
    let capture = parse_global(&shortcuts.capture)?;
    if main == capture {
        return Err("The two system-wide shortcuts must be different".into());
    }
    {
        let conn = db.0.lock().map_err(err)?;
        db::set_setting(&conn, "shortcuts", &serde_json::to_string(&shortcuts).map_err(err)?)
            .map_err(err)?;
    }
    let info = {
        let mut h = hk.0.lock().map_err(err)?;
        h.config = shortcuts;
        register_globals(&app, &mut h);
        info_of(&h)
    };
    let _ = app.emit("shortcuts-changed", &info);
    Ok(info)
}

/// Temporarily releases the system-wide shortcuts while the user records a new one,
/// so pressing the current combo doesn't trigger it.
#[tauri::command]
fn suspend_shortcuts(app: AppHandle, hk: State<HotkeyState>) -> CmdResult<()> {
    let mut h = hk.0.lock().map_err(err)?;
    unregister_globals(&app, &mut h);
    Ok(())
}

#[tauri::command]
fn resume_shortcuts(app: AppHandle, hk: State<HotkeyState>) -> CmdResult<ShortcutsInfo> {
    let mut h = hk.0.lock().map_err(err)?;
    register_globals(&app, &mut h);
    Ok(info_of(&h))
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

#[tauri::command]
fn update_entry(
    db: State<Db>,
    id: i64,
    title: String,
    body: String,
    tags: Vec<String>,
) -> CmdResult<Option<Entry>> {
    let conn = db.0.lock().map_err(err)?;
    db::update_entry(&conn, id, &title, &body, &tags).map_err(err)
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
fn restore_version(db: State<Db>, entry_id: i64, version_id: i64) -> CmdResult<Option<Entry>> {
    let conn = db.0.lock().map_err(err)?;
    db::restore_version(&conn, entry_id, version_id).map_err(err)
}

// ---------- import ----------

#[tauri::command]
async fn import_markdown(app: AppHandle, paths: Vec<String>) -> CmdResult<import::ImportResult> {
    import::import_paths(&app, &paths)
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
    let name = format!("{ms}-{n}.{ext}");
    std::fs::write(images_dir(app)?.join(&name), bytes).map_err(err)?;
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
    for (path, size) in files {
        if std::fs::remove_file(&path).is_ok() {
            done.count += 1;
            done.bytes += size;
        }
    }
    Ok(done)
}

// ---------- window commands ----------

#[tauri::command]
fn hide_capture(app: AppHandle) {
    if let Some(w) = app.get_webview_window(CAPTURE) {
        let _ = w.hide();
    }
}

/// Called by the capture popup after saving (or to jump to the full app).
#[tauri::command]
fn capture_done(app: AppHandle, open_id: Option<i64>, open_main: bool) {
    hide_capture(app.clone());
    let _ = app.emit_to(MAIN, "entries-changed", ());
    if open_main || open_id.is_some() {
        show_main(&app);
        if let Some(id) = open_id {
            let _ = app.emit_to(MAIN, "open-entry", id);
        }
    }
}

// ---------- app / settings commands ----------

// ---------- updates ----------

#[tauri::command]
async fn check_for_update(app: AppHandle) -> CmdResult<Option<updates::UpdateInfo>> {
    updates::check(&app).await
}

#[tauri::command]
async fn install_update(app: AppHandle) -> CmdResult<()> {
    updates::install(&app).await
}

// ---------- backups ----------

#[tauri::command]
fn backup_status(app: AppHandle) -> CmdResult<backup::BackupStatus> {
    backup::status(&app)
}

#[tauri::command]
fn set_backup_config(app: AppHandle, config: backup::BackupConfig) -> CmdResult<backup::BackupStatus> {
    if let Some(dir) = config.dir.as_deref().filter(|d| !d.trim().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("Can’t use that folder: {e}"))?;
    }
    backup::save_config(&app, &config)?;
    backup::status(&app)
}

// async → runs off the UI thread, since copying screenshots can take a moment.
#[tauri::command]
async fn backup_now(app: AppHandle) -> CmdResult<backup::BackupStatus> {
    backup::run_backup(&app, None)?;
    backup::status(&app)
}

#[tauri::command]
async fn restore_backup(app: AppHandle, name: String) -> CmdResult<backup::BackupStatus> {
    backup::restore(&app, &name)?;
    backup::status(&app)
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

#[tauri::command]
fn get_autostart(app: AppHandle) -> CmdResult<bool> {
    app.autolaunch().is_enabled().map_err(err)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<bool> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(err)?;
    } else {
        launcher.disable().map_err(err)?;
    }
    launcher.is_enabled().map_err(err)
}

/// Writes every entry as a Markdown file into Documents\BrainTracker Export\<timestamp>\,
/// with pasted images copied into an `images` subfolder.
#[tauri::command]
fn export_markdown(app: AppHandle, db: State<Db>) -> CmdResult<String> {
    let entries = {
        let conn = db.0.lock().map_err(err)?;
        db::all_for_export(&conn).map_err(err)?
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = app
        .path()
        .document_dir()
        .map_err(err)?
        .join("BrainTracker Export")
        .join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(err)?;
    let src_images = images_dir(&app)?;
    let out_images = dir.join("images");

    for (e, created, updated) in &entries {
        let title = if e.title.trim().is_empty() { "Untitled" } else { e.title.trim() };
        let slug: String = title
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == ' ' { c } else { '_' })
            .collect::<String>()
            .trim()
            .chars()
            .take(60)
            .collect();
        let file = dir.join(format!("{}-{}-{}.md", &created[..10], e.id, slug));
        let tags = if e.tags.is_empty() {
            String::new()
        } else {
            format!("Tags: {}\n", e.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "))
        };

        // Copy referenced images and point links at the exported copies.
        for part in e.body.split("](img:").skip(1) {
            let name: String = part.chars().take_while(|c| *c != ')').collect();
            if safe_image_name(&name) {
                std::fs::create_dir_all(&out_images).map_err(err)?;
                let _ = std::fs::copy(src_images.join(&name), out_images.join(&name));
            }
        }
        let body = e.body.replace("](img:", "](images/");

        let content = format!(
            "# {title}\n\n{tags}Created: {created} · Updated: {updated}\n\n---\n\n{body}\n"
        );
        std::fs::write(file, content).map_err(err)?;
    }
    Ok(dir.display().to_string())
}

// ---------- window helpers ----------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit_to(MAIN, "summoned", ());
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        let minimized = w.is_minimized().unwrap_or(false);
        if visible && focused && !minimized {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

fn toggle_capture(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(CAPTURE) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.center();
            let _ = w.show();
            let _ = w.set_focus();
            let _ = app.emit_to(CAPTURE, "capture-shown", ());
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Portable mode also keeps WebView2's cache beside the exe instead of in %LOCALAPPDATA%.
    if let Some(dir) = portable_dir() {
        let webview = dir.join("webview");
        let _ = std::fs::create_dir_all(&webview);
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", webview);
    }

    tauri::Builder::default()
        // Must be first: a second launch just focuses the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let Some(hk) = app.try_state::<HotkeyState>() else { return };
                    let (is_main, is_capture) = match hk.0.lock() {
                        Ok(h) => (h.main.as_ref() == Some(shortcut), h.capture.as_ref() == Some(shortcut)),
                        Err(_) => return,
                    };
                    if is_main {
                        toggle_main(app);
                    } else if is_capture {
                        toggle_capture(app);
                    }
                })
                .build(),
        )
        // Serves pasted images to the webview at http://btimg.localhost/<name>.
        .register_uri_scheme_protocol("btimg", |ctx, request| {
            let name = request.uri().path().trim_start_matches('/').to_string();
            let not_found = || {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap()
            };
            if !safe_image_name(&name) {
                return not_found();
            }
            let Ok(dir) = data_dir(ctx.app_handle()) else {
                return not_found();
            };
            match std::fs::read(dir.join("images").join(&name)) {
                Ok(bytes) => Response::builder()
                    .header("Content-Type", image_mime(&name))
                    .header("Cache-Control", "max-age=31536000, immutable")
                    .body(bytes)
                    .unwrap(),
                Err(_) => not_found(),
            }
        })
        .setup(|app| {
            // Database
            let dir = data_dir(app.handle())?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("braintracker.db"))?;

            // Turn on "run at startup" the first time the installed app runs.
            // (Skipped in dev builds so the debug exe never registers itself, and in
            // portable mode, which shouldn't touch the registry unless you ask it to.)
            if !cfg!(debug_assertions)
                && portable_dir().is_none()
                && db::get_setting(&conn, "autostart_initialized")?.is_none()
            {
                let _ = app.autolaunch().enable();
                db::set_setting(&conn, "autostart_initialized", "1")?;
            }
            let config: Shortcuts = db::get_setting(&conn, "shortcuts")?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            // Entries trashed more than 30 days ago are deleted for good.
            let _ = db::purge_expired_trash(&conn);
            app.manage(Db(Mutex::new(conn)));

            // Daily automatic backups
            app.manage(backup::BackupState::default());
            backup::start_scheduler(app.handle().clone());

            // Auto-update checks
            app.manage(updates::UpdateState::default());
            updates::start_checker(app.handle().clone());

            // System-wide shortcuts (one may fail if another app already owns it — that's fine).
            let mut hotkeys = Hotkeys { config, ..Default::default() };
            register_globals(app.handle(), &mut hotkeys);
            app.manage(HotkeyState(Mutex::new(hotkeys)));

            // System tray
            let capture_i = MenuItem::with_id(app, "capture", "Quick capture", true, None::<&str>)?;
            let open_i = MenuItem::with_id(app, "open", "Open BrainTracker", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&capture_i, &open_i, &quit_i])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("BrainTracker")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "capture" => toggle_capture(app),
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // Launched at login → stay quietly in the tray.
            if !std::env::args().any(|a| a == "--minimized") {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing any window hides it to the tray instead of quitting.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // The capture popup disappears when you click away (your text is kept).
            WindowEvent::Focused(false) if window.label() == CAPTURE => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            list_entries,
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
            import_markdown,
            find_unused_images,
            delete_unused_images,
            save_image,
            hide_capture,
            capture_done,
            get_shortcuts,
            set_shortcuts,
            suspend_shortcuts,
            resume_shortcuts,
            backup_status,
            set_backup_config,
            backup_now,
            restore_backup,
            check_for_update,
            install_update,
            app_info,
            get_autostart,
            set_autostart,
            export_markdown
        ])
        .run(tauri::generate_context!())
        .expect("error while running BrainTracker");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_formats_parse() {
        for ok in ["Ctrl+Shift+Space", "Ctrl+Shift+E", "Ctrl+Alt+B", "Super+Shift+1", "Ctrl+F5", "Ctrl+Shift+,", "Alt+Shift+/", "Ctrl+Shift+["] {
            assert!(parse_global(ok).is_ok(), "{ok} should parse");
        }
        assert_ne!(parse_global("Ctrl+Shift+Space").unwrap(), parse_global("Ctrl+Shift+E").unwrap());
    }
}
