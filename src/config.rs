//! Persistence for the preview pane's "open with" choices, following the
//! shared-config scheme grit uses (`$XDG_CONFIG_HOME/bitshift/<app>/config.json`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Last-used editor per MIME type: `mime -> .desktop id`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenWithConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_app: BTreeMap<String, String>,
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
pub fn load() -> OpenWithConfig {
    config_path()
        .map(|p| load_from(&p))
        .unwrap_or_default()
}

/// Loads the config from a specific file (defaults when absent or invalid).
pub fn load_from(path: &Path) -> OpenWithConfig {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!("failed to parse config at {}: {e}", path.display());
            OpenWithConfig::default()
        }),
        Err(_) => OpenWithConfig::default(),
    }
}

/// Persists the config to the default location, creating directories as needed.
pub fn save(cfg: &OpenWithConfig) {
    if let Some(path) = config_path() {
        save_to(&path, cfg);
    }
}

/// Persists the config to a specific file, creating directories as needed.
pub fn save_to(path: &Path, cfg: &OpenWithConfig) {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::warn!("failed to create config folder {}: {e}", parent.display());
            return;
        }
    }
    match serde_json::to_vec_pretty(cfg) {
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
        let mut cfg = OpenWithConfig::default();
        cfg.last_app.insert("text/plain".into(), "/usr/share/applications/gedit.desktop".into());
        cfg.last_app.insert("image/png".into(), "/usr/share/applications/eog.desktop".into());
        save_to(&path, &cfg);
        assert_eq!(load_from(&path), cfg);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_from(&dir.path().join("missing.json")), OpenWithConfig::default());
    }

    #[test]
    fn load_invalid_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(load_from(&path), OpenWithConfig::default());
    }

    #[test]
    fn load_writes_create_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("config.json");
        let cfg = OpenWithConfig::default();
        save_to(&path, &cfg);
        assert!(path.is_file());
    }
}