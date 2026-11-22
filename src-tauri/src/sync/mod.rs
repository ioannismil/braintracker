//! Sync between the PC and a phone over Wi-Fi.
//!
//! The PC runs a small server (server.rs) and the phone connects to it (client.rs). They pair
//! once: the PC shows a QR code holding its address and a fresh secret key, and the phone scans
//! it. Every request and reply after that is encrypted with that key (crypto.rs), so nobody
//! else on the network can read or change the notes. Nothing goes through the internet.
//!
//! What gets synced: every entry (title, notes, tags, pin, trash), entries deleted for good,
//! pasted screenshots and the text read from them. Edit history, settings and backups stay on
//! each device.
//!
//! How changes are found: each device numbers its own changes (db.rs: every write gives the
//! entry the next number in `rev`). A device remembers, for the other side, the last number it
//! has seen, and asks only for what came after. A change copied in from a device is marked as
//! coming from it (`rev_from`), so it isn't sent straight back.
//!
//! When both changed the same entry, the one changed last wins (each entry carries when it was
//! last changed, `modified_at`, and an edit is always later than the version it replaced, even
//! with clocks that disagree). The version that loses is kept in the entry's History, so
//! nothing is ever lost.

pub mod commands;
pub mod crypto;
#[cfg_attr(desktop, allow(dead_code))]
pub mod client;
// The phone uses its pairing-code and image helpers too.
#[cfg_attr(mobile, allow(dead_code))]
pub mod server;

use crate::db;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The TCP port the PC listens on (the next free one is used if it's taken).
pub const DEFAULT_PORT: u16 = 47812;
/// UDP port for finding the PC when its address changed (see client::discover).
pub const DISCOVERY_PORT: u16 = 47813;
/// Most entries sent in one reply; a large first sync takes a few rounds.
pub const BATCH: usize = 300;
/// Pairing codes stop working after this long.
pub const PAIRING_MINUTES: i64 = 10;

/// 128 random bits as hex: ids for devices, entries and pairing.
pub fn random_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system can make random numbers");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// This device's id and name, made on first use.
pub fn device_id(conn: &Connection) -> rusqlite::Result<String> {
    if let Some(id) = db::get_setting(conn, "device_id")? {
        return Ok(id);
    }
    let id = random_id();
    db::set_setting(conn, "device_id", &id)?;
    Ok(id)
}

pub fn device_name() -> String {
    let from_env = ["COMPUTERNAME", "HOSTNAME"].iter().find_map(|k| std::env::var(k).ok()).filter(|s| !s.trim().is_empty());
    match from_env {
        Some(n) => n,
        None if cfg!(target_os = "android") => "Android phone".into(),
        None if cfg!(target_os = "ios") => "iPhone".into(),
        None => "Computer".into(),
    }
}

/// Changes whenever this device's notes are replaced wholesale (a backup restored): the other
/// side then sends everything again rather than only what's new.
pub fn epoch(conn: &Connection) -> rusqlite::Result<String> {
    if let Some(e) = db::get_setting(conn, "sync_epoch")? {
        return Ok(e);
    }
    new_epoch(conn)
}

pub fn new_epoch(conn: &Connection) -> rusqlite::Result<String> {
    let e = random_id()[..12].to_string();
    db::set_setting(conn, "sync_epoch", &e)?;
    Ok(e)
}

// ---------- what goes over the wire ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireEntry {
    pub uid: String,
    pub title: String,
    pub body: String,
    /// As stored: ",tag-a,tag-b,"
    pub tags: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub modified_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireTombstone {
    pub uid: String,
    pub deleted_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImageText {
    pub name: String,
    pub text: String,
}

/// A batch of changes from one side.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Changes {
    pub entries: Vec<WireEntry>,
    pub tombstones: Vec<WireTombstone>,
    /// Text read from the screenshots these entries show.
    pub image_texts: Vec<ImageText>,
}

impl Changes {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.tombstones.is_empty()
    }
}

