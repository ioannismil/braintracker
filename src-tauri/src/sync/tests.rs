//! Sync between two databases over a real HTTP server on 127.0.0.1: the PC side (server.rs)
//! and the phone side (client.rs), pairing included.

use super::client;
use super::crypto;
use super::server::{self, listen, Pairing, PairingCode, Running};
use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct Device {
    db: Arc<Mutex<Connection>>,
    images: PathBuf,
}

impl Device {
    fn new() -> Self {
        let images = std::env::temp_dir().join(format!("bt-sync-test-{}", random_id()));
        std::fs::create_dir_all(&images).unwrap();
        Device { db: Arc::new(Mutex::new(db::open(Path::new(":memory:")).unwrap())), images }
    }
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap()
    }
    fn add(&self, title: &str, body: &str) -> i64 {
        db::create_entry(&self.conn(), title, body, &[]).unwrap().id
    }
    fn id_of(&self, title: &str) -> Option<i64> {
        self.conn().query_row("SELECT id FROM entries WHERE title = ?1", params![title], |r| r.get(0)).optional().unwrap()
    }
    fn body_of(&self, title: &str) -> Option<String> {
        self.conn().query_row("SELECT body FROM entries WHERE title = ?1", params![title], |r| r.get(0)).optional().unwrap()
    }
    fn count(&self) -> i64 {
        self.conn().query_row("SELECT count(*) FROM entries", [], |r| r.get(0)).unwrap()
    }
    fn edit(&self, id: i64, title: &str, body: &str) {
        db::save_entry(&self.conn(), id, title, body, &[], None, false).unwrap();
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.images);
    }
}

/// A PC with its server running, and a phone paired with it.
fn paired() -> (Device, Running, Device) {
    let pc = Device::new();
    let running = listen(pc.db.clone(), pc.images.clone(), Box::new(|_| {}), "127.0.0.1", false).unwrap();
    let phone = Device::new();
    let code = pairing_code(&pc, &running);
    client::pair(&phone.db, &code).unwrap();
    (pc, running, phone)
}

fn pairing_code(pc: &Device, running: &Running) -> String {
    let key = crypto::new_key();
    let token = random_id();
    *running.host.pairing.lock().unwrap() = Some(Pairing { token: token.clone(), key, expires_at: db::now_ms() + 60_000 });
    PairingCode {
        id: device_id(&pc.conn()).unwrap(),
        name: "Test PC".into(),
        token,
        key: crypto::key_to_text(&key),
        port: running.host.port,
        addresses: vec!["127.0.0.1".into()],
    }
    .to_text()
}

fn sync(phone: &Device) -> usize {
    client::sync(&phone.db, &phone.images).unwrap()
}

#[test]
fn pairing_code_round_trips() {
    let code = PairingCode {
        id: "a".into(),
        name: "PC".into(),
        token: "t".into(),
        key: "k".into(),
        port: 1,
        addresses: vec!["192.168.1.2".into()],
    };
    assert_eq!(PairingCode::from_text(&format!("  {}\n", code.to_text())), Some(code));
    assert_eq!(PairingCode::from_text("hello"), None);
}

#[test]
fn notes_go_both_ways() {
    let (pc, _server, phone) = paired();
    pc.add("from pc", "written on the PC");
    phone.add("from phone", "written on the phone");
    sync(&phone);
    assert_eq!(phone.body_of("from pc").as_deref(), Some("written on the PC"));
    assert_eq!(pc.body_of("from phone").as_deref(), Some("written on the phone"));
    assert_eq!((pc.count(), phone.count()), (2, 2));

    // Nothing new: nothing changes, and nothing is duplicated.
    assert_eq!(sync(&phone), 0);
    assert_eq!((pc.count(), phone.count()), (2, 2));

    // An edit on either side arrives on the other.
    let id = pc.id_of("from phone").unwrap();
    pc.edit(id, "from phone", "edited on the PC");
    sync(&phone);
    assert_eq!(phone.body_of("from phone").as_deref(), Some("edited on the PC"));
    let id = phone.id_of("from pc").unwrap();
    phone.edit(id, "from pc", "edited on the phone");
    sync(&phone);
    assert_eq!(pc.body_of("from pc").as_deref(), Some("edited on the phone"));
}

