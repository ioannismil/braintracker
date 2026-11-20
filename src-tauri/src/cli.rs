//! The command line. Every command works straight on your notes from the terminal — no window
//! opens — so it can be scripted. Add `--open` to do the same thing in the app instead
//! (quick search, quick capture, or a note in the main window).
//!
//! On Windows the installed `braintracker.exe` is a windowed program, which terminals don't wait
//! for, so use the console companion `bt.exe` installed beside it. Elsewhere either name works.

use crate::db::{self, Entry, EntrySummary};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

pub const HELP: &str = "\
BrainTracker — remember what you did and how you did it.

Usage: bt <command> [options]       (on Linux/macOS, `braintracker` works too)

Commands (they work on your notes right here in the terminal):
  list                    Recent notes, newest first (pinned on top)
  search <words…>         Find notes; filters work too: tag:docker has:image after:2026-10
  show <id>               Print a note in full
  add <title>             Add a note
  edit <id>               Change a note (with --title, --body, --append, --tags)
  pin <id> / unpin <id>   Pin a note to the top of the list, or unpin it
  delete <id>             Move a note to the trash (kept for 30 days)
  tags                    All tags, with how many notes use each

Options:
  --body <text>           The note's text (Markdown). Use - to read it from stdin
  --append <text>         With edit: add text to the end of the note (- for stdin)
  --title <text>          With edit: a new title
  --tags <a,b>            Comma-separated tags (with edit, replaces them)
  --selection             With add: put the text selected in the app in front into the note;
                          with search --open: search for the selected text
  --tag <tag>             With list: only notes with this tag
  --limit <n>             With list/search: at most n notes (list defaults to 20)
  --json                  Machine-readable output
  --open                  Do it in the app instead:
                            bt --open                 show BrainTracker
                            bt search [words] --open  quick search
                            bt search --open --selection
                                                      quick search for the selected text
                            bt add --open             quick capture
                            bt add --open --selection quick capture with the selected text
                            bt show <id> --open       open the note in BrainTracker
  --minimized             Start BrainTracker quietly in the system tray
  -h, --help              Print this help
  -V, --version           Print the version

Examples:
  bt search docker dns
  bt add \"Fixed DNS in WSL\" --tags wsl,network --body \"Set nameserver 1.1.1.1 in /etc/resolv.conf\"
  git log -1 | bt add \"Today's release\" --body -
";

/// Something to do in the app's windows (needs the app running).
#[derive(Clone, Debug, PartialEq)]
pub enum GuiAction {
    Show,
    OpenEntry(i64),
    Search(Option<String>),
    Capture,
    CaptureSelection,
    SearchSelection,
}

/// Something to do straight on the database, printing to the terminal.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    List { tag: Option<String>, limit: usize },
    Search { query: String, limit: Option<usize> },
    Show { id: i64 },
    Add { title: String, body: Option<String>, tags: Vec<String>, selection: bool },
    Edit { id: i64, title: Option<String>, body: Option<String>, append: Option<String>, tags: Option<Vec<String>> },
    Pin { id: i64, pinned: bool },
    Delete { id: i64 },
    Tags,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    /// Print this (help / version) and exit.
    Info(String),
    /// Run in the terminal; `bool` = JSON output.
    Headless(Cmd, bool),
    /// Start / talk to the app. `None` = a plain launch.
    Gui(Option<GuiAction>),
    /// Bad usage: print the message and exit with code 2.
    Error(String),
}

#[derive(Default)]
struct Opts {
    json: bool,
    open: bool,
    selection: bool,
    limit: Option<usize>,
    tag: Option<String>,
    tags: Option<String>,
    title: Option<String>,
    body: Option<String>,
    append: Option<String>,
}

