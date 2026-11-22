//! What the windows call: on the PC, Settings → Phone sync (turn it on, pair a phone, remove
//! one); on the phone, pairing from the QR code and syncing.

use crate::{err, CmdResult};
#[cfg(desktop)]
pub use desktop::*;
#[cfg(mobile)]
pub use phone::*;

#[cfg(desktop)]
mod desktop {
    use super::*;
    use crate::db::{self, Db};
    use crate::sync::crypto;
    use crate::sync::server::{self, Pairing, PairingCode, ServerState};
    use crate::sync::{device_id, device_name, random_id, PAIRING_MINUTES};
    use serde::Serialize;
    use tauri::{AppHandle, Manager, State};

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PeerInfo {
        id: String,
        name: String,
        paired_at: i64,
        last_sync: Option<i64>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PairingInfo {
        /// The QR code to scan, as an SVG image.
        qr_svg: String,
        /// The same as text, for typing or pasting into the phone.
        code: String,
        expires_at: i64,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SyncStatus {
        enabled: bool,
        offline: bool,
        running: bool,
        port: Option<u16>,
        addresses: Vec<String>,
        error: Option<String>,
        peers: Vec<PeerInfo>,
        pairing: Option<PairingInfo>,
    }

    fn qr_svg(text: &str) -> Result<String, String> {
        use qrcode::render::svg;
        let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::L).map_err(err)?;
        Ok(code
            .render::<svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build())
    }

    fn pairing_info(conn: &rusqlite::Connection, p: &Pairing, port: u16) -> Result<PairingInfo, String> {
        let code = PairingCode {
            id: device_id(conn).map_err(err)?,
            name: device_name(),
            token: p.token.clone(),
            key: crypto::key_to_text(&p.key),
            port,
            addresses: server::local_addresses(),
        }
        .to_text();
        Ok(PairingInfo { qr_svg: qr_svg(&code)?, code, expires_at: p.expires_at })
    }

    fn status(app: &AppHandle) -> CmdResult<SyncStatus> {
        // Before locking the database: reading this setting locks it too.
        let offline = crate::prefs::is_offline(app);
        let db = app.state::<Db>();
        let state = app.state::<ServerState>();
        let conn = db.0.lock().map_err(err)?;
        let running = state.0.lock().map_err(err)?;
        let host = running.as_ref().map(|r| r.host.clone());
        let pairing = match &host {
            Some(h) => {
                let p = h.pairing.lock().map_err(err)?.clone().filter(|p| p.expires_at > db::now_ms());
                p.map(|p| pairing_info(&conn, &p, h.port)).transpose()?
            }
            None => None,
        };
        let error = state.1.lock().map_err(err)?.clone();
        drop(running);
        Ok(SyncStatus {
            enabled: server::enabled(&conn),
            offline,
            running: host.is_some(),
            port: host.as_ref().map(|h| h.port),
            addresses: if host.is_some() { server::local_addresses() } else { Vec::new() },
            error,
            peers: server::load_peers(&conn)
                .into_iter()
                .map(|p| PeerInfo { id: p.id, name: p.name, paired_at: p.paired_at, last_sync: p.last_sync })
                .collect(),
            pairing,
        })
    }

    #[tauri::command]
    pub fn sync_status(app: AppHandle) -> CmdResult<SyncStatus> {
        status(&app)
    }

    #[tauri::command]
    pub fn set_sync_enabled(app: AppHandle, db: State<Db>, enabled: bool) -> CmdResult<SyncStatus> {
        {
            let conn = db.0.lock().map_err(err)?;
            db::set_setting(&conn, "sync_enabled", if enabled { "1" } else { "0" }).map_err(err)?;
        }
        server::refresh(&app);
        status(&app)
    }

    /// Shows a new pairing code (turning sync on if it was off).
    #[tauri::command]
    pub fn start_pairing(app: AppHandle, db: State<Db>) -> CmdResult<SyncStatus> {
        if crate::prefs::is_offline(&app) {
            return Err("Offline mode is on: turn it off to sync with a phone.".into());
        }
        {
            let conn = db.0.lock().map_err(err)?;
            db::set_setting(&conn, "sync_enabled", "1").map_err(err)?;
        }
        server::refresh(&app);
        {
            let state = app.state::<ServerState>();
            let running = state.0.lock().map_err(err)?;
            let Some(r) = running.as_ref() else {
                let e = state.1.lock().map_err(err)?.clone();
                return Err(e.unwrap_or_else(|| "Sync couldn’t start.".into()));
            };
            *r.host.pairing.lock().map_err(err)? = Some(Pairing {
                token: random_id(),
                key: crypto::new_key(),
                expires_at: db::now_ms() + PAIRING_MINUTES * 60_000,
            });
        }
        status(&app)
    }

    #[tauri::command]
    pub fn cancel_pairing(app: AppHandle) -> CmdResult<SyncStatus> {
        if let Some(r) = app.state::<ServerState>().0.lock().map_err(err)?.as_ref() {
            *r.host.pairing.lock().map_err(err)? = None;
        }
        status(&app)
    }

    /// Forgets a phone: its key stops working, so it can't sync until it's paired again.
    #[tauri::command]
    pub fn remove_peer(app: AppHandle, db: State<Db>, id: String) -> CmdResult<SyncStatus> {
        {
            let conn = db.0.lock().map_err(err)?;
            let mut peers = server::load_peers(&conn);
            peers.retain(|p| p.id != id);
            server::save_peers(&conn, &peers).map_err(err)?;
        }
        status(&app)
    }
}

#[cfg(mobile)]
mod phone {
    use super::*;
    use crate::db::Db;
    use crate::sync::client;
    use serde::Serialize;
    use tauri::{AppHandle, Manager};

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SyncStatus {
        paired: bool,
        pc_name: Option<String>,
        last_sync: Option<i64>,
        last_error: Option<String>,
        syncing: bool,
        offline: bool,
    }

