//! High-level Nix-like Store backed by `iroh-blobs` with optional P2P substitution.

use crate::blob_store::{BlobStore, path_already_populated};
use crate::config::{RixConfig, default_blobs_dir, default_identity_path, default_store_dir};
use crate::error::{Result, StoreError};
use crate::keys::{load_identity, parse_public_key};
use crate::manifest::SignedPathInfo;
use crate::p2p::substituter::{Substituter, SubstitutionConfig};
use crate::path::{compute_store_hash, extract_store_key};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

type BridgeJob = Box<dyn FnOnce() + Send + 'static>;

fn bridge_sender() -> &'static mpsc::Sender<BridgeJob> {
    static BRIDGE: OnceLock<mpsc::Sender<BridgeJob>> = OnceLock::new();
    BRIDGE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<BridgeJob>();
        std::thread::Builder::new()
            .name("rix-store-rt".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("failed to build rix-store Tokio runtime");
                let _guard = rt.enter();
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("failed to spawn rix-store runtime thread");
        tx
    })
}

/// Run an async `Send + 'static` future synchronously without ever colliding with a caller's Tokio runtime.
pub fn block_on_store<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let job: BridgeJob = Box::new(move || {
        tokio::spawn(async move {
            let out = fut.await;
            let _ = done_tx.send(out);
        });
    });
    bridge_sender()
        .send(job)
        .expect("rix-store runtime thread exited");
    done_rx
        .recv()
        .expect("rix-store async task panicked before returning")
}

/// Nix-like Store manager backed by `iroh-blobs`.
#[derive(Debug, Clone)]
pub struct Store {
    pub store_dir: PathBuf,
    pub blobs_dir: PathBuf,
    pub config: RixConfig,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// Initialize the store using environment / default directories and `~/.rix/config.toml`.
    pub fn new() -> Self {
        let store_dir = default_store_dir();
        let blobs_dir = default_blobs_dir(&store_dir);
        let config = RixConfig::load().unwrap_or_default();
        Self {
            store_dir,
            blobs_dir,
            config,
        }
    }

    /// Initialize a store at explicit `store_dir` and `blobs_dir` with default config.
    pub fn with_dirs(store_dir: impl Into<PathBuf>, blobs_dir: impl Into<PathBuf>) -> Self {
        Self {
            store_dir: store_dir.into(),
            blobs_dir: blobs_dir.into(),
            config: RixConfig::default(),
        }
    }