/// `args` is the full argument list, program name first.
pub fn parse(args: &[String]) -> Parsed {
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let mut o = Opts::default();
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i];
        i += 1;
        if arg == "-" || !arg.starts_with('-') {
            pos.push(arg);
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg, None),
        };
        let mut value = || -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            let v = rest.get(i).ok_or_else(|| format!("{name} needs a value"))?;
            i += 1;
            Ok(v.to_string())
        };
        let result: Result<(), String> = (|| {
            match name {
                "-h" | "--help" => return Err("\u{0}help".into()),
                "-V" | "--version" => return Err("\u{0}version".into()),
                "--json" => o.json = true,
                "--open" => o.open = true,
                "--selection" => o.selection = true,
                "--minimized" => {}
                "--limit" => {
                    let v = value()?;
                    o.limit = Some(v.parse().map_err(|_| format!("--limit needs a number, not “{v}”"))?);
                }
                "--tag" => o.tag = Some(value()?),
                "--tags" => o.tags = Some(value()?),
                "--title" => o.title = Some(value()?),
                "--body" => o.body = Some(value()?),
                "--append" => o.append = Some(value()?),
                _ => return Err(format!("unknown option {name} (see bt --help)")),
            }
            Ok(())
        })();
        match result {
            Ok(()) => {}
            Err(e) if e == "\u{0}help" => return Parsed::Info(HELP.to_string()),
            Err(e) if e == "\u{0}version" => return Parsed::Info(format!("BrainTracker {}\n", env!("CARGO_PKG_VERSION"))),
            Err(e) => return Parsed::Error(e),
        }
    }

    let words = |from: usize| pos.get(from..).unwrap_or_default().join(" ");
    let id = |what: &str| -> Result<i64, Parsed> {
        let v = pos.get(1).ok_or_else(|| Parsed::Error(format!("{what} needs a note id, e.g. bt {what} 12")))?;
        v.trim_start_matches('#')
            .parse()
            .map_err(|_| Parsed::Error(format!("“{v}” isn’t a note id (ids are numbers — see bt list)")))
    };
    let tags = |t: &Option<String>| t.as_deref().map(crate::db::parse_tags_arg);
    let json = o.json;

    let Some(&command) = pos.first() else {
        return Parsed::Gui(o.open.then_some(GuiAction::Show));
    };
    let cmd = match command {
        "list" | "ls" => Cmd::List { tag: o.tag, limit: o.limit.unwrap_or(20) },
        "search" | "find" => {
            let query = words(1);
            if o.open && o.selection {
                return Parsed::Gui(Some(GuiAction::SearchSelection));
            }
            if o.open {
                return Parsed::Gui(Some(GuiAction::Search(Some(query).filter(|q| !q.trim().is_empty()))));
            }
            if query.trim().is_empty() {
                return Parsed::Error("search needs something to look for, e.g. bt search docker".into());
            }
            Cmd::Search { query, limit: o.limit }
        }
        "show" | "cat" => {
            let id = match id("show") {
                Ok(id) => id,
                Err(e) => return e,
            };
            if o.open {
                return Parsed::Gui(Some(GuiAction::OpenEntry(id)));
            }
            Cmd::Show { id }
        }
        "add" | "new" => {
            if o.open {
                return Parsed::Gui(Some(if o.selection { GuiAction::CaptureSelection } else { GuiAction::Capture }));
            }
            let title = o.title.clone().unwrap_or_else(|| words(1));
            if title.trim().is_empty() && o.body.is_none() && !o.selection {
                return Parsed::Error("add needs a title, e.g. bt add \"Fixed the printer\"".into());
            }
            Cmd::Add { title, body: o.body, tags: tags(&o.tags).unwrap_or_default(), selection: o.selection }
        }
        "edit" => {
            let id = match id("edit") {
                Ok(id) => id,
                Err(e) => return e,
            };
            if o.title.is_none() && o.body.is_none() && o.append.is_none() && o.tags.is_none() {
                return Parsed::Error("edit needs --title, --body, --append or --tags".into());
            }
            Cmd::Edit { id, title: o.title, body: o.body, append: o.append, tags: tags(&o.tags) }
        }
        "pin" | "unpin" => match id(command) {
            Ok(id) => Cmd::Pin { id, pinned: command == "pin" },
            Err(e) => return e,
        },
        "delete" | "rm" | "trash" => match id("delete") {
            Ok(id) => Cmd::Delete { id },
            Err(e) => return e,
        },
        "tags" => Cmd::Tags,
        other => return Parsed::Error(format!("unknown command “{other}” (see bt --help)")),
    };
    Parsed::Headless(cmd, json)
}

