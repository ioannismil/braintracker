//! The desktop app (Windows first): tray icon, system-wide shortcuts, the quick-capture and
//! quick-search popups, pop-out notes, backups, the inbox folder and auto-update. The phone
//! app (in lib.rs) shares the note commands in lib.rs but none of this.

use crate::db::{self, Db};
use crate::{backup, cli, err, images_dir, import, inbox, ocr, portable_dir, safe_image_name, selection, updates, CmdResult};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const MAIN: &str = "main";
const CAPTURE: &str = "capture";
const SPOTLIGHT: &str = "spotlight";
/// Pop-out note windows are labelled `note-<id>`.
const NOTE_PREFIX: &str = "note-";

// ---------- shortcuts ----------

/// Every configurable shortcut, stored as "Ctrl+Shift+E"-style strings ("" = not set).
/// The `Global` ones work system-wide; `capture_*` only work in the quick-capture
/// popup; the rest only work in the main window.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Shortcuts {
    show_main: String,
    capture: String,
    capture_selection: String,
    spotlight: String,
    spotlight_selection: String,
    new_entry: String,
    search: String,
    preview: String,
    settings: String,
    capture_save: String,
    capture_save_close: String,
    capture_notes: String,
    capture_open_app: String,
    capture_hide: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            show_main: "Ctrl+Shift+Space".into(),
            capture: "Ctrl+Shift+E".into(),
            capture_selection: "Ctrl+Shift+D".into(),
            spotlight: "Ctrl+Alt+Space".into(),
            spotlight_selection: "Ctrl+Alt+Shift+Space".into(),
            new_entry: "Ctrl+T".into(),
            search: "Ctrl+K".into(),
            preview: "Ctrl+E".into(),
            settings: "Ctrl+,".into(),
            capture_save: "Enter".into(),
            capture_save_close: "Ctrl+Shift+Enter".into(),
            capture_notes: "Shift+Enter".into(),
            capture_open_app: "Ctrl+O".into(),
            capture_hide: "Escape".into(),
        }
    }
}

/// Loads the saved shortcuts on top of the defaults.
///
/// Whatever the user saved always wins, so updating the app never resets their choices,
/// even if the saved settings are partly unreadable (each shortcut is read on its own).
/// A shortcut that's new in this version gets its default only if the user hasn't already
/// put that key combo on something else — otherwise it starts unset, for them to pick.
fn load_shortcuts(saved: Option<&str>) -> Shortcuts {
    use serde_json::Value;
    let defaults = Shortcuts::default();
    let Some(saved) = saved.and_then(|s| serde_json::from_str::<serde_json::Map<String, Value>>(s).ok()) else {
        return defaults;
    };
    let Ok(Value::Object(mut merged)) = serde_json::to_value(&defaults) else {
        return defaults;
    };
    let used: std::collections::HashSet<&str> = saved
        .iter()
        .filter(|(k, _)| merged.contains_key(k.as_str()))
        .filter_map(|(_, v)| v.as_str())
        .collect();
    for (key, value) in merged.iter_mut() {
        match saved.get(key).and_then(Value::as_str) {
            Some(mine) => *value = Value::String(mine.to_string()),
            None => {
                if value.as_str().is_some_and(|d| used.contains(d)) {
                    *value = Value::String(String::new());
                }
            }
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or(defaults)
}

/// The system-wide shortcuts.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Global {
    ShowMain,
    Capture,
    CaptureSelection,
    Spotlight,
    SpotlightSelection,
}

impl Global {
    const ALL: [Global; 5] = [
        Global::ShowMain,
        Global::Capture,
        Global::CaptureSelection,
        Global::Spotlight,
        Global::SpotlightSelection,
    ];

    /// Matches the field name the UI uses.
    fn id(self) -> &'static str {
        match self {
            Global::ShowMain => "showMain",
            Global::Capture => "capture",
            Global::CaptureSelection => "captureSelection",
            Global::Spotlight => "spotlight",
            Global::SpotlightSelection => "spotlightSelection",
        }
    }

    fn combo(self, s: &Shortcuts) -> &str {
        match self {
            Global::ShowMain => &s.show_main,
            Global::Capture => &s.capture,
            Global::CaptureSelection => &s.capture_selection,
            Global::Spotlight => &s.spotlight,
            Global::SpotlightSelection => &s.spotlight_selection,
        }
    }
}

#[derive(Default)]
struct Hotkeys {
    config: Shortcuts,
    /// The system-wide shortcuts that are currently registered with Windows.
    registered: Vec<(Global, Shortcut)>,
}