#[test]
fn search_finds_synced_notes() {
    let (pc, _server, phone) = paired();
    pc.add("kubectl", "rollout restart deployment");
    sync(&phone);
    let hits = db::list_entries(&phone.conn(), Some("rollout"), None).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn the_later_edit_wins_and_the_other_is_kept_in_history() {
    let (pc, _server, phone) = paired();
    let pc_id = pc.add("shared", "original");
    sync(&phone);
    let phone_id = phone.id_of("shared").unwrap();

    phone.edit(phone_id, "shared", "phone version");
    std::thread::sleep(std::time::Duration::from_millis(5));
    pc.edit(pc_id, "shared", "pc version"); // later
    sync(&phone);
    assert_eq!(phone.body_of("shared").as_deref(), Some("pc version"));
    assert_eq!(pc.body_of("shared").as_deref(), Some("pc version"));
    let kept = db::list_versions(&phone.conn(), phone_id).unwrap();
    assert!(kept.iter().any(|v| v.snippet.contains("phone version")), "the phone's version is in its History");
}

#[test]
fn trash_and_deleting_for_good_carry_over() {
    let (pc, _server, phone) = paired();
    let a = pc.add("trash me", "x");
    let b = pc.add("delete me", "y");
    sync(&phone);
    assert_eq!(phone.count(), 2);

    db::trash_entry(&pc.conn(), a).unwrap();
    db::purge_entry(&pc.conn(), b).unwrap();
    sync(&phone);
    assert_eq!(db::trash_count(&phone.conn()).unwrap(), 1);
    assert_eq!(phone.id_of("delete me"), None);

    // Deleted on the phone, for good: gone from the PC too.
    let a_phone = phone.id_of("trash me").unwrap();
    db::purge_entry(&phone.conn(), a_phone).unwrap();
    sync(&phone);
    assert_eq!(pc.count(), 0);
}

#[test]
fn an_edit_after_a_delete_brings_the_note_back() {
    let (pc, _server, phone) = paired();
    let id = pc.add("keep", "v1");
    sync(&phone);
    let phone_id = phone.id_of("keep").unwrap();
    db::purge_entry(&pc.conn(), id).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    phone.edit(phone_id, "keep", "v2, edited after the PC deleted it");
    sync(&phone);
    assert_eq!(phone.body_of("keep").as_deref(), Some("v2, edited after the PC deleted it"));
    assert_eq!(pc.body_of("keep").as_deref(), Some("v2, edited after the PC deleted it"));
}

#[test]
fn screenshots_are_copied_both_ways() {
    let (pc, _server, phone) = paired();
    std::fs::write(pc.images.join("1-0-aaaaaa.png"), b"pc image").unwrap();
    pc.add("pc shot", "![](img:1-0-aaaaaa.png)");
    db::set_image_text(&pc.conn(), "1-0-aaaaaa.png", "ERR_CONNECTION_REFUSED").unwrap();
    std::fs::write(phone.images.join("2-0-bbbbbb.jpg"), b"phone image").unwrap();
    phone.add("phone shot", "![](img:2-0-bbbbbb.jpg)");
    sync(&phone);
    assert_eq!(std::fs::read(phone.images.join("1-0-aaaaaa.png")).unwrap(), b"pc image");
    assert_eq!(std::fs::read(pc.images.join("2-0-bbbbbb.jpg")).unwrap(), b"phone image");
    // The text read from the PC's screenshot is searchable on the phone.
    assert_eq!(db::list_entries(&phone.conn(), Some("ERR_CONNECTION_REFUSED"), None).unwrap().len(), 1);
}

#[test]
fn a_big_first_sync_takes_several_rounds() {
    let (pc, _server, phone) = paired();
    for i in 0..(BATCH * 2 + 7) {
        pc.add(&format!("pc {i}"), "x");
    }
    for i in 0..(BATCH + 3) {
        phone.add(&format!("phone {i}"), "y");
    }
    sync(&phone);
    let total = (BATCH * 3 + 10) as i64;
    assert_eq!((pc.count(), phone.count()), (total, total));
}

#[test]
fn a_restored_backup_on_the_pc_gets_everything_again() {
    let (pc, _server, phone) = paired();
    phone.add("on the phone", "x");
    sync(&phone);
    // As if the PC restored an older backup: the note is gone and the epoch is new.
    let id = pc.id_of("on the phone").unwrap();
    pc.conn().execute("DELETE FROM entries WHERE id = ?1", params![id]).unwrap();
    pc.conn().execute("DELETE FROM sync_tombstones", []).unwrap();
    new_epoch(&pc.conn()).unwrap();
    sync(&phone);
    assert_eq!(pc.body_of("on the phone").as_deref(), Some("x"));
}

#[test]
fn pairing_codes_work_once_and_only_with_the_right_key() {
    let pc = Device::new();
    let running = listen(pc.db.clone(), pc.images.clone(), Box::new(|_| {}), "127.0.0.1", false).unwrap();
    let code = pairing_code(&pc, &running);
    let phone = Device::new();
    client::pair(&phone.db, &code).unwrap();
    let second = Device::new();
    let e = client::pair(&second.db, &code).unwrap_err();
    assert!(e.contains("expired"), "{e}");

    // A phone with the wrong key is refused.
    let mut link = client::load_link(&phone.conn()).unwrap();
    link.key = crypto::key_to_text(&crypto::new_key());
    client::save_link(&phone.conn(), &link).unwrap();
    let e = client::sync(&phone.db, &phone.images).unwrap_err();
    assert!(e.contains("recognise"), "{e}");

    // A phone removed on the PC is refused too.
    let other = Device::new();
    client::pair(&other.db, &pairing_code(&pc, &running)).unwrap();
    server::save_peers(&pc.conn(), &[]).unwrap();
    assert!(client::sync(&other.db, &other.images).is_err());
}

#[test]
fn changes_made_by_sync_are_not_sent_back() {
    let (pc, _server, phone) = paired();
    pc.add("a", "1");
    sync(&phone);
    // The phone's copy came from the PC: it isn't offered back to it.
    let conn = phone.conn();
    let pc_id = client::load_link(&conn).unwrap().pc_id;
    let (changes, _, _) = collect(&conn, 0, &pc_id, BATCH).unwrap();
    assert!(changes.is_empty());
}