/// Phone → PC.
#[derive(Serialize, Deserialize, Debug)]
pub struct SyncRequest {
    pub device_name: String,
    /// The PC's epoch the phone's `since` belongs to (None on a first sync).
    pub epoch: Option<String>,
    /// The PC's change number the phone has everything up to.
    pub since: i64,
    pub changes: Changes,
}

/// PC → phone.
#[derive(Serialize, Deserialize, Debug)]
pub struct SyncResponse {
    pub epoch: String,
    /// Everything up to here is now on the phone (once it applies `changes`).
    pub cursor: i64,
    /// More changes are waiting: sync again straight away.
    pub more: bool,
    pub changes: Changes,
    /// Screenshots the phone sent entries for that the PC doesn't have yet.
    pub need_images: Vec<String>,
    /// The PC's notes were replaced (a backup restored): send everything again.
    pub resend_all: bool,
    pub pc_name: String,
    /// The PC's current addresses, in case they changed since pairing.
    pub addresses: Vec<String>,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PairRequest {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PairResponse {
    pub pc_id: String,
    pub pc_name: String,
}

// ---------- reading changes ----------

fn wire_entry(r: &rusqlite::Row) -> rusqlite::Result<WireEntry> {
    Ok(WireEntry {
        uid: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        tags: r.get(3)?,
        pinned: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        modified_at: r.get(7)?,
        deleted_at: r.get(8)?,
    })
}

const ENTRY_COLS: &str = "uid, title, body, tags, pinned, created_at, updated_at, modified_at, deleted_at";

/// Screenshot file names an entry body shows.
pub fn images_in(body: &str) -> Vec<String> {
    body.split("](img:")
        .skip(1)
        .map(|p| p.chars().take_while(|c| *c != ')').collect::<String>())
        .filter(|n| !n.is_empty())
        .collect()
}

/// This device's changes after `since` that didn't come from `peer`, oldest first, at most
/// `limit` of them. Returns them and the change number they go up to.
pub fn collect(conn: &Connection, since: i64, peer: &str, limit: usize) -> rusqlite::Result<(Changes, i64, bool)> {
    // Entries and tombstones share one numbering: take the oldest `limit` across both.
    let mut stmt = conn.prepare(
        "SELECT rev, 0 FROM entries WHERE rev > ?1 AND (rev_from IS NULL OR rev_from != ?2)
         UNION ALL
         SELECT rev, 1 FROM sync_tombstones WHERE rev > ?1 AND (rev_from IS NULL OR rev_from != ?2)
         ORDER BY 1 LIMIT ?3",
    )?;
    let revs: Vec<(i64, i64)> = stmt
        .query_map(params![since, peer, limit as i64 + 1], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let more = revs.len() > limit;
    let revs = &revs[..revs.len().min(limit)];
    let upto = match (more, revs.last()) {
        (true, Some((rev, _))) => *rev,
        // Everything sent: up to the newest change, including ones skipped for coming from `peer`.
        _ => clock(conn)?,
    };

    let mut out = Changes::default();
    let mut entry_stmt = conn.prepare(&format!("SELECT {ENTRY_COLS} FROM entries WHERE rev = ?1"))?;
    let mut tomb_stmt = conn.prepare("SELECT uid, deleted_at FROM sync_tombstones WHERE rev = ?1")?;
    let mut names = HashSet::new();
    for (rev, kind) in revs {
        if *kind == 0 {
            for e in entry_stmt.query_map(params![rev], wire_entry)? {
                let e = e?;
                names.extend(images_in(&e.body));
                out.entries.push(e);
            }
        } else {
            for t in tomb_stmt.query_map(params![rev], |r| Ok(WireTombstone { uid: r.get(0)?, deleted_at: r.get(1)? }))? {
                out.tombstones.push(t?);
            }
        }
    }
    let mut text_stmt = conn.prepare("SELECT text FROM image_text WHERE name = ?1")?;
    for name in names {
        if let Some(text) = text_stmt.query_row(params![name], |r| r.get::<_, String>(0)).optional()? {
            out.image_texts.push(ImageText { name, text });
        }
    }
    Ok((out, upto, more))
}

/// The newest change number on this device.
pub fn clock(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT rev FROM sync_clock WHERE id = 1", [], |r| r.get(0))
}

fn next_rev(conn: &Connection) -> rusqlite::Result<i64> {
    conn.execute("UPDATE sync_clock SET rev = rev + 1 WHERE id = 1", [])?;
    clock(conn)
}

// ---------- applying changes ----------

/// What applying a batch did.
#[derive(Default, Debug)]
pub struct Applied {
    /// Entries added, changed or deleted here.
    pub changed: usize,
    /// Entries where this device's version is newer than the one received: the other side
    /// needs it back (it may already have been sent to it earlier).
    pub keep_ours: Vec<String>,
}

/// Orders two versions of an entry: the one changed last wins; a tie (practically never) is
/// broken by the content, so both devices pick the same one.
fn newer(a: &WireEntry, b: &WireEntry) -> bool {
    let key = |e: &WireEntry| (e.modified_at, e.title.clone(), e.body.clone(), e.tags.clone(), e.pinned, e.deleted_at);
    key(a) > key(b)
}

fn same_content(a: &WireEntry, b: &WireEntry) -> bool {
    a.title == b.title && a.body == b.body && a.tags == b.tags && a.pinned == b.pinned && a.deleted_at == b.deleted_at
}

/// Applies changes received from `peer` in one transaction.
pub fn apply(conn: &Connection, peer: &str, changes: &Changes) -> rusqlite::Result<Applied> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let mut out = Applied::default();

    // Screenshot text first, so entries added below are indexed with it.
    for t in &changes.image_texts {
        if crate::safe_image_name(&t.name) {
            db::set_image_text_in(&tx, &t.name, &t.text)?;
        }
    }

    for incoming in &changes.entries {
        let local = tx
            .query_row(&format!("SELECT id, {ENTRY_COLS} FROM entries WHERE uid = ?1"), params![incoming.uid], |r| {
                Ok((r.get::<_, i64>(0)?, WireEntry {
                    uid: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    tags: r.get(4)?,
                    pinned: r.get::<_, i64>(5)? != 0,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                    modified_at: r.get(8)?,
                    deleted_at: r.get(9)?,
                }))
            })
            .optional()?;
        match local {
            None => {
                let gone: Option<i64> = tx
                    .query_row("SELECT deleted_at FROM sync_tombstones WHERE uid = ?1", params![incoming.uid], |r| r.get(0))
                    .optional()?;
                if gone.is_some_and(|at| at >= incoming.modified_at) {
                    continue; // deleted for good here after that edit
                }
                let rev = next_rev(&tx)?;
                tx.execute(
                    "INSERT INTO entries (uid, title, body, tags, pinned, created_at, updated_at, modified_at, deleted_at, rev, rev_from)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        incoming.uid,
                        incoming.title,
                        incoming.body,
                        incoming.tags,
                        incoming.pinned as i64,
                        incoming.created_at,
                        incoming.updated_at,
                        incoming.modified_at,
                        incoming.deleted_at,
                        rev,
                        peer
                    ],
                )?;
                tx.execute("DELETE FROM sync_tombstones WHERE uid = ?1", params![incoming.uid])?;
                out.changed += 1;
            }
            Some((id, mine)) => {
                if same_content(&mine, incoming) && mine.modified_at == incoming.modified_at {
                    continue;
                }
                if !newer(incoming, &mine) {
                    out.keep_ours.push(mine.uid);
                    continue;
                }
                // Keep this device's text in History before it's replaced.
                let content_differs = mine.title != incoming.title || mine.body != incoming.body || mine.tags != incoming.tags;
                if content_differs && !(mine.title.trim().is_empty() && mine.body.trim().is_empty()) {
                    db::snapshot_version(&tx, id, &mine.title, &mine.body, &mine.tags, mine.updated_at)?;
                }
                let rev = next_rev(&tx)?;
                tx.execute(
                    "UPDATE entries SET title = ?2, body = ?3, tags = ?4, pinned = ?5, created_at = ?6, updated_at = ?7,
                            modified_at = ?8, deleted_at = ?9, rev = ?10, rev_from = ?11
                     WHERE id = ?1",
                    params![
                        id,
                        incoming.title,
                        incoming.body,
                        incoming.tags,
                        incoming.pinned as i64,
                        incoming.created_at,
                        incoming.updated_at,
                        incoming.modified_at,
                        incoming.deleted_at,
                        rev,
                        peer
                    ],
                )?;
                out.changed += 1;
            }
        }
    }

    for t in &changes.tombstones {
        let local: Option<(i64, i64)> = tx
            .query_row("SELECT id, modified_at FROM entries WHERE uid = ?1", params![t.uid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let mut purged = false;
        match local {
            // Changed here after it was deleted there: keep it (it goes back to the other side).
            Some((_, modified)) if modified > t.deleted_at => {
                out.keep_ours.push(t.uid.clone());
                continue;
            }
            Some((id, _)) => {
                db::purge_entry(&tx, id)?; // leaves a tombstone (db.rs), marked as from `peer` below
                purged = true;
                out.changed += 1;
            }
            None => {}
        }
        // Keep the tombstone too, to pass on to other devices.
        let known: Option<i64> = tx
            .query_row("SELECT deleted_at FROM sync_tombstones WHERE uid = ?1", params![t.uid], |r| r.get(0))
            .optional()?;
        if purged || known.is_none_or(|k| k < t.deleted_at) {
            let rev = next_rev(&tx)?;
            tx.execute(
                "INSERT INTO sync_tombstones (uid, deleted_at, rev, rev_from) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(uid) DO UPDATE SET deleted_at = max(deleted_at, excluded.deleted_at), rev = excluded.rev,
                                                rev_from = excluded.rev_from",
                params![t.uid, t.deleted_at, rev, peer],
            )?;
        }
    }
    tx.commit()?;
    Ok(out)
}

/// This device's current version of these entries (or their tombstones), to send back to a
/// device that sent an older one.
pub fn current(conn: &Connection, uids: &[String]) -> rusqlite::Result<Changes> {
    let mut out = Changes::default();
    let mut names = HashSet::new();
    for uid in uids {
        let e = conn
            .query_row(&format!("SELECT {ENTRY_COLS} FROM entries WHERE uid = ?1"), params![uid], wire_entry)
            .optional()?;
        if let Some(e) = e {
            names.extend(images_in(&e.body));
            out.entries.push(e);
        }
    }
    for name in names {
        if let Some(text) = conn
            .query_row("SELECT text FROM image_text WHERE name = ?1", params![name], |r| r.get::<_, String>(0))
            .optional()?
        {
            out.image_texts.push(ImageText { name, text });
        }
    }
    Ok(out)
}

/// Adds `extra` to `into`, without sending an entry twice.
pub fn merge_changes(into: &mut Changes, extra: Changes) {
    let have: HashSet<String> = into.entries.iter().map(|e| e.uid.clone()).collect();
    into.entries.extend(extra.entries.into_iter().filter(|e| !have.contains(&e.uid)));
    let names: HashSet<String> = into.image_texts.iter().map(|t| t.name.clone()).collect();
    into.image_texts.extend(extra.image_texts.into_iter().filter(|t| !names.contains(&t.name)));
}

/// Screenshot names these entries show.
pub fn images_of(changes: &Changes) -> Vec<String> {
    let mut seen = HashSet::new();
    changes
        .entries
        .iter()
        .flat_map(|e| images_in(&e.body))
        .filter(|n| crate::safe_image_name(n) && seen.insert(n.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
