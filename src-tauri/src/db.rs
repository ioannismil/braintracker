//! SQLite storage with FTS5 full-text search.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db(pub Mutex<Connection>);

/// Markers wrapped around search hits in snippets (rendered as highlights by the UI).
const HIT_START: &str = "\u{1}";
const HIT_END: &str = "\u{2}";

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
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// "Rust, How To ,rust" -> ",rust,how-to,"
fn encode_tags(tags: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let norm = t
            .trim()
            .trim_start_matches('#')
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .replace(',', "");
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

fn preview(body: &str) -> String {
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

pub fn list_entries(
    conn: &Connection,
    query: Option<&str>,
    tag: Option<&str>,
) -> rusqlite::Result<Vec<EntrySummary>> {
    let tag = tag.filter(|t| !t.is_empty());

    if let Some(q) = query.and_then(fts_query) {
        let mut stmt = conn.prepare(
            "SELECT e.id, e.title,
                    snippet(entries_fts, -1, ?3, ?4, '…', 16),
                    e.tags, e.pinned, e.created_at, e.updated_at
             FROM entries_fts
             JOIN entries e ON e.id = entries_fts.rowid
             WHERE entries_fts MATCH ?1
               AND (?2 IS NULL OR e.tags LIKE '%,' || ?2 || ',%')
             ORDER BY bm25(entries_fts, 10.0, 1.0, 5.0)
             LIMIT 300",
        )?;
        let rows = stmt.query_map(params![q, tag, HIT_START, HIT_END], |r| {
            Ok(EntrySummary {
                id: r.get(0)?,
                title: r.get(1)?,
                snippet: r.get(2)?,
                tags: decode_tags(&r.get::<_, String>(3)?),
                pinned: r.get::<_, i64>(4)? != 0,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        return rows.collect();
    }

    let mut stmt = conn.prepare(
        "SELECT id, title, body, tags, pinned, created_at, updated_at
         FROM entries
         WHERE (?1 IS NULL OR tags LIKE '%,' || ?1 || ',%')
         ORDER BY pinned DESC, updated_at DESC",
    )?;
    let rows = stmt.query_map(params![tag], |r| {
        Ok(EntrySummary {
            id: r.get(0)?,
            title: r.get(1)?,
            snippet: preview(&r.get::<_, String>(2)?),
            tags: decode_tags(&r.get::<_, String>(3)?),
            pinned: r.get::<_, i64>(4)? != 0,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    })?;
    rows.collect()
}

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
    conn.execute(
        "INSERT INTO entries (title, body, tags, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title.trim(), body, encode_tags(tags), now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(get_entry(conn, id)?.expect("just inserted"))
}

pub fn update_entry(
    conn: &Connection,
    id: i64,
    title: &str,
    body: &str,
    tags: &[String],
) -> rusqlite::Result<Option<Entry>> {
    conn.execute(
        "UPDATE entries SET title = ?2, body = ?3, tags = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, title, body, encode_tags(tags), now_ms()],
    )?;
    get_entry(conn, id)
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE entries SET pinned = ?2 WHERE id = ?1",
        params![id, pinned as i64],
    )?;
    Ok(())
}

pub fn delete_entry(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_tags(conn: &Connection) -> rusqlite::Result<Vec<TagCount>> {
    let mut counts: std::collections::BTreeMap<String, i64> = Default::default();
    let mut stmt = conn.prepare("SELECT tags FROM entries WHERE tags != ''")?;
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

/// Every entry plus its local created/updated date, for Markdown export.
pub fn all_for_export(conn: &Connection) -> rusqlite::Result<Vec<(Entry, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, tags, pinned, created_at, updated_at,
                strftime('%Y-%m-%d %H:%M', created_at / 1000, 'unixepoch', 'localtime'),
                strftime('%Y-%m-%d %H:%M', updated_at / 1000, 'unixepoch', 'localtime')
         FROM entries ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |r| Ok((row_to_entry(r)?, r.get(7)?, r.get(8)?)))?;
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
        c.execute_batch(SCHEMA).unwrap();
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
        delete_entry(&c, e.id).unwrap();
        assert!(list_entries(&c, Some("resolv"), None).unwrap().is_empty());
    }
}
