//! The PC side of phone sync: a small HTTP server on the home network that paired phones
//! sync with, plus a UDP responder so a phone can find the PC again when its address changes.
//!
//! It only runs while "Sync with your phone" is on in Settings (and offline mode is off).
//! Every request must be encrypted with a paired phone's key (or, while pairing, the key in
//! the QR code on screen); anything else is refused without being read.

use super::crypto::{self, Key};
use super::*;
use std::io::Read;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Largest request accepted (a screenshot uploaded from the phone).
const MAX_BODY: u64 = 64 * 1024 * 1024;

/// A phone paired with this PC.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub name: String,
    /// The shared key, base64url.
    pub key: String,
    pub paired_at: i64,
    pub last_sync: Option<i64>,
}

pub fn load_peers(conn: &Connection) -> Vec<Peer> {
    db::get_setting(conn, "sync_peers")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_peers(conn: &Connection, peers: &[Peer]) -> rusqlite::Result<()> {
    db::set_setting(conn, "sync_peers", &serde_json::to_string(peers).unwrap_or_else(|_| "[]".into()))
}

/// A pairing code on screen, waiting for a phone to scan it.
#[derive(Clone)]
pub struct Pairing {
    pub token: String,
    pub key: Key,
    pub expires_at: i64,
}

/// What the QR code holds (as `BTSYNC1:` + base64url JSON).
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct PairingCode {
    /// The PC's device id and name
    pub id: String,
    pub name: String,
    pub token: String,
    pub key: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

pub const CODE_PREFIX: &str = "BTSYNC1:";

impl PairingCode {
    pub fn to_text(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        format!("{CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).unwrap_or_default()))
    }

    pub fn from_text(text: &str) -> Option<Self> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let t = text.trim();
        let rest = t.strip_prefix(CODE_PREFIX).or_else(|| t.strip_prefix(&CODE_PREFIX.to_lowercase()))?;
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(rest.trim()).ok()?).ok()
    }
}

/// Something the app should know about.
pub enum Event {
    /// Notes changed: windows reload.
    Changed,
    /// A phone paired, or synced (Settings shows when).
    Status,
    /// A screenshot arrived from the phone (read its text).
    Image(String),
}

/// What the server works with. Separate from the Tauri app so the tests can run one.
pub struct Host {
    pub db: Arc<Mutex<Connection>>,
    pub images: PathBuf,
    pub port: u16,
    pub pairing: Mutex<Option<Pairing>>,
    pub on_event: Box<dyn Fn(Event) + Send + Sync>,
}

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok(body: Vec<u8>) -> Self {
        Reply { status: 200, body }
    }
    fn status(status: u16) -> Self {
        Reply { status, body: Vec::new() }
    }
}

/// This PC's addresses on the local network, the likeliest home-network ones first.
pub fn local_addresses() -> Vec<String> {
    let mut v4: Vec<std::net::Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(a) if !a.is_link_local() && !a.is_unspecified() => Some(a),
            _ => None,
        })
        .collect();
    let rank = |a: &std::net::Ipv4Addr| match a.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, b, ..] if (16..32).contains(&b) => 2,
        _ => 3,
    };
    v4.sort_by_key(|a| (rank(a), *a));
    v4.dedup();
    v4.iter().map(|a| a.to_string()).collect()
}

fn peer_key(conn: &Connection, id: &str) -> Option<(Peer, Key)> {
    let peer = load_peers(conn).into_iter().find(|p| p.id == id)?;
    let key = crypto::key_from_text(&peer.key)?;
    Some((peer, key))
}

fn json<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_default()
}

