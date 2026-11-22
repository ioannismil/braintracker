//! The phone side of sync: pairs with the PC from its QR code, then syncs with it whenever the
//! app asks (when it opens, after edits, and every so often while it's open).
//!
//! It finds the PC at the address it last answered on, then the others it reported, and if
//! none answers (the PC got a new address) it asks the network with a UDP broadcast.

use super::crypto::{self, Key};
use super::server::{join_upload, write_new, PairingCode};
use super::*;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

/// Most screenshots copied in one sync (the rest follow next time).
const IMAGES_PER_SYNC: usize = 40;
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;

/// The PC this phone is paired with, and how far the two are in sync.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Link {
    pub pc_id: String,
    pub pc_name: String,
    pub key: String,
    pub addresses: Vec<String>,
    pub port: u16,
    /// The address that answered last time ("ip:port").
    pub last_address: Option<String>,
    /// The PC's epoch and change number this phone has everything up to.
    pub epoch: Option<String>,
    pub cursor: i64,
    /// This phone's change number the PC has everything up to.
    pub sent: i64,
    pub last_sync: Option<i64>,
    pub last_error: Option<String>,
}

const LINK: &str = "sync_pc";

pub fn load_link(conn: &Connection) -> Option<Link> {
    serde_json::from_str(&db::get_setting(conn, LINK).ok()??).ok()
}

pub fn save_link(conn: &Connection, link: &Link) -> rusqlite::Result<()> {
    db::set_setting(conn, LINK, &serde_json::to_string(link).unwrap_or_default())
}

pub fn forget_link(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![LINK]).map(|_| ())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// Is this a "couldn't reach it" failure (try another address) rather than an answer?
enum Failure {
    Unreachable(String),
    Answered(u16, Vec<u8>),
}

fn post(agent: &ureq::Agent, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut req = agent.post(url).header("Content-Type", "application/octet-stream");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.send(body).map_err(|e| Failure::Unreachable(e.to_string()))?;
    let status = resp.status().as_u16();
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE)
        .read_to_vec()
        .map_err(|e| Failure::Unreachable(e.to_string()))?;
    if status == 200 {
        Ok(bytes)
    } else {
        Err(Failure::Answered(status, bytes))
    }
}

// ---------- pairing ----------

/// Pairs with the PC whose code (from its QR code) this is.
pub fn pair(db: &Mutex<Connection>, code_text: &str) -> Result<Link, String> {
    let code = PairingCode::from_text(code_text)
        .ok_or("That isn’t a BrainTracker pairing code. On the PC, open Settings → Phone sync → Pair a phone.")?;
    let key = crypto::key_from_text(&code.key).ok_or("That pairing code is damaged. Make a new one on the PC.")?;
    let my_id = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        device_id(&conn).map_err(|e| e.to_string())?
    };
    let body = crypto::seal(
        &key,
        "/pair:req",
        &serde_json::to_vec(&PairRequest { device_id: my_id, device_name: device_name() }).unwrap_or_default(),
    );
    let agent = agent();
    let mut last = String::from("The PC didn’t answer.");
    for addr in &code.addresses {
        let base = format!("{addr}:{}", code.port);
        match post(&agent, &format!("http://{base}/pair"), &[("X-BT-Pair", code.token.as_str())], &body) {
            Ok(bytes) => {
                let plain = crypto::open(&key, "/pair:resp", &bytes).ok_or("The PC’s answer couldn’t be read.")?;
                let resp: PairResponse = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
                let link = Link {
                    pc_id: resp.pc_id,
                    pc_name: resp.pc_name,
                    key: code.key.clone(),
                    addresses: code.addresses.clone(),
                    port: code.port,
                    last_address: Some(base),
                    ..Default::default()
                };
                let conn = db.lock().map_err(|e| e.to_string())?;
                save_link(&conn, &link).map_err(|e| e.to_string())?;
                return Ok(link);
            }
            Err(Failure::Answered(410, _)) => {
                return Err("That pairing code has expired or was already used. Make a new one on the PC.".into())
            }
            Err(Failure::Answered(code, _)) => return Err(format!("The PC refused to pair (error {code}).")),
            Err(Failure::Unreachable(e)) => last = e,
        }
    }
    let tried = code.addresses.iter().map(|a| format!("{a}:{}", code.port)).collect::<Vec<_>>().join(", ");
    Err(format!(
        "Couldn’t reach the PC at {tried} ({last}).

Check that this phone is on the same Wi-Fi as the PC (its address should start the same way, e.g. 192.168.1.…) and not on mobile data or a guest network. To test, open http://{}:{}/ in the phone’s browser: if it doesn’t load, the network or the PC’s firewall is blocking it.",
        code.addresses.first().map(String::as_str).unwrap_or("the PC"),
        code.port
    ))
}

