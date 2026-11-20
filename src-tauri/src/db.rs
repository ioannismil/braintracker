//! SQLite storage: entries with FTS5 full-text search, search filters, trash and edit history.

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db(pub Mutex<Connection>);

/// Markers wrapped around search hits in snippets (rendered as highlights by the UI).
#[cfg(test)]
const HIT_START: &str = "\u{1}";

/// Entries stay in the trash this long before they're deleted for good.
pub const TRASH_DAYS: i64 = 30;
/// A new history version is saved at most this often while you keep editing.
const VERSION_GAP_MS: i64 = 10 * 60 * 1000;
/// Versions kept per entry.
const VERSIONS_KEPT: i64 = 50;
const DAY_MS: i64 = 86_400_000;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL DEFAULT '',
    body        TEXT    NOT NULL DEFAULT '',
    tags        TEXT    NOT NULL DEFAULT '',   -- stored as ",tag-a,tag-b,"
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,              -- unix ms
    updated_at  INTEGER NOT NULL
    -- deleted_at INTEGER (added by migrate(): NULL = live, otherwise when it was trashed)
);

CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
    title, body, tags,
    content = 'entries', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
    INSERT INTO entries_fts(rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE OF title, body, tags ON entries BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
    INSERT INTO entries_fts(rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;

CREATE TABLE IF NOT EXISTS entry_versions (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id  INTEGER NOT NULL,
    title     TEXT    NOT NULL,
    body      TEXT    NOT NULL,
    tags      TEXT    NOT NULL,
    saved_at  INTEGER NOT NULL,   -- when this content was last edited
    taken_at  INTEGER NOT NULL    -- when the snapshot was taken
);
CREATE INDEX IF NOT EXISTS entry_versions_entry ON entry_versions(entry_id, id);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: i64,
    pub title: String,
    pub snippet: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub id: i64,
    pub title: String,
    pub snippet: String,
    pub tags: Vec<String>,
    pub deleted_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: i64,
    pub title: String,
    pub snippet: String,
    pub chars: i64,
    pub saved_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: i64,
    pub entry_id: i64,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub saved_at: i64,
}

#[derive(Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // The app and the `bt` command line can both have the database open: wait for each other.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    migrate(conn)
}

