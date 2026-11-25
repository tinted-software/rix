//! Configuration and default directory resolution for Rix store and P2P.

use crate::error::{Result, StoreError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Return the base Rix state/config directory (`$RIX_HOME` or `~/.rix`).
pub fn rix_home() -> PathBuf {
    if let Ok(dir) = std::env::var("RIX_HOME")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(".rix");
    }
    std::env::temp_dir().join("rix")
}

/// Resolve the active Nix-compatible store directory.
pub fn default_store_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NIX_STORE_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    let nix_store = Path::new("/nix/store");
    if nix_store.is_dir() && is_dir_writable(nix_store) {
        nix_store.to_path_buf()
    } else {
        rix_home().join("store")
    }
}

/// Resolve the `iroh-blobs` storage directory corresponding to `store_dir`.
pub fn default_blobs_dir(store_dir: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("RIX_BLOBS_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    if store_dir == Path::new("/nix/store") {
        rix_home().join("blobs")
    } else if let Some(parent) = store_dir.parent()
        && !parent.as_os_str().is_empty()
    {
        parent.join("blobs")
    } else {
        rix_home().join("blobs")
    }
}

/// Default path to `config.toml`.
pub fn default_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("RIX_CONFIG")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    rix_home().join("config.toml")
}

/// Default path to the local Ed25519 identity secret key.
pub fn default_identity_path() -> PathBuf {
    if let Ok(path) = std::env::var("RIX_IDENTITY_KEY")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    rix_home().join("identity.key")
}

fn is_dir_writable(path: &Path) -> bool {
    let test_file = path.join(".writable_test");
    if std::fs::write(&test_file, b"test").is_ok() {
        let _ = std::fs::remove_file(test_file);
        true
    } else {
        false
    }
}

/// Top-level Rix configuration stored in `~/.rix/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RixConfig {
    pub p2p: P2pConfig,
}

/// P2P substitution and opt-in seeding configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct P2pConfig {
    /// Whether P2P substitution is enabled when peers or trusted keys are configured.
    pub enabled: bool,
    /// Seeding is strictly opt-in (default `false`).
    pub seed: bool,
    /// Whether Mainline DHT (`n0-mainline` + `iroh-mainline-address-lookup`) is enabled.
    pub enable_dht: bool,
    /// Allow substituting unsigned/untrusted store paths (default `false`).
    pub allow_untrusted: bool,
    /// Trusted Ed25519 public keys (`EndpointId` strings) whose signed manifests are accepted.
    pub trusted_keys: Vec<String>,
    /// Explicit peer `EndpointId` strings to query over `/rix/p2p/1`.
    pub peers: Vec<String>,
    /// Timeout in seconds for P2P discovery lookups.
    pub lookup_timeout_secs: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            seed: false,
            enable_dht: true,
            allow_untrusted: false,
            trusted_keys: Vec::new(),
            peers: Vec::new(),
            lookup_timeout_secs: 15,
        }
    }
}

impl RixConfig {
    /// Load configuration from the default config path, returning defaults if absent.
    pub fn load() -> Result<Self> {
        Self::load_from(&default_config_path())
    }

    /// Load configuration from `path`, returning defaults if the file does not exist.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw)
            .map_err(|e| StoreError::Config(format!("failed to parse {}: {e}", path.display())))
    }

    /// Persist configuration to `path`.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| StoreError::Config(format!("failed to serialize config: {e}")))?;
        std::fs::write(path, body)?;
        Ok(())
    }
}
