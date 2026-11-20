//! Small preferences: offline mode, and the fill-in blanks (`{{host}}`) in saved commands.

use crate::{db, db::Db, err};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use tauri::{AppHandle, Manager};

const OFFLINE: &str = "offline";
const PLACEHOLDERS: &str = "placeholders";

fn get(app: &AppHandle, key: &str) -> Option<String> {
    let db = app.state::<Db>();
    let conn = db.0.lock().ok()?;
    db::get_setting(&conn, key).ok().flatten()
}

fn set(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(err)?;
    db::set_setting(&conn, key, value).map_err(err)
}

// ---------- offline mode ----------

/// Offline mode: BrainTracker makes no network connections at all (no update checks, and
/// web images in notes aren't loaded).
pub fn is_offline(app: &AppHandle) -> bool {
    get(app, OFFLINE).as_deref() == Some("1")
}

pub fn set_offline(app: &AppHandle, on: bool) -> Result<(), String> {
    set(app, OFFLINE, if on { "1" } else { "0" })
}

// ---------- command placeholders ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaceholderConfig {
    /// Copying a code block with {{blanks}} asks for them first.
    pub ask: bool,
    /// Remember what was typed, to suggest it next time.
    pub remember: bool,
    /// Last value typed for each blank, by name.
    pub values: BTreeMap<String, String>,
}

impl Default for PlaceholderConfig {
    fn default() -> Self {
        Self { ask: true, remember: true, values: BTreeMap::new() }
    }
}

/// Blanks that look like secrets are never remembered.
pub fn is_sensitive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["pass", "pwd", "secret", "token", "key", "credential", "auth"].iter().any(|w| n.contains(w))
}

pub fn placeholders(app: &AppHandle) -> PlaceholderConfig {
    get(app, PLACEHOLDERS).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn set_placeholders(app: &AppHandle, mut cfg: PlaceholderConfig) -> Result<PlaceholderConfig, String> {
    cfg.values.retain(|k, v| !k.trim().is_empty() && !v.is_empty() && !is_sensitive(k));
    set(app, PLACEHOLDERS, &serde_json::to_string(&cfg).map_err(err)?)?;
    Ok(cfg)
}

/// Remembers the values just typed (if remembering is on).
pub fn remember(app: &AppHandle, values: HashMap<String, String>) -> Result<(), String> {
    let mut cfg = placeholders(app);
    if !cfg.remember {
        return Ok(());
    }
    cfg.values.extend(merge(values));
    set_placeholders(app, cfg).map(|_| ())
}

fn merge(values: HashMap<String, String>) -> impl Iterator<Item = (String, String)> {
    values
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v))
        .filter(|(k, v)| !k.is_empty() && !v.trim().is_empty() && !is_sensitive(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_never_kept() {
        for n in ["password", "DB_PASS", "apiKey", "token", "client_secret", "pwd"] {
            assert!(is_sensitive(n), "{n}");
        }
        for n in ["host", "user", "port", "namespace", "vm"] {
            assert!(!is_sensitive(n), "{n}");
        }
        let kept: Vec<_> = merge(HashMap::from([
            ("host".to_string(), "db01".to_string()),
            ("password".to_string(), "hunter2".to_string()),
            ("empty".to_string(), " ".to_string()),
        ]))
        .collect();
        assert_eq!(kept, vec![("host".to_string(), "db01".to_string())]);
    }

    #[test]
    fn defaults_ask_and_remember() {
        let c: PlaceholderConfig = serde_json::from_str("{}").unwrap();
        assert!(c.ask && c.remember && c.values.is_empty());
    }
}