struct HotkeyState(Mutex<Hotkeys>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutsInfo {
    shortcuts: Shortcuts,
    /// Ids of the system-wide shortcuts that are working (one is missing if another app owns its keys).
    registered: Vec<&'static str>,
}

fn info_of(h: &Hotkeys) -> ShortcutsInfo {
    ShortcutsInfo {
        shortcuts: h.config.clone(),
        registered: h.registered.iter().map(|(g, _)| g.id()).collect(),
    }
}

fn parse_global(combo: &str) -> Result<Shortcut, String> {
    combo
        .parse::<Shortcut>()
        .map_err(|_| format!("“{combo}” can’t be used as a system-wide shortcut"))
}

fn unregister_globals(app: &AppHandle, h: &mut Hotkeys) {
    let gs = app.global_shortcut();
    for (_, s) in h.registered.drain(..) {
        let _ = gs.unregister(s);
    }
}

/// (Re)registers the system-wide shortcuts. One may fail if another app already owns it.
fn register_globals(app: &AppHandle, h: &mut Hotkeys) {
    unregister_globals(app, h);
    let gs = app.global_shortcut();
    for g in Global::ALL {
        let combo = g.combo(&h.config);
        if combo.is_empty() {
            continue;
        }
        if let Ok(s) = parse_global(combo) {
            if gs.register(s).is_ok() {
                h.registered.push((g, s));
            }
        }
    }
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
    let mut seen = Vec::new();
    for g in Global::ALL {
        let combo = g.combo(&shortcuts);
        if combo.is_empty() {
            continue;
        }
        let s = parse_global(combo)?;
        if seen.contains(&s) {
            return Err("The system-wide shortcuts must all be different".into());
        }
        seen.push(s);
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

/// Tells every window when another program (the `bt` command line) changed the notes,
/// so open notes and lists reload instead of showing — and later saving over — old text.
fn watch_for_outside_changes(app: AppHandle) {
    std::thread::spawn(move || {
        let version = |app: &AppHandle| app.try_state::<Db>()?.0.lock().ok().and_then(|c| db::data_version(&c).ok());
        let mut last = version(&app);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let now = version(&app);
            if now.is_some() && now != last {
                last = now;
                let _ = app.emit("outside-change", ());
            }
        }
    });
}

// ---------- import ----------

#[tauri::command]
async fn import_markdown(app: AppHandle, paths: Vec<String>) -> CmdResult<import::ImportResult> {
    import::import_paths(&app, &paths)
}

// ---------- window commands ----------

#[tauri::command]
fn hide_capture(app: AppHandle) {
    if let Some(w) = app.get_webview_window(CAPTURE) {
        let _ = w.hide();
    }
}

/// Called by the capture popup after a save that keeps the popup open for the next entry.
#[tauri::command]
fn capture_saved(app: AppHandle) {
    let _ = app.emit_to(MAIN, "entries-changed", ());
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

#[tauri::command]
fn hide_spotlight(app: AppHandle) {
    if let Some(w) = app.get_webview_window(SPOTLIGHT) {
        let _ = w.hide();
    }
}

/// Called by quick search to jump to an entry (or just the app) in the main window.
#[tauri::command]
fn spotlight_open(app: AppHandle, open_id: Option<i64>) {
    hide_spotlight(app.clone());
    show_main(&app);
    if let Some(id) = open_id {
        let _ = app.emit_to(MAIN, "open-entry", id);
    }
}

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
    ocr::scan(&app); // restored screenshots that haven't been read yet
    backup::status(&app)
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

// ---------- inbox folder ----------

#[tauri::command]
fn inbox_status(app: AppHandle) -> CmdResult<inbox::InboxStatus> {
    inbox::status(&app)
}

#[tauri::command]
fn set_inbox_config(app: AppHandle, config: inbox::InboxConfig) -> CmdResult<inbox::InboxStatus> {
    inbox::save_config(&app, &config)?;
    inbox::status(&app)
}

// async → off the UI thread: importing can copy images.
#[tauri::command]
async fn inbox_check_now(app: AppHandle) -> CmdResult<inbox::InboxStatus> {
    let _ = inbox::check_now(&app); // the outcome is in the status
    inbox::status(&app)
}

#[tauri::command]
fn inbox_dir_path(app: AppHandle) -> CmdResult<String> {
    let dir = inbox::inbox_dir(&app, &inbox::load_config(&app))?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir.display().to_string())
}

// ---------- pop-out notes ----------

/// Opens a note in its own small window that stays on top of other apps, e.g. to follow
/// the steps while you work. One window per note; asking again brings it to the front.
/// (async: creating a window from a synchronous command can deadlock on Windows.)
#[tauri::command]
async fn pop_out(app: AppHandle, id: i64) -> CmdResult<()> {
    let label = format!("{NOTE_PREFIX}{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let title = {
        let db = app.state::<Db>();
        let conn = db.0.lock().map_err(err)?;
        db::get_entry(&conn, id).map_err(err)?.ok_or("That note no longer exists")?.title
    };
    let (w, h) = (440.0, 560.0);
    let mut builder = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(format!("note.html?id={id}").into()))
        .title(if title.trim().is_empty() { "Untitled" } else { title.trim() })
        .inner_size(w, h)
        .min_inner_size(280.0, 180.0)
        .always_on_top(true)
        .focused(true);
    // Top-right of the main window's screen, each further pop-out a little lower-left.
    let monitor = app.get_webview_window(MAIN).and_then(|m| m.current_monitor().ok().flatten());
    if let Some(m) = monitor.or_else(|| app.primary_monitor().ok().flatten()) {
        let scale = m.scale_factor();
        let (pos, size) = (m.position().to_logical::<f64>(scale), m.size().to_logical::<f64>(scale));
        let others = app.webview_windows().keys().filter(|l| l.starts_with(NOTE_PREFIX)).count() as f64;
        let x = pos.x + size.width - w - 32.0 - others * 28.0;
        let y = pos.y + 64.0 + others * 28.0;
        builder = builder.position(x.max(pos.x), y);
    }
    builder.build().map_err(err)?;
    Ok(())
}

/// From a popped-out note: open it for editing in the main window.
#[tauri::command]
fn open_in_main(app: AppHandle, id: i64) {
    show_main(&app);
    let _ = app.emit_to(MAIN, "open-entry", id);
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

/// Things waiting for a window to pick up: "selection" (text for quick capture's notes),
/// "search" (a query for quick search) and "open" (an entry id for the main window).
/// Held here rather than only sent in an event so nothing is lost while a window is still
/// loading, e.g. on a first launch with `bt add --open --selection`.
#[derive(Default)]
struct Pending(Mutex<std::collections::HashMap<&'static str, String>>);

fn set_pending(app: &AppHandle, key: &'static str, value: String) {
    if let Ok(mut p) = app.state::<Pending>().0.lock() {
        p.insert(key, value);
    }
}

/// Windows call this when shown (and when they first load) to collect what's waiting for them.
#[tauri::command]
fn take_pending(pending: State<Pending>, key: String) -> Option<String> {
    pending.0.lock().ok()?.remove(key.as_str())
}

/// Shows the quick-capture popup; `selection` is text to add to the end of its notes.
fn show_capture(app: &AppHandle, selection: Option<String>) {
    if let Some(text) = selection {
        set_pending(app, "selection", text);
    }
    if let Some(w) = app.get_webview_window(CAPTURE) {
        if !w.is_visible().unwrap_or(false) {
            let _ = w.center();
        }
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit_to(CAPTURE, "capture-shown", ());
    }
}

/// Does what `--open` on the command line asked for (see `cli.rs`).
fn run_gui_action(app: &AppHandle, action: cli::GuiAction) {
    match action {
        cli::GuiAction::Show => show_main(app),
        cli::GuiAction::OpenEntry(id) => {
            set_pending(app, "open", id.to_string());
            show_main(app);
            let _ = app.emit_to(MAIN, "pending-open", ());
        }
        cli::GuiAction::Search(query) => {
            if let Some(q) = query {
                set_pending(app, "search", q);
            }
            show_spotlight(app);
        }
        cli::GuiAction::Capture => show_capture(app, None),
        cli::GuiAction::CaptureSelection => capture_selection(app),
        cli::GuiAction::SearchSelection => search_selection(app),
    }
}

fn toggle_capture(app: &AppHandle) {
    match app.get_webview_window(CAPTURE) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => show_capture(app, None),
    }
}

/// Copies the text selected in the app in front, then opens quick capture with it in the notes.
/// Runs on its own thread: it waits for the shortcut's keys to be released and for the copy.
fn capture_selection(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let text = selection::copy_selection();
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || show_capture(&handle, text));
    });
}