/// Handles one request. `peer` and `pair` are the X-BT-Peer / X-BT-Pair headers.
pub fn handle(host: &Host, path: &str, peer: Option<&str>, pair: Option<&str>, body: &[u8]) -> Reply {
    match (path, peer, pair) {
        ("/pair", _, Some(token)) => pair_phone(host, token, body),
        (_, Some(id), _) => {
            let found = match host.db.lock() {
                Ok(conn) => peer_key(&conn, id),
                Err(_) => return Reply::status(500),
            };
            let Some((peer, key)) = found else { return Reply::status(401) };
            let label = format!("{path}:req");
            let Some(plain) = crypto::open(&key, &label, body) else { return Reply::status(401) };
            let reply = match path {
                "/sync" => sync(host, &peer, &plain),
                "/image/get" => get_image(host, &plain),
                "/image/put" => put_image(host, &plain),
                "/images/missing" => missing_images(host),
                _ => return Reply::status(404),
            };
            match reply {
                Ok(r) if r.status == 200 => Reply::ok(crypto::seal(&key, &format!("{path}:resp"), &r.body)),
                Ok(r) => r,
                Err(e) => Reply { status: 500, body: crypto::seal(&key, &format!("{path}:err"), e.as_bytes()) },
            }
        }
        _ => Reply::status(401),
    }
}

fn pair_phone(host: &Host, token: &str, body: &[u8]) -> Reply {
    let pending = host.pairing.lock().ok().and_then(|p| p.clone());
    let Some(p) = pending.filter(|p| p.token == token && p.expires_at > db::now_ms()) else {
        return Reply::status(410); // no such code, or it expired
    };
    let Some(plain) = crypto::open(&p.key, "/pair:req", body) else { return Reply::status(401) };
    let Ok(req) = serde_json::from_slice::<PairRequest>(&plain) else { return Reply::status(400) };
    let Ok(conn) = host.db.lock() else { return Reply::status(500) };
    let mut peers = load_peers(&conn);
    peers.retain(|x| x.id != req.device_id); // pairing again replaces the old key
    peers.push(Peer {
        id: req.device_id,
        name: req.device_name.chars().take(60).collect(),
        key: crypto::key_to_text(&p.key),
        paired_at: db::now_ms(),
        last_sync: None,
    });
    let (Ok(()), Ok(pc_id)) = (save_peers(&conn, &peers), device_id(&conn)) else { return Reply::status(500) };
    drop(conn);
    if let Ok(mut slot) = host.pairing.lock() {
        *slot = None; // each code pairs one phone
    }
    (host.on_event)(Event::Status);
    Reply::ok(crypto::seal(&p.key, "/pair:resp", &json(&PairResponse { pc_id, pc_name: device_name() })))
}

fn sync(host: &Host, peer: &Peer, plain: &[u8]) -> Result<Reply, String> {
    let req: SyncRequest = serde_json::from_slice(plain).map_err(|e| format!("bad request: {e}"))?;
    let conn = host.db.lock().map_err(|e| e.to_string())?;
    let epoch = epoch(&conn).map_err(|e| e.to_string())?;
    let same_epoch = req.epoch.as_deref() == Some(epoch.as_str());
    let since = if same_epoch { req.since } else { 0 };

    // What the phone hasn't seen, read before applying what it sent (that's marked as coming
    // from it, so it wouldn't be sent back anyway).
    let (mut out, cursor, more) = collect(&conn, since, &peer.id, BATCH).map_err(|e| e.to_string())?;
    let applied = apply(&conn, &peer.id, &req.changes).map_err(|e| e.to_string())?;
    // Where this PC's version is newer than the one the phone sent, send it back.
    let ours = current(&conn, &applied.keep_ours).map_err(|e| e.to_string())?;
    merge_changes(&mut out, ours);

    let mut peers = load_peers(&conn);
    if let Some(p) = peers.iter_mut().find(|p| p.id == peer.id) {
        p.last_sync = Some(db::now_ms());
        if !req.device_name.trim().is_empty() {
            p.name = req.device_name.chars().take(60).collect();
        }
    }
    let _ = save_peers(&conn, &peers);
    drop(conn);

    let need_images = images_of(&req.changes).into_iter().filter(|n| !host.images.join(n).exists()).collect();
    if applied.changed > 0 {
        (host.on_event)(Event::Changed);
    }
    (host.on_event)(Event::Status);
    let resp = SyncResponse {
        epoch,
        cursor,
        more,
        changes: out,
        need_images,
        // The phone's `since` was for notes this PC no longer has (a backup was restored), so
        // what it sent before may be missing here too.
        resend_all: req.epoch.is_some() && !same_epoch,
        pc_name: device_name(),
        addresses: local_addresses(),
        port: host.port,
    };
    Ok(Reply::ok(json(&resp)))
}

