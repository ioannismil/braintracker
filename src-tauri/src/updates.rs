//! Auto-update from GitHub Releases (signed with the key in the TAURI_SIGNING_PRIVATE_KEY secret).
//!
//! Installed copies download and run the new installer in the background, then restart.
//! Portable copies only get told that a new version exists (they can't self-install).

use crate::{err, portable_dir, prefs};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const RELEASES_URL: &str = "https://github.com/ioannismil/braintracker/releases/latest";

#[derive(Default)]
pub struct UpdateState(Mutex<Option<Update>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
    /// Portable copies can't install themselves — show a download link instead.
    portable: bool,
    releases_url: String,
}

const OFFLINE_MSG: &str = "Offline mode is on, so BrainTracker doesn’t check for updates.";

pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    if prefs::is_offline(app) {
        return Err(OFFLINE_MSG.into());
    }
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        portable: portable_dir().is_some(),
        releases_url: RELEASES_URL.into(),
    });
    if let Ok(mut slot) = app.state::<UpdateState>().0.lock() {
        *slot = update;
    }
    Ok(info)
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    if portable_dir().is_some() {
        return Err("Portable copies update by downloading the new zip.".into());
    }
    if prefs::is_offline(app) {
        return Err(OFFLINE_MSG.into());
    }
    let update = app
        .state::<UpdateState>()
        .0
        .lock()
        .map_err(err)?
        .clone()
        .ok_or("No update is pending — check again first.")?;
    let _ = app.emit("update-progress", "downloading");
    update.download_and_install(|_, _| {}, || {}).await.map_err(err)?;
    app.restart();
}

/// Checks shortly after launch, then every 6 hours, and tells the main window
/// (skipped while offline mode is on).
pub fn start_checker(app: AppHandle) {
    if cfg!(debug_assertions) {
        return; // dev builds are always "older" than the latest release
    }
    tauri::async_runtime::spawn(async move {
        tokio_sleep(Duration::from_secs(20)).await;
        loop {
            if let Ok(Some(info)) = check(&app).await {
                let _ = app.emit_to("main", "update-available", info);
            }
            tokio_sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

async fn tokio_sleep(d: Duration) {
    // Avoids depending on tokio directly: sleep on a blocking thread.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}