// ---------- syncing ----------

/// A connection to the paired PC.
struct Session<'a> {
    agent: ureq::Agent,
    link: &'a mut Link,
    key: Key,
    my_id: String,
    base: Option<String>,
}

impl Session<'_> {
    /// Addresses to try, the one that worked last first.
    fn candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = self.link.last_address.iter().cloned().collect();
        for a in &self.link.addresses {
            let c = format!("{a}:{}", self.link.port);
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    fn call(&mut self, path: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
        let body = crypto::seal(&self.key, &format!("{path}:req"), plain);
        let headers = [("X-BT-Peer", self.my_id.clone())];
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut tried = Vec::new();
        let mut last = String::new();
        let mut candidates = match &self.base {
            Some(b) => vec![b.clone()],
            None => self.candidates(),
        };
        let mut discovered = false;
        loop {
            for base in candidates.drain(..) {
                if tried.contains(&base) {
                    continue;
                }
                tried.push(base.clone());
                match post(&self.agent, &format!("http://{base}{path}"), &headers, &body) {
                    Ok(bytes) => {
                        self.base = Some(base.clone());
                        self.link.last_address = Some(base);
                        return crypto::open(&self.key, &format!("{path}:resp"), &bytes)
                            .ok_or_else(|| "The PC’s answer couldn’t be read.".to_string());
                    }
                    Err(Failure::Answered(401, _)) => {
                        return Err("The PC doesn’t recognise this phone any more. Pair again from Settings.".into())
                    }
                    Err(Failure::Answered(status, bytes)) => {
                        let msg = crypto::open(&self.key, &format!("{path}:err"), &bytes)
                            .map(|b| String::from_utf8_lossy(&b).to_string())
                            .unwrap_or_else(|| format!("error {status}"));
                        return Err(format!("The PC couldn’t sync: {msg}"));
                    }
                    Err(Failure::Unreachable(e)) => last = e,
                }
            }
            if discovered {
                break;
            }
            discovered = true;
            if let Some(found) = discover(&self.link.pc_id) {
                candidates.push(found);
            }
            if candidates.is_empty() {
                break;
            }
        }
        let _ = last;
        Err(format!("{} isn’t reachable. Is it on, with BrainTracker running, on the same Wi-Fi as this phone?", self.link.pc_name))
    }
}

/// Syncs with the PC until both sides are up to date. Returns how many notes changed here.
pub fn sync(db: &Mutex<Connection>, images: &Path) -> Result<usize, String> {
    let (mut link, my_id) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let link = load_link(&conn).ok_or("This phone isn’t paired with a PC yet.")?;
        (link, device_id(&conn).map_err(|e| e.to_string())?)
    };
    let result = sync_with(db, images, &mut link, my_id);
    match &result {
        Ok(_) => {
            link.last_sync = Some(db::now_ms());
            link.last_error = None;
        }
        Err(e) => link.last_error = Some(e.clone()),
    }
    // Saved even after a failure: how far it got, and the error to show.
    if let Ok(conn) = db.lock() {
        // Unless the phone was unpaired meanwhile.
        if load_link(&conn).is_some_and(|l| l.pc_id == link.pc_id) {
            let _ = save_link(&conn, &link);
        }
    }
    result
}