// ---------- running commands ----------

fn db_path() -> Result<PathBuf, String> {
    let dir = crate::standalone_data_dir().ok_or("can’t find BrainTracker’s data folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("can’t create {}: {e}", dir.display()))?;
    Ok(dir.join("braintracker.db"))
}

/// Runs a terminal command and returns the process exit code.
pub fn execute(cmd: Cmd, json: bool) -> i32 {
    match run(cmd, json) {
        Ok(()) => 0,
        Err(e) => {
            print_err(&e);
            1
        }
    }
}

fn stdin_if_dash(v: Option<String>) -> Result<Option<String>, String> {
    match v.as_deref() {
        Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| format!("couldn’t read stdin: {e}"))?;
            Ok(Some(s.trim_end().to_string()))
        }
        _ => Ok(v),
    }
}

fn not_found(id: i64) -> String {
    format!("there’s no note #{id} (see bt list)")
}

/// A live (not trashed) note.
fn live_entry(conn: &Connection, id: i64) -> Result<Entry, String> {
    let trashed: Option<bool> = conn
        .query_row("SELECT deleted_at IS NOT NULL FROM entries WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match trashed {
        None => Err(not_found(id)),
        Some(true) => Err(format!("note #{id} is in the trash (restore it in BrainTracker → Trash)")),
        Some(false) => db::get_entry(conn, id).map_err(|e| e.to_string())?.ok_or_else(|| not_found(id)),
    }
}

fn run(cmd: Cmd, json: bool) -> Result<(), String> {
    let conn = db::open(&db_path()?).map_err(|e| format!("can’t open your notes: {e}"))?;
    let e = |e: rusqlite::Error| e.to_string();
    let out = Out::new();
    match cmd {
        Cmd::List { tag, limit } => {
            let mut rows = db::list_entries(&conn, None, tag.as_deref()).map_err(e)?;
            rows.truncate(limit);
            out.summaries(&conn, &rows, json, None)
        }
        Cmd::Search { query, limit } => {
            let mut rows = db::list_entries(&conn, Some(&query), None).map_err(e)?;
            if let Some(n) = limit {
                rows.truncate(n);
            }
            out.summaries(&conn, &rows, json, Some(&query))
        }
        Cmd::Show { id } => {
            let entry = live_entry(&conn, id)?;
            out.entry(&conn, &entry, json)
        }
        Cmd::Add { title, body, tags, selection } => {
            let mut body = stdin_if_dash(body)?.unwrap_or_default();
            if selection {
                match crate::selection::copy_selection() {
                    Some(text) => body = if body.trim().is_empty() { text } else { format!("{}\n\n{text}", body.trim_end()) },
                    None => return Err("no text is selected in the app in front".into()),
                }
            }
            let title = match title.trim() {
                "" => body.lines().find(|l| !l.trim().is_empty()).unwrap_or("Untitled").trim().chars().take(80).collect(),
                t => t.to_string(),
            };
            let entry = db::create_entry(&conn, &title, &body, &tags).map_err(e)?;
            if json {
                out.json(&JsonEntry::from(&conn, &entry));
            } else {
                out.line(&format!("Added #{} {}", entry.id, out.bold(&entry.title)));
            }
            Ok(())
        }
        Cmd::Edit { id, title, body, append, tags } => {
            let entry = live_entry(&conn, id)?;
            let mut new_body = stdin_if_dash(body)?.unwrap_or(entry.body.clone());
            if let Some(more) = stdin_if_dash(append)? {
                new_body = if new_body.trim().is_empty() { more } else { format!("{}\n\n{more}", new_body.trim_end()) };
            }
            let updated = db::update_entry(
                &conn,
                id,
                title.as_deref().unwrap_or(&entry.title),
                &new_body,
                tags.as_ref().unwrap_or(&entry.tags),
            )
            .map_err(e)?
            .ok_or_else(|| not_found(id))?;
            if json {
                out.json(&JsonEntry::from(&conn, &updated));
            } else {
                out.line(&format!("Updated #{} {}", updated.id, out.bold(&updated.title)));
            }
            Ok(())
        }
        Cmd::Pin { id, pinned } => {
            let entry = live_entry(&conn, id)?;
            db::set_pinned(&conn, id, pinned).map_err(e)?;
            if !json {
                out.line(&format!("{} #{id} {}", if pinned { "Pinned" } else { "Unpinned" }, out.bold(&entry.title)));
            }
            Ok(())
        }
        Cmd::Delete { id } => {
            let entry = live_entry(&conn, id)?;
            db::trash_entry(&conn, id).map_err(e)?;
            if !json {
                out.line(&format!("Moved #{id} {} to the trash", out.bold(&entry.title)));
            }
            Ok(())
        }
        Cmd::Tags => {
            let tags = db::list_tags(&conn).map_err(e)?;
            if json {
                out.json(&tags);
            } else if tags.is_empty() {
                out.line("No tags yet.");
            } else {
                let width = tags.iter().map(|t| t.tag.chars().count()).max().unwrap_or(0) + 1;
                for t in &tags {
                    out.line(&format!("{:<width$}  {}", format!("#{}", t.tag), t.count));
                }
            }
            Ok(())
        }
    }
}