fn get_image(host: &Host, plain: &[u8]) -> Result<Reply, String> {
    let name = String::from_utf8_lossy(plain).to_string();
    if !crate::safe_image_name(&name) {
        return Ok(Reply::status(400));
    }
    match std::fs::read(host.images.join(&name)) {
        Ok(bytes) => Ok(Reply::ok(bytes)),
        Err(_) => Ok(Reply::status(404)),
    }
}

/// Splits an image upload: a 2-byte name length, the name, then the file.
pub fn split_upload(plain: &[u8]) -> Option<(String, &[u8])> {
    let len = u16::from_be_bytes([*plain.first()?, *plain.get(1)?]) as usize;
    let name = std::str::from_utf8(plain.get(2..2 + len)?).ok()?.to_string();
    Some((name, &plain[2 + len..]))
}

pub fn join_upload(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + name.len() + bytes.len());
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(bytes);
    out
}

fn put_image(host: &Host, plain: &[u8]) -> Result<Reply, String> {
    let Some((name, bytes)) = split_upload(plain) else { return Ok(Reply::status(400)) };
    if !crate::safe_image_name(&name) {
        return Ok(Reply::status(400));
    }
    let path = host.images.join(&name);
    if !path.exists() {
        std::fs::create_dir_all(&host.images).map_err(|e| e.to_string())?;
        write_new(&path, bytes)?;
        (host.on_event)(Event::Image(name));
        (host.on_event)(Event::Changed); // the notes showing it can now display it
    }
    Ok(Reply::ok(Vec::new()))
}

/// Writes a file via a temporary name, so a half-received image never appears.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Screenshots notes here show but the images folder doesn't have (the phone may have them).
pub fn missing_list(db: &Mutex<Connection>, images: &Path) -> Result<Vec<String>, String> {
    let names = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        db::referenced_images(&conn).map_err(|e| e.to_string())?
    };
    let mut out: Vec<String> =
        names.into_iter().filter(|n| crate::safe_image_name(n) && !images.join(n).exists()).collect();
    out.sort();
    Ok(out)
}

fn missing_images(host: &Host) -> Result<Reply, String> {
    Ok(Reply::ok(json(&missing_list(&host.db, &host.images)?)))
}

// ---------- running it ----------

/// The running server, so Settings can stop it, show its port, or start pairing.
pub struct Running {
    pub host: Arc<Host>,
    server: Arc<tiny_http::Server>,
    udp: Option<Arc<UdpSocket>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(u) = &self.udp {
            // Wakes the discovery thread so it notices it should stop.
            let _ = u.send_to(b"BT-STOP", ("127.0.0.1", DISCOVERY_PORT));
        }
    }
}