/// Copies the text selected in the app in front, then opens quick search looking for it, so
/// an error message you're staring at finds the note about it. With nothing selected it just
/// opens quick search.
fn search_selection(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let query = selection::copy_selection().map(|t| selection_query(&t)).filter(|q| !q.is_empty());
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(q) = query {
                set_pending(&handle, "search", q);
            }
            show_spotlight(&handle);
        });
    });
}

/// Selected text as a search: one line, at most ~150 characters (whole words), so a pasted
/// stack trace or log excerpt still makes a usable query.
fn selection_query(text: &str) -> String {
    const MAX: usize = 150;
    let mut out = String::new();
    for word in text.split_whitespace() {
        if out.is_empty() {
            // Even a single enormous "word" (e.g. a minified line) is cut to size.
            out = word.chars().take(MAX).collect();
        } else if out.len() + 1 + word.len() <= MAX {
            out.push(' ');
            out.push_str(word);
        } else {
            break;
        }
    }
    out
}

fn toggle_spotlight(app: &AppHandle) {
    match app.get_webview_window(SPOTLIGHT) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => show_spotlight(app),
    }
}

/// Shows quick search centred horizontally, a little above the middle of the screen
/// (like Spotlight), so the bar stays put while the results grow below it.
fn show_spotlight(app: &AppHandle) {
    let Some(w) = app.get_webview_window(SPOTLIGHT) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.set_focus();
        let _ = app.emit_to(SPOTLIGHT, "spotlight-shown", ());
        return;
    }
    let _ = w.center();
    if let (Ok(Some(m)), Ok(size)) = (w.current_monitor(), w.outer_size()) {
        let (mp, ms) = (m.position(), m.size());
        let x = mp.x + (ms.width as i32 - size.width as i32) / 2;
        let y = mp.y + (ms.height as f64 * 0.2) as i32;
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to(SPOTLIGHT, "spotlight-shown", ());
}