fn sync_with(db: &Mutex<Connection>, images: &Path, link: &mut Link, my_id: String) -> Result<usize, String> {
    let key = crypto::key_from_text(&link.key).ok_or("The pairing key is damaged. Pair again from Settings.")?;
    let pc_id = link.pc_id.clone();
    let mut s = Session { agent: agent(), link, key, my_id, base: None };
    let mut changed = 0;
    let mut wanted_images: Vec<String> = Vec::new();

    for _round in 0..200 {
        let (changes, upto, more_mine) = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            collect(&conn, s.link.sent, &pc_id, BATCH).map_err(|e| e.to_string())?
        };
        let req = SyncRequest { device_name: device_name(), epoch: s.link.epoch.clone(), since: s.link.cursor, changes };
        let plain = s.call("/sync", &serde_json::to_vec(&req).unwrap_or_default())?;
        let resp: SyncResponse = serde_json::from_slice(&plain).map_err(|e| format!("The PC’s answer couldn’t be read: {e}"))?;

        let applied = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let applied = apply(&conn, &pc_id, &resp.changes).map_err(|e| e.to_string())?;
            // Newer here than what the PC sent: make sure the PC gets this version.
            for uid in &applied.keep_ours {
                conn.execute("UPDATE sync_clock SET rev = rev + 1 WHERE id = 1", []).map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE entries SET rev = (SELECT rev FROM sync_clock WHERE id = 1), rev_from = NULL WHERE uid = ?1",
                    params![uid],
                )
                .map_err(|e| e.to_string())?;
            }
            applied
        };
        changed += applied.changed;
        wanted_images.extend(images_of(&resp.changes));

        // Screenshots the PC asked for.
        for name in &resp.need_images {
            upload(&mut s, images, name);
        }

        s.link.sent = if resp.resend_all { 0 } else { upto };
        s.link.cursor = resp.cursor;
        s.link.epoch = Some(resp.epoch);
        s.link.pc_name = resp.pc_name;
        if !resp.addresses.is_empty() {
            s.link.addresses = resp.addresses;
        }
        s.link.port = resp.port;
        {
            // Progress is kept after every round, so a lost connection doesn't start over.
            let conn = db.lock().map_err(|e| e.to_string())?;
            let _ = save_link(&conn, s.link);
        }
        if !more_mine && !resp.more && !resp.resend_all && applied.keep_ours.is_empty() {
            break;
        }
    }

    // Screenshots: fetch the ones notes here show but this phone doesn't have yet, and send
    // the PC the ones it's missing.
    let missing_here = super::server::missing_list(db, images)?;
    for name in missing_here.iter().take(IMAGES_PER_SYNC) {
        if let Ok(bytes) = s.call("/image/get", name.as_bytes()) {
            std::fs::create_dir_all(images).map_err(|e| e.to_string())?;
            if write_new(&images.join(name), &bytes).is_ok() {
                changed += usize::from(wanted_images.contains(name) || changed == 0);
            }
        }
    }
    let plain = s.call("/images/missing", b"")?;
    let missing_there: Vec<String> = serde_json::from_slice(&plain).unwrap_or_default();
    for name in missing_there.iter().take(IMAGES_PER_SYNC) {
        upload(&mut s, images, name);
    }
    Ok(changed)
}

fn upload(s: &mut Session, images: &Path, name: &str) {
    if !crate::safe_image_name(name) {
        return;
    }
    if let Ok(bytes) = std::fs::read(images.join(name)) {
        let _ = s.call("/image/put", &join_upload(name, &bytes));
    }
}

/// Asks the local network where the PC with this id is. Returns "ip:port".
pub fn discover(pc_id: &str) -> Option<String> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.set_broadcast(true).ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let msg = format!("BT-DISCOVER {pc_id}");
    let mut targets = vec![std::net::Ipv4Addr::BROADCAST];
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            if let Some(b) = v4.broadcast {
                if !iface.is_loopback() && !targets.contains(&b) {
                    targets.push(b);
                }
            }
        }
    }
    for t in targets {
        let _ = sock.send_to(msg.as_bytes(), (t, DISCOVERY_PORT));
    }
    let mut buf = [0u8; 256];
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    while std::time::Instant::now() < deadline {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { break };
        let reply = String::from_utf8_lossy(&buf[..n]).to_string();
        let mut parts = reply.split_whitespace();
        if parts.next() == Some("BT-HERE") && parts.next() == Some(pc_id) {
            if let Some(port) = parts.next().and_then(|p| p.parse::<u16>().ok()) {
                return Some(format!("{}:{port}", from.ip()));
            }
        }
    }
    None
}

// ---------- in the app ----------

/// Held while a sync runs, so two never overlap.
static SYNC_LOCK: Mutex<()> = Mutex::new(());
static SYNCING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How often the phone syncs by itself while the app is open.
const EVERY: Duration = Duration::from_secs(45);

pub fn is_syncing() -> bool {
    SYNCING.load(std::sync::atomic::Ordering::Relaxed)
}

/// Syncs once if the phone is paired and offline mode is off, then tells the window what
/// changed. Waits for a sync that's already running rather than starting a second one.
pub fn run_once(app: &tauri::AppHandle) -> Result<usize, String> {
    use std::sync::atomic::Ordering;
    use tauri::{Emitter, Manager};
    if crate::prefs::is_offline(app) {
        return Err("Offline mode is on.".into());
    }
    let db = app.try_state::<crate::db::Db>().ok_or("Your notes aren’t open yet.")?.0.clone();
    if db.lock().map(|c| load_link(&c).is_none()).unwrap_or(true) {
        return Err("This phone isn’t paired with a PC yet.".into());
    }
    let images = crate::images_dir(app)?;
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    SYNCING.store(true, Ordering::Relaxed);
    let _ = app.emit("sync-changed", ());
    let result = sync(&db, &images);
    SYNCING.store(false, Ordering::Relaxed);
    if matches!(result, Ok(n) if n > 0) {
        let _ = app.emit("outside-change", ());
    }
    let _ = app.emit("sync-changed", ());
    result
}

/// Syncs when the app opens and then every so often while it's open.
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        let _ = run_once(&app);
        std::thread::sleep(EVERY);
    });
}