/// Starts listening on the first free port from DEFAULT_PORT (any port if none is).
pub fn listen(
    db: Arc<Mutex<Connection>>,
    images: PathBuf,
    on_event: Box<dyn Fn(Event) + Send + Sync>,
    bind: &str,
    with_discovery: bool,
) -> Result<Running, String> {
    let mut server = None;
    for port in (DEFAULT_PORT..DEFAULT_PORT + 10).chain([0]) {
        if let Ok(s) = tiny_http::Server::http((bind, port)) {
            server = Some(s);
            break;
        }
    }
    let server = Arc::new(server.ok_or("Couldn’t open a network port for sync")?);
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("Couldn’t open a network port for sync")?;
    let host = Arc::new(Host { db, images, port, pairing: Mutex::new(None), on_event });

    let (srv, h) = (server.clone(), host.clone());
    std::thread::spawn(move || {
        for mut request in srv.incoming_requests() {
            let header = |name: &'static str| {
                request.headers().iter().find(|x| x.field.equiv(name)).map(|x| x.value.as_str().to_string())
            };
            let (peer, pair) = (header("X-BT-Peer"), header("X-BT-Pair"));
            let path = request.url().split('?').next().unwrap_or("").to_string();
            let reply = if *request.method() == tiny_http::Method::Get && path == "/" {
                // So a phone's browser can check that it reaches this PC at all.
                Reply::ok("BrainTracker on this PC can be reached from here. Pair from the BrainTracker phone app.".into())
            } else if *request.method() != tiny_http::Method::Post {
                Reply::status(405)
            } else {
                let mut body = Vec::new();
                match request.as_reader().take(MAX_BODY).read_to_end(&mut body) {
                    Ok(_) => handle(&h, &path, peer.as_deref(), pair.as_deref(), &body),
                    Err(_) => Reply::status(400),
                }
            };
            let _ = request.respond(tiny_http::Response::from_data(reply.body).with_status_code(reply.status));
        }
    });

    let udp = if with_discovery { start_discovery(&host) } else { None };
    Ok(Running { host, server, udp })
}

/// Answers "where are you?" broadcasts from paired phones with this PC's sync port.
fn start_discovery(host: &Arc<Host>) -> Option<Arc<UdpSocket>> {
    let sock = Arc::new(UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).ok()?);
    let (s, h) = (sock.clone(), host.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = s.recv_from(&mut buf) {
            if Arc::strong_count(&s) == 1 {
                break; // the server was stopped
            }
            let msg = String::from_utf8_lossy(&buf[..n]).to_string();
            let Some(wanted) = msg.strip_prefix("BT-DISCOVER ") else { continue };
            let mine = h.db.lock().ok().and_then(|c| device_id(&c).ok());
            if mine.as_deref() == Some(wanted.trim()) {
                let _ = s.send_to(format!("BT-HERE {} {}", wanted.trim(), h.port).as_bytes(), from);
            }
        }
    });
    Some(sock)
}

// ---------- in the app ----------

pub struct ServerState(pub Mutex<Option<Running>>, pub Mutex<Option<String>>);

pub fn enabled(conn: &Connection) -> bool {
    db::get_setting(conn, "sync_enabled").ok().flatten().as_deref() == Some("1")
}

/// Starts the server if sync is on (called at startup and when the setting changes).
pub fn start(app: tauri::AppHandle) {
    use tauri::Manager;
    app.manage(ServerState(Mutex::new(None), Mutex::new(None)));
    refresh(&app);
}

/// Starts or stops the server to match the setting and offline mode.
pub fn refresh(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let Some(state) = app.try_state::<ServerState>() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    let on = db.0.lock().map(|c| enabled(&c)).unwrap_or(false); // (unlocked again before is_offline locks it)
    let want = on && !crate::prefs::is_offline(app);
    let Ok(mut running) = state.0.lock() else { return };
    if !want {
        *running = None;
        return;
    }
    if running.is_some() {
        return;
    }
    let images = match crate::images_dir(app) {
        Ok(d) => d,
        Err(e) => {
            set_error(app, Some(e));
            return;
        }
    };
    let handle = app.clone();
    let on_event = Box::new(move |e: Event| match e {
        Event::Changed => {
            let _ = handle.emit("outside-change", ());
        }
        Event::Status => {
            let _ = handle.emit("sync-changed", ());
        }
        Event::Image(name) => crate::ocr::queue(&handle, &name),
    });
    match listen(db.0.clone(), images, on_event, "0.0.0.0", true) {
        Ok(r) => {
            *running = Some(r);
            set_error(app, None);
        }
        Err(e) => set_error(app, Some(e)),
    }
}

fn set_error(app: &tauri::AppHandle, e: Option<String>) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<ServerState>() {
        if let Ok(mut slot) = state.1.lock() {
            *slot = e;
        }
    }
}