// ---------- output ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonEntry<'a> {
    id: i64,
    title: &'a str,
    body: &'a str,
    tags: &'a [String],
    pinned: bool,
    created: String,
    updated: String,
}

impl<'a> JsonEntry<'a> {
    fn from(conn: &Connection, e: &'a Entry) -> Self {
        Self {
            id: e.id,
            title: &e.title,
            body: &e.body,
            tags: &e.tags,
            pinned: e.pinned,
            created: iso(conn, e.created_at),
            updated: iso(conn, e.updated_at),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSummary<'a> {
    id: i64,
    title: &'a str,
    snippet: String,
    tags: &'a [String],
    pinned: bool,
    created: String,
    updated: String,
}

fn local_time(conn: &Connection, ms: i64, fmt: &str) -> String {
    conn.query_row("SELECT strftime(?2, ?1 / 1000, 'unixepoch', 'localtime')", rusqlite::params![ms, fmt], |r| r.get(0))
        .unwrap_or_default()
}

fn iso(conn: &Connection, ms: i64) -> String {
    local_time(conn, ms, "%Y-%m-%dT%H:%M:%S")
}

/// Search hits come wrapped in \u{1} … \u{2}.
fn strip_marks(s: &str) -> String {
    s.replace(['\u{1}', '\u{2}'], "")
}

struct Out {
    color: bool,
}

impl Out {
    fn new() -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() && enable_ansi();
        Out { color }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn accent(&self, s: &str) -> String {
        self.paint("38;5;209", s)
    }

    fn line(&self, s: &str) {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{s}");
    }

    fn json<T: Serialize + ?Sized>(&self, v: &T) {
        self.line(&serde_json::to_string_pretty(v).unwrap_or_default());
    }

    fn snippet(&self, s: &str) -> String {
        if !self.color {
            return strip_marks(s);
        }
        s.replace('\u{1}', "\x1b[1;33m").replace('\u{2}', "\x1b[0;2m")
    }

    fn tags(&self, tags: &[String]) -> String {
        self.accent(&tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "))
    }

    fn summaries(&self, conn: &Connection, rows: &[EntrySummary], json: bool, query: Option<&str>) -> Result<(), String> {
        if json {
            let v: Vec<JsonSummary> = rows
                .iter()
                .map(|r| JsonSummary {
                    id: r.id,
                    title: &r.title,
                    snippet: strip_marks(&r.snippet),
                    tags: &r.tags,
                    pinned: r.pinned,
                    created: iso(conn, r.created_at),
                    updated: iso(conn, r.updated_at),
                })
                .collect();
            self.json(&v);
            return Ok(());
        }
        if rows.is_empty() {
            self.line(&match query {
                Some(q) => format!("No notes match “{q}”."),
                None => "No notes yet — add one with: bt add \"What I did\"".into(),
            });
            return Ok(());
        }
        let width = rows.iter().map(|r| r.id.to_string().len()).max().unwrap_or(1) + 1;
        for r in rows {
            let title = if r.title.trim().is_empty() { "Untitled" } else { r.title.as_str() };
            let pin = if r.pinned { self.accent("● ") } else { String::new() };
            let id = format!("{:>width$}", format!("#{}", r.id));
            let mut line = format!(
                "{}  {}  {pin}{}",
                self.dim(&id),
                self.dim(&local_time(conn, r.updated_at, "%Y-%m-%d")),
                self.bold(title)
            );
            if !r.tags.is_empty() {
                line.push_str("  ");
                line.push_str(&self.tags(&r.tags));
            }
            self.line(&line);
            // Skip the snippet when the match was in the title (it would just repeat it).
            if !r.snippet.trim().is_empty() && strip_marks(&r.snippet).trim() != title.trim() {
                let pad = " ".repeat(width + 14);
                self.line(&format!("{pad}{}", self.dim(&self.snippet(&r.snippet))));
            }
        }
        Ok(())
    }

    fn entry(&self, conn: &Connection, e: &Entry, json: bool) -> Result<(), String> {
        if json {
            self.json(&JsonEntry::from(conn, e));
            return Ok(());
        }
        let title = if e.title.trim().is_empty() { "Untitled" } else { e.title.as_str() };
        self.line(&format!("{}  {}", self.dim(&format!("#{}", e.id)), self.bold(title)));
        let mut meta = Vec::new();
        if !e.tags.is_empty() {
            meta.push(self.tags(&e.tags));
        }
        if e.pinned {
            meta.push(self.accent("pinned"));
        }
        meta.push(self.dim(&format!(
            "created {} · updated {}",
            local_time(conn, e.created_at, "%Y-%m-%d %H:%M"),
            local_time(conn, e.updated_at, "%Y-%m-%d %H:%M")
        )));
        self.line(&meta.join(&self.dim(" · ")));
        if !e.body.trim().is_empty() {
            self.line("");
            self.line(e.body.trim_end());
        }
        Ok(())
    }
}

/// Colours need "virtual terminal" mode on Windows consoles.
#[cfg(windows)]
fn enable_ansi() -> bool {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
    };
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        GetConsoleMode(h, &mut mode) != 0 && SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_ansi() -> bool {
    true
}

/// The windowed `braintracker.exe` on Windows has no console of its own: borrow the terminal
/// that started it (if any) so messages show up there.
#[cfg(windows)]
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 && std::io::stdout().is_terminal() {
            println!();
        }
    });
}