    /// Ensure the store and blobs directories exist.
    pub fn ensure_store_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.store_dir)?;
        std::fs::create_dir_all(&self.blobs_dir)?;
        Ok(())
    }

    /// Compute a store path for a named derivation or file.
    pub fn compute_store_path(&self, name: &str, content: &[u8], is_drv: bool) -> (String, String) {
        let hash = compute_store_hash(content);
        let ext = if is_drv { ".drv" } else { "" };
        let store_path = self
            .store_dir
            .join(format!("{}-{}{}", hash, name, ext))
            .to_string_lossy()
            .to_string();
        (store_path, hash)
    }

    /// Compute derivation store path and its output paths.
    pub fn compute_derivation_paths(
        &self,
        name: &str,
        outputs: &[String],
        env: &HashMap<String, String>,
    ) -> (String, HashMap<String, String>) {
        let mut sorted_env: Vec<(&String, &String)> = env.iter().collect();
        sorted_env.sort_by_key(|(k, _)| *k);

        let mut hash_data = format!("name={};outputs={:?};", name, outputs);
        for (k, v) in sorted_env {
            hash_data.push_str(k);
            hash_data.push('=');
            hash_data.push_str(v);
            hash_data.push(';');
        }

        let drv_hash = compute_store_hash(hash_data.as_bytes());
        let drv_path = self
            .store_dir
            .join(format!("{}-{}.drv", drv_hash, name))
            .to_string_lossy()
            .to_string();

        let mut output_paths = HashMap::new();
        for output in outputs {
            let out_data = format!("drv={};out={};name={}", drv_hash, output, name);
            let out_hash = compute_store_hash(out_data.as_bytes());
            let out_suffix = if output == "out" {
                name.to_string()
            } else {
                format!("{}-{}", name, output)
            };
            let out_path = self
                .store_dir
                .join(format!("{}-{}", out_hash, out_suffix))
                .to_string_lossy()
                .to_string();
            output_paths.insert(output.clone(), out_path);
        }

        (drv_path, output_paths)
    }

    /// Check if a store path already exists and is populated on disk.
    pub fn is_valid_path(&self, path: &Path) -> bool {
        path_already_populated(path)
    }

    /// Open the persistent `iroh-blobs` [`BlobStore`] asynchronously.
    pub async fn open_blob_store(&self) -> Result<BlobStore> {
        BlobStore::open_fs(&self.blobs_dir).await
    }

    /// Ensure `path` is materialized on disk, restoring it from `iroh-blobs` if cached there.
    pub fn ensure_materialized(&self, path: &Path) -> Result<bool> {
        if self.is_valid_path(path) {
            return Ok(true);
        }
        let Some(store_key) = extract_store_key(&self.store_dir, path) else {
            return Ok(false);
        };
        let blobs_dir = self.blobs_dir.clone();
        let dest = path.to_path_buf();
        block_on_store(async move {
            let bs = BlobStore::open_fs(&blobs_dir).await?;
            let found = if bs.has_path(&store_key).await? {
                bs.materialize_path(&store_key, &dest).await?;
                true
            } else {
                false
            };
            let _ = bs.shutdown().await;
            Ok(found)
        })
    }

    /// Ingest a locally built store path into `iroh-blobs`, optionally signing with local identity.
    pub fn ingest_built_path(
        &self,
        disk_path: &Path,
        drv_path: Option<String>,
        sign: bool,
    ) -> Result<SignedPathInfo> {
        let store_key = extract_store_key(&self.store_dir, disk_path).ok_or_else(|| {
            StoreError::InvalidPath(format!(
                "path {} is not inside store dir {}",
                disk_path.display(),
                self.store_dir.display()
            ))
        })?;
        let signer = if sign {
            load_identity(&default_identity_path())?
        } else {
            None
        };
        let store_dir = self.store_dir.clone();
        let blobs_dir = self.blobs_dir.clone();
        let disk_path = disk_path.to_path_buf();
        block_on_store(async move {
            let bs = BlobStore::open_fs(&blobs_dir).await?;
            let res = bs
                .ingest_path(
                    &store_dir,
                    &store_key,
                    &disk_path,
                    drv_path,
                    signer.as_ref(),
                )
                .await;
            let _ = bs.shutdown().await;
            res
        })
    }

    /// Build a [`SubstitutionConfig`] by merging `~/.rix/config.toml` with CLI overrides.
    pub fn substitution_config(
        &self,
        extra_peers: &[String],
        extra_trusted_keys: &[String],
        allow_untrusted_override: bool,
    ) -> Result<SubstitutionConfig> {
        let p2p = &self.config.p2p;
        let mut peers = Vec::new();
        for s in p2p.peers.iter().chain(extra_peers.iter()) {
            let pk = parse_public_key(s)?;
            if !peers.contains(&pk) {
                peers.push(pk);
            }
        }
        let mut trusted_keys = Vec::new();
        for s in p2p.trusted_keys.iter().chain(extra_trusted_keys.iter()) {
            let pk = parse_public_key(s)?;
            if !trusted_keys.contains(&pk) {
                trusted_keys.push(pk);
            }
        }
        Ok(SubstitutionConfig {
            peers,
            known_addrs: Vec::new(),
            trusted_keys,
            enable_dht: p2p.enable_dht,
            use_relays: true,
            allow_untrusted: p2p.allow_untrusted || allow_untrusted_override,
            timeout: Duration::from_secs(p2p.lookup_timeout_secs.max(1)),
        })
    }

    /// Attempt to substitute all `output_paths` via P2P into `self.store_dir`.
    ///
    /// Returns `Ok(true)` only if every output path was successfully substituted (or already valid).
    pub fn try_substitute(
        &self,
        output_paths: &[PathBuf],
        cfg: &SubstitutionConfig,
    ) -> Result<bool> {
        if !cfg.has_discovery_sources() {
            return Ok(false);
        }
        let mut keys = Vec::with_capacity(output_paths.len());
        for p in output_paths {
            let Some(key) = extract_store_key(&self.store_dir, p) else {
                return Ok(false);
            };
            keys.push(key);
        }

        let blobs_dir = self.blobs_dir.clone();
        let store_dir = self.store_dir.clone();
        let cfg = cfg.clone();

        block_on_store(async move {
            let bs = BlobStore::open_fs(&blobs_dir).await?;
            let sub = match Substituter::new(bs.clone(), cfg).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = bs.shutdown().await;
                    return Err(e);
                }
            };
            let mut all_ok = true;
            for key in keys {
                match sub.substitute_path(&store_dir, &key).await {
                    Ok(true) => {}
                    Ok(false) => {
                        all_ok = false;
                        break;
                    }
                    Err(e) => {
                        sub.close().await;
                        let _ = bs.shutdown().await;
                        return Err(e);
                    }
                }
            }
            sub.close().await;
            let _ = bs.shutdown().await;
            Ok(all_ok)
        })
    }
}