/// Upgrades databases created by older versions in place.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let has_deleted: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('entries') WHERE name = 'deleted_at'",
        [],
        |r| r.get(0),
    )?;
    if has_deleted == 0 {
        conn.execute_batch("ALTER TABLE entries ADD COLUMN deleted_at INTEGER")?;
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn normalize_tag(t: &str) -> String {
    t.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .replace(',', "")
}

/// "Docker, How To" (as typed on the command line) -> ["docker", "how-to"]
pub fn parse_tags_arg(text: &str) -> Vec<String> {
    text.split(',').map(normalize_tag).filter(|t| !t.is_empty()).collect()
}

/// "Rust, How To ,rust" -> ",rust,how-to,"
fn encode_tags(tags: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let norm = normalize_tag(t);
        if !norm.is_empty() && !out.contains(&norm) {
            out.push(norm);
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!(",{},", out.join(","))
    }
}

fn decode_tags(raw: &str) -> Vec<String> {
    raw.split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Turns free text into a safe FTS5 query: every word must match, as a prefix.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

pub fn preview(body: &str) -> String {
    let flat = body
        .split_whitespace()
        .map(|w| if w.contains("](img:") { "🖼" } else { w })
        .collect::<Vec<_>>()
        .join(" ");
    flat.chars().take(160).collect()
}

fn row_to_entry(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        tags: decode_tags(&r.get::<_, String>(3)?),
        pinned: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

// ---------- search filters ----------

/// A search box query split into free text and `key:value` filters.
#[derive(Default, Debug, PartialEq)]
pub struct Filters {
    pub text: String,
    pub tags: Vec<String>,
    pub after: Vec<(String, &'static str)>,
    pub before: Vec<(String, &'static str)>,
    pub on: Vec<(String, &'static str)>,
    pub has: Vec<String>,
    pub pinned: bool,
}

/// "2026" / "2026-10" / "2026-10-05" → (start date, SQLite modifier for the period's length)
fn parse_period(v: &str) -> Option<(String, &'static str)> {
    let p: Vec<&str> = v.split('-').collect();
    let num = |s: &str, len: usize| s.len() == len && s.chars().all(|c| c.is_ascii_digit());
    match p.as_slice() {
        [y] if num(y, 4) => Some((format!("{y}-01-01"), "+1 year")),
        [y, m] if num(y, 4) && (num(m, 2) || num(m, 1)) => Some((format!("{y}-{:0>2}-01", m), "+1 month")),
        [y, m, d] if num(y, 4) && (num(m, 2) || num(m, 1)) && (num(d, 2) || num(d, 1)) => {
            Some((format!("{y}-{:0>2}-{:0>2}", m, d), "+1 day"))
        }
        _ => None,
    }
}

pub fn parse_query(q: &str) -> Filters {
    let mut f = Filters::default();
    let mut text: Vec<&str> = Vec::new();
    for token in q.split_whitespace() {
        let Some((k, v)) = token.split_once(':') else {
            text.push(token);
            continue;
        };
        let v = v.trim();
        let handled = !v.is_empty()
            && match k.to_ascii_lowercase().as_str() {
                "tag" | "t" => {
                    f.tags.push(normalize_tag(v));
                    true
                }
                "after" | "since" | "from" => parse_period(v).map(|p| f.after.push(p)).is_some(),
                "before" | "until" => parse_period(v).map(|p| f.before.push(p)).is_some(),
                "on" | "in" | "date" => parse_period(v).map(|p| f.on.push(p)).is_some(),
                "has" => matches!(v.to_ascii_lowercase().as_str(), "image" | "img" | "code" | "link")
                    .then(|| f.has.push(v.to_ascii_lowercase()))
                    .is_some(),
                "is" => v.eq_ignore_ascii_case("pinned").then(|| f.pinned = true).is_some(),
                _ => false,
            };
        if !handled {
            text.push(token); // e.g. "http://…" or an unknown key stays plain text
        }
    }
    f.text = text.join(" ");
    f
}

const EPOCH_MS: &str = "CAST(strftime('%s', ?, 'utc') AS INTEGER) * 1000";
const EPOCH_END_MS: &str = "CAST(strftime('%s', ?, ?, 'utc') AS INTEGER) * 1000";

fn filter_sql(f: &Filters, sidebar_tag: Option<&str>) -> (String, Vec<Value>) {
    let mut conds = vec!["e.deleted_at IS NULL".to_string()];
    let mut args: Vec<Value> = Vec::new();

    for t in sidebar_tag.into_iter().map(String::from).chain(f.tags.iter().cloned()) {
        conds.push("e.tags LIKE ?".into());
        args.push(Value::Text(format!("%,{t},%")));
    }
    for (start, _) in &f.after {
        conds.push(format!("e.created_at >= {EPOCH_MS}"));
        args.push(Value::Text(start.clone()));
    }
    for (start, _) in &f.before {
        conds.push(format!("e.created_at < {EPOCH_MS}"));
        args.push(Value::Text(start.clone()));
    }
    for (start, len) in &f.on {
        conds.push(format!("(e.created_at >= {EPOCH_MS} AND e.created_at < {EPOCH_END_MS})"));
        args.push(Value::Text(start.clone()));
        args.push(Value::Text(start.clone()));
        args.push(Value::Text((*len).to_string()));
    }
    for h in &f.has {
        conds.push(
            match h.as_str() {
                "image" | "img" => "e.body LIKE '%](img:%'",
                "code" => "e.body LIKE '%`%'",
                _ => "(e.body LIKE '%http://%' OR e.body LIKE '%https://%')",
            }
            .into(),
        );
    }
    if f.pinned {
        conds.push("e.pinned = 1".into());
    }
    (conds.join(" AND "), args)
}

pub fn list_entries(
    conn: &Connection,
    query: Option<&str>,
    tag: Option<&str>,
) -> rusqlite::Result<Vec<EntrySummary>> {
    let filters = parse_query(query.unwrap_or(""));
    let (conds, mut args) = filter_sql(&filters, tag.filter(|t| !t.is_empty()));

    let summary = |r: &Row, snippet: String| -> rusqlite::Result<EntrySummary> {
        Ok(EntrySummary {
            id: r.get(0)?,
            title: r.get(1)?,
            snippet,
            tags: decode_tags(&r.get::<_, String>(3)?),
            pinned: r.get::<_, i64>(4)? != 0,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    };

    if let Some(q) = fts_query(&filters.text) {
        let sql = format!(
            "SELECT e.id, e.title, snippet(entries_fts, -1, char(1), char(2), '…', 16),
                    e.tags, e.pinned, e.created_at, e.updated_at
             FROM entries_fts JOIN entries e ON e.id = entries_fts.rowid
             WHERE entries_fts MATCH ? AND {conds}
             ORDER BY bm25(entries_fts, 10.0, 1.0, 5.0)
             LIMIT 300"
        );
        args.insert(0, Value::Text(q));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let s: String = r.get(2)?;
            summary(r, s)
        })?;
        return rows.collect();
    }

    let sql = format!(
        "SELECT e.id, e.title, e.body, e.tags, e.pinned, e.created_at, e.updated_at
         FROM entries e WHERE {conds}
         ORDER BY e.pinned DESC, e.updated_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        let b: String = r.get(2)?;
        summary(r, preview(&b))
    })?;
    rows.collect()
}

// ---------- entries ----------

pub fn get_entry(conn: &Connection, id: i64) -> rusqlite::Result<Option<Entry>> {
    conn.query_row(
        "SELECT id, title, body, tags, pinned, created_at, updated_at FROM entries WHERE id = ?1",
        params![id],
        row_to_entry,
    )
    .optional()
}

pub fn create_entry(
    conn: &Connection,
    title: &str,
    body: &str,
    tags: &[String],
) -> rusqlite::Result<Entry> {
    let now = now_ms();
    import_entry(conn, title, body, tags, now, now)
}

/// Like create_entry, but with explicit timestamps (used by import).
pub fn import_entry(
    conn: &Connection,
    title: &str,
    body: &str,
    tags: &[String],
    created_at: i64,
    updated_at: i64,
) -> rusqlite::Result<Entry> {
    conn.execute(
        "INSERT INTO entries (title, body, tags, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![title.trim(), body, encode_tags(tags), created_at, updated_at],
    )?;
    let id = conn.last_insert_rowid();
    Ok(get_entry(conn, id)?.expect("just inserted"))
}

/// True if a live entry with this title and creation time already exists (import de-duplication).
pub fn entry_exists(conn: &Connection, title: &str, created_at: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE title = ?1 AND created_at = ?2 AND deleted_at IS NULL)",
        params![title.trim(), created_at],
        |r| r.get(0),
    )
}

/// "2026-11-17 14:32" (local time) → unix ms
pub fn local_to_ms(conn: &Connection, local: &str) -> Option<i64> {
    conn.query_row(
        "SELECT CAST(strftime('%s', ?1, 'utc') AS INTEGER) * 1000",
        params![local.trim()],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
}

pub fn update_entry(
    conn: &Connection,
    id: i64,
    title: &str,
    body: &str,
    tags: &[String],
) -> rusqlite::Result<Option<Entry>> {
    let new_tags = encode_tags(tags);
    let old: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT title, body, tags, updated_at FROM entries WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((old_title, old_body, old_tags, old_updated)) = old else {
        return Ok(None);
    };
    if old_title == title && old_body == body && old_tags == new_tags {
        return get_entry(conn, id); // nothing changed — don't bump "edited"
    }

    // Edit history: keep the state from before this burst of editing.
    let now = now_ms();
    let last_taken: Option<i64> = conn.query_row(
        "SELECT max(taken_at) FROM entry_versions WHERE entry_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    let blank = old_title.trim().is_empty() && old_body.trim().is_empty();
    if !blank && last_taken.is_none_or(|t| now - t >= VERSION_GAP_MS) {
        snapshot(conn, id, &old_title, &old_body, &old_tags, old_updated, now)?;
    }

    conn.execute(
        "UPDATE entries SET title = ?2, body = ?3, tags = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, title, body, new_tags, now],
    )?;
    get_entry(conn, id)
}

fn snapshot(
    conn: &Connection,
    id: i64,
    title: &str,
    body: &str,
    tags: &str,
    saved_at: i64,
    taken_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO entry_versions (entry_id, title, body, tags, saved_at, taken_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title, body, tags, saved_at, taken_at],
    )?;
    conn.execute(
        "DELETE FROM entry_versions WHERE entry_id = ?1 AND id NOT IN
           (SELECT id FROM entry_versions WHERE entry_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![id, VERSIONS_KEPT],
    )?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE entries SET pinned = ?2 WHERE id = ?1",
        params![id, pinned as i64],
    )?;
    Ok(())
}

// ---------- trash ----------

pub fn trash_entry(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE entries SET deleted_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
    Ok(())
}

pub fn restore_entry(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE entries SET deleted_at = NULL WHERE id = ?1", params![id])?;
    Ok(())
}

/// Deletes an entry and its history for good.
pub fn purge_entry(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM entry_versions WHERE entry_id = ?1", params![id])?;
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_trash(conn: &Connection) -> rusqlite::Result<Vec<TrashItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, tags, deleted_at FROM entries
         WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TrashItem {
            id: r.get(0)?,
            title: r.get(1)?,
            snippet: preview(&r.get::<_, String>(2)?),
            tags: decode_tags(&r.get::<_, String>(3)?),
            deleted_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn trash_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM entries WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
}

/// Purges trashed entries older than `older_than_ms` ago (0 = everything in the trash).
pub fn purge_trash(conn: &Connection, older_than_ms: i64) -> rusqlite::Result<usize> {
    let cutoff = now_ms() - older_than_ms;
    conn.execute(
        "DELETE FROM entry_versions WHERE entry_id IN
           (SELECT id FROM entries WHERE deleted_at IS NOT NULL AND deleted_at <= ?1)",
        params![cutoff],
    )?;
    conn.execute(
        "DELETE FROM entries WHERE deleted_at IS NOT NULL AND deleted_at <= ?1",
        params![cutoff],
    )
}

pub fn purge_expired_trash(conn: &Connection) -> rusqlite::Result<usize> {
    purge_trash(conn, TRASH_DAYS * DAY_MS)
}

// ---------- history ----------

pub fn list_versions(conn: &Connection, entry_id: i64) -> rusqlite::Result<Vec<VersionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, saved_at FROM entry_versions WHERE entry_id = ?1 ORDER BY id DESC",
    )?;
    let rows = stmt.query_map(params![entry_id], |r| {
        let body: String = r.get(2)?;
        Ok(VersionSummary {
            id: r.get(0)?,
            title: r.get(1)?,
            chars: body.chars().count() as i64,
            snippet: preview(&body),
            saved_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn get_version(conn: &Connection, id: i64) -> rusqlite::Result<Option<Version>> {
    conn.query_row(
        "SELECT id, entry_id, title, body, tags, saved_at FROM entry_versions WHERE id = ?1",
        params![id],
        |r| {
            Ok(Version {
                id: r.get(0)?,
                entry_id: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                tags: decode_tags(&r.get::<_, String>(4)?),
                saved_at: r.get(5)?,
            })
        },
    )
    .optional()
}

/// Puts an old version back. The current content is saved as a version first, so it's undoable.
pub fn restore_version(conn: &Connection, entry_id: i64, version_id: i64) -> rusqlite::Result<Option<Entry>> {
    let Some(v) = get_version(conn, version_id)?.filter(|v| v.entry_id == entry_id) else {
        return Ok(None);
    };
    let Some(cur) = get_entry(conn, entry_id)? else {
        return Ok(None);
    };
    let now = now_ms();
    let cur_tags = encode_tags(&cur.tags);
    snapshot(conn, entry_id, &cur.title, &cur.body, &cur_tags, cur.updated_at, now)?;
    conn.execute(
        "UPDATE entries SET title = ?2, body = ?3, tags = ?4, updated_at = ?5 WHERE id = ?1",
        params![entry_id, v.title, v.body, encode_tags(&v.tags), now],
    )?;
    get_entry(conn, entry_id)
}

// ---------- tags, export, images ----------

pub fn list_tags(conn: &Connection) -> rusqlite::Result<Vec<TagCount>> {
    let mut counts: std::collections::BTreeMap<String, i64> = Default::default();
    let mut stmt = conn.prepare("SELECT tags FROM entries WHERE tags != '' AND deleted_at IS NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for raw in rows {
        for t in decode_tags(&raw?) {
            *counts.entry(t).or_default() += 1;
        }
    }
    let mut out: Vec<TagCount> = counts
        .into_iter()
        .map(|(tag, count)| TagCount { tag, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.tag.cmp(&b.tag)));
    Ok(out)
}

/// Every live entry plus its local created/updated date, for Markdown export.
pub fn all_for_export(conn: &Connection) -> rusqlite::Result<Vec<(Entry, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, tags, pinned, created_at, updated_at,
                strftime('%Y-%m-%d %H:%M', created_at / 1000, 'unixepoch', 'localtime'),
                strftime('%Y-%m-%d %H:%M', updated_at / 1000, 'unixepoch', 'localtime')
         FROM entries WHERE deleted_at IS NULL ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |r| Ok((row_to_entry(r)?, r.get(7)?, r.get(8)?)))?;
    rows.collect()
}

/// File names of every screenshot still referenced anywhere — live entries, the trash
/// and edit history — so cleanup never removes an image something could bring back.
pub fn referenced_images(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT body FROM entries WHERE body LIKE '%](img:%'
         UNION ALL SELECT body FROM entry_versions WHERE body LIKE '%](img:%'",
    )?;
    let mut out = HashSet::new();
    for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
        for part in body?.split("](img:").skip(1) {
            let name: String = part.chars().take_while(|c| *c != ')').collect();
            if !name.is_empty() {
                out.insert(name);
            }
        }
    }
    Ok(out)
}

// ---------- backup helpers ----------

/// Cheap "did anything change?" signature, so daily backups skip identical copies.
pub fn fingerprint(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT (SELECT count(*) || ':' || coalesce(max(updated_at), 0) || ':' || coalesce(sum(pinned), 0)
                     || ':' || coalesce(sum(id), 0) || ':' || coalesce(sum(deleted_at), 0) FROM entries)
             || ':' || (SELECT count(*) || ':' || coalesce(max(id), 0) FROM entry_versions)",
        [],
        |r| r.get(0),
    )
}

/// Writes a consistent, compacted copy of the live database to `dest` (must not exist).
pub fn vacuum_into(conn: &Connection, dest: &Path) -> rusqlite::Result<()> {
    conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
    Ok(())
}

/// Local time as "2026-11-18_143005", for backup folder names.
pub fn local_stamp(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT strftime('%Y-%m-%d_%H%M%S', 'now', 'localtime')", [], |r| r.get(0))
}

pub fn all_settings(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    #[test]
    fn create_search_update_delete() {
        let c = mem();
        let e = create_entry(&c, "Fixed docker DNS", "", &["Docker".into(), " networking ".into()]).unwrap();
        assert_eq!(e.tags, vec!["docker", "networking"]);
        update_entry(&c, e.id, "Fixed docker DNS", "Edited /etc/resolv.conf and restarted", &e.tags).unwrap();

        let hits = list_entries(&c, Some("resolv"), None).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains(HIT_START));

        assert_eq!(list_entries(&c, Some("doc"), Some("docker")).unwrap().len(), 1);
        assert_eq!(list_entries(&c, None, Some("python")).unwrap().len(), 0);
        assert_eq!(list_entries(&c, Some("\"weird ( query"), None).unwrap().len(), 0);

        assert_eq!(list_tags(&c).unwrap().len(), 2);
        let before = fingerprint(&c).unwrap();
        trash_entry(&c, e.id).unwrap();
        assert!(list_entries(&c, Some("resolv"), None).unwrap().is_empty());
        assert_ne!(before, fingerprint(&c).unwrap());
    }

    #[test]
    fn trash_restore_and_purge() {
        let c = mem();
        let e = create_entry(&c, "Keep me", "notes", &["a".into()]).unwrap();
        trash_entry(&c, e.id).unwrap();
        assert_eq!(list_entries(&c, None, None).unwrap().len(), 0);
        assert_eq!(list_tags(&c).unwrap().len(), 0);
        assert_eq!(trash_count(&c).unwrap(), 1);

        restore_entry(&c, e.id).unwrap();
        assert_eq!(list_entries(&c, None, None).unwrap().len(), 1);

        trash_entry(&c, e.id).unwrap();
        assert_eq!(purge_expired_trash(&c).unwrap(), 0); // too recent
        assert_eq!(purge_trash(&c, 0).unwrap(), 1);
        assert!(get_entry(&c, e.id).unwrap().is_none());
    }

    #[test]
    fn query_filters() {
        assert_eq!(
            parse_query("dns tag:Docker after:2026-10 has:image is:pinned http://x.io"),
            Filters {
                text: "dns http://x.io".into(),
                tags: vec!["docker".into()],
                after: vec![("2026-10-01".into(), "+1 month")],
                has: vec!["image".into()],
                pinned: true,
                ..Default::default()
            }
        );
        assert_eq!(parse_query("before:nonsense").text, "before:nonsense");

        let c = mem();
        let a = create_entry(&c, "Screenshot fix", "see ![s](img:1.png) and `ls`", &["docker".into()]).unwrap();
        let b = create_entry(&c, "Plain", "nothing here", &[]).unwrap();
        set_pinned(&c, b.id, true).unwrap();
        c.execute("UPDATE entries SET created_at = ?2 WHERE id = ?1", params![b.id, local_to_ms(&c, "2020-03-15 12:00").unwrap()])
            .unwrap();

        let ids = |q: &str| list_entries(&c, Some(q), None).unwrap().iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids("has:image"), vec![a.id]);
        assert_eq!(ids("has:code tag:docker"), vec![a.id]);
        assert_eq!(ids("is:pinned"), vec![b.id]);
        assert_eq!(ids("on:2020-03"), vec![b.id]);
        assert_eq!(ids("before:2021"), vec![b.id]);
        assert_eq!(ids("after:2021"), vec![a.id]);
        assert_eq!(ids("screenshot after:2021"), vec![a.id]);
        assert!(ids("screenshot before:2021").is_empty());
    }

    #[test]
    fn history_versions_and_restore() {
        let c = mem();
        let e = create_entry(&c, "", "", &[]).unwrap();
        update_entry(&c, e.id, "Title", "first version", &[]).unwrap(); // old state was blank → no version
        assert_eq!(list_versions(&c, e.id).unwrap().len(), 0);

        update_entry(&c, e.id, "Title", "second version", &[]).unwrap(); // first snapshot
        let v = list_versions(&c, e.id).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].snippet.contains("first"));
        update_entry(&c, e.id, "Title", "third version", &[]).unwrap(); // within 10 min → no new version
        assert_eq!(list_versions(&c, e.id).unwrap().len(), 1);

        let restored = restore_version(&c, e.id, v[0].id).unwrap().unwrap();
        assert_eq!(restored.body, "first version");
        assert_eq!(list_versions(&c, e.id).unwrap().len(), 2); // "third version" saved before restoring

        purge_entry(&c, e.id).unwrap();
        assert!(list_versions(&c, e.id).unwrap().is_empty());
    }

    #[test]
    fn referenced_images_include_trash_and_history() {
        let c = mem();
        let e = create_entry(&c, "T", "![](img:a.png)", &[]).unwrap();
        update_entry(&c, e.id, "T", "![](img:b.png)", &[]).unwrap();
        let other = create_entry(&c, "U", "![](img:c.png)", &[]).unwrap();
        trash_entry(&c, other.id).unwrap();
        let refs = referenced_images(&c).unwrap();
        assert!(refs.contains("a.png") && refs.contains("b.png") && refs.contains("c.png"));
    }

    #[test]
    fn migrates_old_database() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE entries (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL DEFAULT '',
             body TEXT NOT NULL DEFAULT '', tags TEXT NOT NULL DEFAULT '', pinned INTEGER NOT NULL DEFAULT 0,
             created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             INSERT INTO entries (title, created_at, updated_at) VALUES ('old', 1, 1);",
        )
        .unwrap();
        init(&c).unwrap();
        init(&c).unwrap(); // idempotent
        assert_eq!(list_entries(&c, None, None).unwrap().len(), 1);
    }

    #[test]
    fn vacuum_into_makes_a_readable_copy() {
        let dir = std::env::temp_dir().join(format!("bt-test-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let live = open(&dir.join("live.db")).unwrap();
        create_entry(&live, "Backed up", "body text", &[]).unwrap();
        let copy_path = dir.join("copy.db");
        vacuum_into(&live, &copy_path).unwrap();
        let copy = open(&copy_path).unwrap();
        assert_eq!(list_entries(&copy, Some("body"), None).unwrap().len(), 1);
        assert_eq!(fingerprint(&live).unwrap(), fingerprint(&copy).unwrap());
        drop((live, copy));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