#[cfg(not(windows))]
pub fn attach_console() {}

pub fn print_out(s: &str) {
    attach_console();
    print!("{s}");
    let _ = std::io::stdout().flush();
}

pub fn print_err(s: &str) {
    attach_console();
    eprintln!("bt: {s}");
}

/// Entry point of the `bt` console program.
pub fn console_main() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        print_out(HELP);
        return 0;
    }
    match parse(&args) {
        Parsed::Info(text) => {
            print_out(&text);
            0
        }
        Parsed::Error(e) => {
            print_err(&e);
            2
        }
        Parsed::Headless(cmd, json) => execute(cmd, json),
        // Hand anything that needs a window to the app (it passes it on if already running).
        Parsed::Gui(_) => {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "braintracker.exe" } else { "braintracker" })));
            match exe.map(|exe| std::process::Command::new(&exe).args(&args[1..]).spawn()) {
                Some(Ok(_)) => 0,
                _ => {
                    print_err("couldn’t start BrainTracker (it should be in the same folder as bt)");
                    1
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &str) -> Parsed {
        let args: Vec<String> = std::iter::once("bt").chain(a.split_whitespace()).map(String::from).collect();
        parse(&args)
    }

    #[test]
    fn plain_launch_and_open() {
        assert_eq!(p(""), Parsed::Gui(None));
        assert_eq!(p("--minimized"), Parsed::Gui(None));
        assert_eq!(p("--open"), Parsed::Gui(Some(GuiAction::Show)));
    }

    #[test]
    fn open_flag_moves_commands_into_the_app() {
        assert_eq!(p("search --open"), Parsed::Gui(Some(GuiAction::Search(None))));
        assert_eq!(p("search docker dns --open"), Parsed::Gui(Some(GuiAction::Search(Some("docker dns".into())))));
        assert_eq!(p("add --open"), Parsed::Gui(Some(GuiAction::Capture)));
        assert_eq!(p("add --open --selection"), Parsed::Gui(Some(GuiAction::CaptureSelection)));
        assert_eq!(p("search --open --selection"), Parsed::Gui(Some(GuiAction::SearchSelection)));
        assert_eq!(p("show 12 --open"), Parsed::Gui(Some(GuiAction::OpenEntry(12))));
    }

    #[test]
    fn headless_commands() {
        assert_eq!(p("list"), Parsed::Headless(Cmd::List { tag: None, limit: 20 }, false));
        assert_eq!(
            p("list --tag docker --limit 5 --json"),
            Parsed::Headless(Cmd::List { tag: Some("docker".into()), limit: 5 }, true)
        );
        assert_eq!(
            p("search tag:wsl dns"),
            Parsed::Headless(Cmd::Search { query: "tag:wsl dns".into(), limit: None }, false)
        );
        assert_eq!(p("show #7"), Parsed::Headless(Cmd::Show { id: 7 }, false));
        assert_eq!(
            p("add Fixed the printer --tags Home,office --body -"),
            Parsed::Headless(
                Cmd::Add {
                    title: "Fixed the printer".into(),
                    body: Some("-".into()),
                    tags: vec!["home".into(), "office".into()],
                    selection: false
                },
                false
            )
        );
        assert_eq!(
            p("edit 3 --append=done"),
            Parsed::Headless(Cmd::Edit { id: 3, title: None, body: None, append: Some("done".into()), tags: None }, false)
        );
        assert_eq!(p("unpin 4"), Parsed::Headless(Cmd::Pin { id: 4, pinned: false }, false));
        assert_eq!(p("rm 4"), Parsed::Headless(Cmd::Delete { id: 4 }, false));
    }

    #[test]
    fn usage_errors() {
        for bad in ["search", "show", "show abc", "add", "edit 3", "frobnicate", "list --limit x", "list --nope", "add x --body"] {
            assert!(matches!(p(bad), Parsed::Error(_)), "{bad} should be an error");
        }
        assert!(matches!(p("--help"), Parsed::Info(_)));
        assert!(matches!(p("list -V"), Parsed::Info(_)));
    }

    #[test]
    fn commands_against_a_real_database() {
        let conn = db::open(std::path::Path::new(":memory:")).unwrap();
        let e = db::create_entry(&conn, "Fix docker DNS", "set nameserver", &["docker".into()]).unwrap();
        assert!(live_entry(&conn, e.id).is_ok());
        assert!(live_entry(&conn, 999).err().unwrap().contains("no note #999"));
        db::trash_entry(&conn, e.id).unwrap();
        assert!(live_entry(&conn, e.id).err().unwrap().contains("trash"));
        assert_eq!(strip_marks("a\u{1}hit\u{2}b"), "ahitb");
    }
}