/// Runs the desktop app. The database is opened before the app starts, not in setup(): Tauri
/// creates the windows before setup() runs, and a window can ask for its data (e.g. "is offline
/// mode on?") as soon as it loads. Opening it first means it's there for the very first request.
pub fn run(gui_action: Option<cli::GuiAction>, db: Db) {
    tauri::Builder::default()
        .manage(db)
        // Must be first: a second launch hands its command-line options to the running
        // instance (or, with none, just brings it to the front) and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match cli::parse(&args) {
                cli::Parsed::Gui(Some(action)) => run_gui_action(app, action),
                _ => show_main(app),
            }
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
                    let hit = match hk.0.lock() {
                        Ok(h) => h.registered.iter().find(|(_, s)| s == shortcut).map(|(g, _)| *g),
                        Err(_) => return,
                    };
                    match hit {
                        Some(Global::ShowMain) => toggle_main(app),
                        Some(Global::Capture) => toggle_capture(app),
                        Some(Global::CaptureSelection) => capture_selection(app),
                        Some(Global::Spotlight) => toggle_spotlight(app),
                        Some(Global::SpotlightSelection) => search_selection(app),
                        None => {}
                    }
                })
                .build(),
        )
        // Serves pasted images to the webview at http://btimg.localhost/<name>.
        .register_uri_scheme_protocol("btimg", crate::serve_image)
        .setup(|app| {
            let db = app.state::<Db>();
            let conn = db.0.lock().map_err(|e| e.to_string())?;

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
            let config = load_shortcuts(db::get_setting(&conn, "shortcuts")?.as_deref());
            // Entries trashed more than 30 days ago are deleted for good.
            let _ = db::purge_expired_trash(&conn);
            drop(conn);

            // Reads the text in screenshots (in the background) so search finds it.
            ocr::start(app.handle());

            // Notes changed from the `bt` command line show up in open windows.
            watch_for_outside_changes(app.handle().clone());

            // Notes dropped into the inbox folder are imported automatically.
            inbox::start(app.handle().clone());

            // Phones paired for sync reach this PC over Wi-Fi (sync/server.rs).
            crate::sync::server::start(app.handle().clone());

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
            app.manage(Pending::default());

            // System tray
            let capture_i = MenuItem::with_id(app, "capture", "Quick capture", true, None::<&str>)?;
            let search_i = MenuItem::with_id(app, "search", "Quick search", true, None::<&str>)?;
            let open_i = MenuItem::with_id(app, "open", "Open BrainTracker", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&capture_i, &search_i, &open_i, &quit_i])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("BrainTracker")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "capture" => toggle_capture(app),
                    "search" => toggle_spotlight(app),
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

            // `bt search --open` etc. opens just that; launched at login → stay quietly in the tray.
            if let Some(action) = gui_action {
                run_gui_action(app.handle(), action);
            } else if !std::env::args().any(|a| a == "--minimized") {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the app's own windows hides them to the tray instead of quitting
            // (popped-out notes really close).
            WindowEvent::CloseRequested { api, .. } if !window.label().starts_with(NOTE_PREFIX) => {
                api.prevent_close();
                let _ = window.hide();
            }
            // The popups disappear when you click away (your text is kept).
            WindowEvent::Focused(false) if window.label() == CAPTURE || window.label() == SPOTLIGHT => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            crate::list_entries,
            crate::search_any,
            crate::get_entry,
            crate::create_entry,
            crate::update_entry,
            crate::set_pinned,
            crate::delete_entry,
            crate::discard_entry,
            crate::list_tags,
            crate::list_trash,
            crate::trash_count,
            crate::restore_entry,
            crate::purge_entry,
            crate::empty_trash,
            crate::list_versions,
            crate::get_version,
            crate::restore_version,
            import_markdown,
            crate::find_unused_images,
            crate::delete_unused_images,
            crate::save_image,
            hide_capture,
            capture_done,
            capture_saved,
            hide_spotlight,
            take_pending,
            spotlight_open,
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
            crate::app_info,
            crate::get_offline,
            crate::set_offline,
            crate::get_placeholders,
            crate::set_placeholders,
            crate::remember_placeholders,
            inbox_status,
            set_inbox_config,
            inbox_check_now,
            inbox_dir_path,
            pop_out,
            open_in_main,
            get_autostart,
            set_autostart,
            export_markdown,
            crate::sync::commands::sync_status,
            crate::sync::commands::set_sync_enabled,
            crate::sync::commands::start_pairing,
            crate::sync::commands::cancel_pairing,
            crate::sync::commands::remove_peer
        ])
        .run(tauri::generate_context!())
        .expect("error while running BrainTracker");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_formats_parse() {
        for ok in ["Ctrl+Shift+Space", "Ctrl+Shift+E", "Ctrl+Alt+Space", "Ctrl+Alt+B", "Super+Shift+1", "Ctrl+F5", "Ctrl+Shift+,", "Alt+Shift+/", "Ctrl+Shift+["] {
            assert!(parse_global(ok).is_ok(), "{ok} should parse");
        }
        assert_ne!(parse_global("Ctrl+Shift+Space").unwrap(), parse_global("Ctrl+Shift+E").unwrap());
        let d = Shortcuts::default();
        for g in Global::ALL {
            assert!(parse_global(g.combo(&d)).is_ok(), "default for {} should parse", g.id());
        }
    }

    #[test]
    fn never_saved_uses_defaults() {
        assert_eq!(load_shortcuts(None), Shortcuts::default());
        assert_eq!(load_shortcuts(Some("not json")), Shortcuts::default());
    }

    #[test]
    fn saved_shortcuts_survive_updates() {
        // Saved by an older version: no quick search / capture selection yet, and one customised key.
        let old = r#"{"showMain":"Ctrl+Alt+B","capture":"Ctrl+Shift+E","newEntry":"Ctrl+N","search":"Ctrl+K",
            "preview":"Ctrl+E","settings":"Ctrl+,","captureSave":"Enter","captureSaveClose":"Ctrl+Shift+Enter",
            "captureNotes":"Shift+Enter","captureOpenApp":"Ctrl+O","captureHide":"Escape"}"#;
        let s = load_shortcuts(Some(old));
        assert_eq!(s.show_main, "Ctrl+Alt+B");
        assert_eq!(s.new_entry, "Ctrl+N");
        // New shortcuts get their defaults.
        assert_eq!(s.spotlight, "Ctrl+Alt+Space");
        assert_eq!(s.capture_selection, "Ctrl+Shift+D");
        assert_eq!(s.spotlight_selection, "Ctrl+Alt+Shift+Space");
    }

    #[test]
    fn selection_becomes_a_one_line_query() {
        assert_eq!(selection_query("  Error 0x80070005:
  Access is denied.
"), "Error 0x80070005: Access is denied.");
        let long = "word ".repeat(100);
        let q = selection_query(&long);
        assert!(q.len() <= 150 && q.ends_with("word"));
        assert_eq!(selection_query(&"x".repeat(400)).len(), 150);
    }

    #[test]
    fn new_shortcut_never_steals_a_combo_the_user_picked() {
        let s = load_shortcuts(Some(r#"{"showMain":"Ctrl+Alt+Space","newEntry":"Ctrl+Shift+D"}"#));
        assert_eq!(s.show_main, "Ctrl+Alt+Space");
        assert_eq!(s.spotlight, "", "quick search starts unset rather than clashing");
        assert_eq!(s.capture_selection, "");
        assert_eq!(s.capture, "Ctrl+Shift+E");
    }

    #[test]
    fn one_bad_value_keeps_the_rest() {
        let s = load_shortcuts(Some(r#"{"showMain":"Ctrl+Alt+B","capture":42}"#));
        assert_eq!(s.show_main, "Ctrl+Alt+B");
        assert_eq!(s.capture, "Ctrl+Shift+E");
    }
}