    fn status(app: &AppHandle) -> CmdResult<SyncStatus> {
        let db = app.try_state::<Db>().ok_or("Your notes aren’t open yet.")?;
        let link = client::load_link(&*db.0.lock().map_err(err)?);
        Ok(SyncStatus {
            paired: link.is_some(),
            pc_name: link.as_ref().map(|l| l.pc_name.clone()),
            last_sync: link.as_ref().and_then(|l| l.last_sync),
            last_error: link.and_then(|l| l.last_error),
            syncing: client::is_syncing(),
            offline: crate::prefs::is_offline(app),
        })
    }

    #[tauri::command]
    pub fn sync_status(app: AppHandle) -> CmdResult<SyncStatus> {
        status(&app)
    }

    /// Pairs with the PC whose QR code was scanned (or pasted), then syncs straight away.
    #[tauri::command]
    pub async fn pair_with(app: AppHandle, code: String) -> CmdResult<SyncStatus> {
        let db = app.try_state::<Db>().ok_or("Your notes aren’t open yet.")?.0.clone();
        tauri::async_runtime::spawn_blocking(move || client::pair(&db, &code)).await.map_err(err)??;
        let _ = client::run_once(&app);
        status(&app)
    }

    /// Syncs now (waits for one already running). Errors are kept in the status, not returned.
    #[tauri::command]
    pub async fn sync_now(app: AppHandle) -> CmdResult<SyncStatus> {
        let a = app.clone();
        tauri::async_runtime::spawn_blocking(move || client::run_once(&a)).await.map_err(err)?.ok();
        status(&app)
    }

    #[tauri::command]
    pub fn unpair(app: AppHandle) -> CmdResult<SyncStatus> {
        {
            let db = app.try_state::<Db>().ok_or("Your notes aren’t open yet.")?;
            client::forget_link(&*db.0.lock().map_err(err)?).map_err(err)?;
        }
        status(&app)
    }
}
