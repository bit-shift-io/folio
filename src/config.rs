//! Persistence for the preview pane's "open with" choices, following the
//! shared-config scheme grit uses (`$XDG_CONFIG_HOME/bitshift/<app>/config.json`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bind host used when the config file says nothing: loopback only.
pub fn default_host() -> String {
    "127.0.0.1".to_string()
}

/// Folio's persisted configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolioConfig {
    /// Host to bind. `127.0.0.1` keeps folio on this machine; `0.0.0.0` exposes
    /// the whole filesystem to the network. Maintained by Grit's settings page.
    #[serde(default = "default_host")]
    pub host: String,

    /// Last-used editor per MIME type: `mime -> .desktop id`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_app: BTreeMap<String, String>,
}

impl Default for FolioConfig {
    fn default() -> Self {
        FolioConfig {
            host: default_host(),
            last_app: BTreeMap::new(),
        }
    }
}

/// Config folder per the XDG base directory spec: `$XDG_CONFIG_HOME` (default
/// `~/.config`) plus `bitshift/folio`.
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("bitshift").join("folio"))
}

/// Path of the folio configuration file.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

/// Loads the config from the default location (empty defaults when absent).
pub fn load() -> FolioConfig {
    config_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Loads the config from a specific file (defaults when absent or invalid).
pub fn load_from(path: &Path) -> FolioConfig {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!("failed to parse config at {}: {e}", path.display());
            FolioConfig::default()
        }),
        Err(_) => FolioConfig::default(),
    }
}

/// Persists the config to the default location, creating directories as needed.
pub fn save(cfg: &FolioConfig) {
    if let Some(path) = config_path() {
        save_to(&path, cfg);
    }
}

/// Persists the config to a specific file, creating directories as needed.
///
/// Merges into the on-disk document instead of replacing it, and treats an
/// existing `host` as authoritative: Grit's settings page rewrites that field
/// while folio is running, so picking an editor here must not put folio's stale
/// copy back.
pub fn save_to(path: &Path, cfg: &FolioConfig) {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::warn!("failed to create config folder {}: {e}", parent.display());
            return;
        }
    }

    let mut doc = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
        Err(_) => serde_json::Value::Object(serde_json::Map::new()),
    };
    let serde_json::Value::Object(fields) = &mut doc else {
        return;
    };
    // Seed `host` only when the file has none. Anything already there was
    // written by Grit and outranks whatever folio read at startup.
    fields
        .entry("host".to_string())
        .or_insert_with(|| serde_json::Value::String(cfg.host.clone()));
    fields.insert(
        "last_app".to_string(),
        serde_json::to_value(&cfg.last_app)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
    );

    match serde_json::to_vec_pretty(&doc) {
        Ok(bytes) => {
            if let Err(e) = fs::write(path, bytes) {
                tracing::warn!("failed to save config to {}: {e}", path.display());
            }
        }
        Err(e) => tracing::warn!("failed to serialize config: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut cfg = FolioConfig::default();
        cfg.last_app.insert(
            "text/plain".into(),
            "/usr/share/applications/gedit.desktop".into(),
        );
        cfg.last_app.insert(
            "image/png".into(),
            "/usr/share/applications/eog.desktop".into(),
        );
        save_to(&path, &cfg);
        assert_eq!(load_from(&path), cfg);
    }

    #[test]
    fn host_defaults_to_loopback_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(load_from(&path).host, "127.0.0.1");
        let cfg = FolioConfig {
            host: "0.0.0.0".to_string(),
            ..Default::default()
        };
        save_to(&path, &cfg);
        assert_eq!(load_from(&path).host, "0.0.0.0");
    }

    #[test]
    fn save_preserves_a_host_written_while_folio_was_running() {
        // Grit's settings page rewrites `host` behind folio's back. Picking an
        // editor here must not put folio's stale copy back.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, br#"{"host":"127.0.0.1","last_app":{}}"#).unwrap();
        let mut in_memory = load_from(&path);
        assert_eq!(in_memory.host, "127.0.0.1");
        in_memory
            .last_app
            .insert("text/plain".into(), "zed.desktop".into());

        // ...and grit turns LAN on while folio is still running.
        fs::write(&path, br#"{"host":"0.0.0.0","last_app":{}}"#).unwrap();

        save_to(&path, &in_memory);

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["host"], "0.0.0.0", "the save reverted grit's host");
        assert_eq!(value["last_app"]["text/plain"], "zed.desktop");
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_from(&dir.path().join("missing.json")),
            FolioConfig::default()
        );
    }

    #[test]
    fn load_invalid_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(load_from(&path), FolioConfig::default());
    }

    #[test]
    fn load_writes_create_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("config.json");
        let cfg = FolioConfig::default();
        save_to(&path, &cfg);
        assert!(path.is_file());
    }
}
