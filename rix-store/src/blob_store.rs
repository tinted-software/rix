//! `iroh-blobs`-backed storage for Nix-like store paths without NAR.
//!
//! Each store path `<hash>-<name>` is represented as a `HashSeq` (`BlobFormat::HashSeq`):
//! - Index 0: JSON-encoded [`StoreManifest`] (`BlobFormat::Raw`)
//! - Index 1..N: Deduplicated, sorted BLAKE3 file payload blobs referenced by the manifest
//!
//! Persistent tags pin every store path and its signed metadata against GC:
//! - `rix:path:<store_key>` -> `HashAndFormat::hash_seq(root_hash)`
//! - `rix:meta:<store_key>` -> `HashAndFormat::raw(signed_info_blob_hash)`

use crate::error::{Result, StoreError};
use crate::manifest::{ManifestKind, PathInfo, SignedPathInfo, StoreManifest, TreeEntry};
use crate::path::{is_valid_store_key, scan_references};
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh_blobs::api::Store as IrohApiStore;
use iroh_blobs::api::blobs::AddBytesOptions;
use iroh_blobs::hashseq::HashSeq;
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::{BlobFormat, Hash, HashAndFormat};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const PATH_TAG_PREFIX: &str = "rix:path:";
const META_TAG_PREFIX: &str = "rix:meta:";
static TMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Report returned by [`BlobStore::verify_path`].
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub store_key: String,
    pub root_hash: Hash,
    pub file_blob_count: usize,
    pub total_size: u64,
}

/// Wrapper around `iroh_blobs::api::Store` (either persistent `FsStore` or `MemStore`).
#[derive(Debug, Clone)]
pub struct BlobStore {
    inner: IrohApiStore,
}

impl BlobStore {
    /// Open or initialize a persistent `FsStore` at `blobs_dir`.
    pub async fn open_fs(blobs_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(blobs_dir)?;
        let fs_store = FsStore::load(blobs_dir).await.map_err(|e| {
            StoreError::BlobStore(format!(
                "failed to open FsStore at {}: {e}",
                blobs_dir.display()
            ))
        })?;
        Ok(Self {
            inner: fs_store.into(),
        })
    }

    /// Create an ephemeral in-memory `MemStore` (useful for tests).
    pub fn open_mem() -> Self {
        let mem = MemStore::new();
        Self { inner: mem.into() }
    }

    /// Borrow the underlying `iroh_blobs::api::Store`.
    pub fn store(&self) -> &IrohApiStore {
        &self.inner
    }

    /// Flush database metadata and shut down the store cleanly (releasing `redb` file locks).
    pub async fn shutdown(self) -> Result<()> {
        self.inner
            .sync_db()
            .await
            .map_err(|e| StoreError::BlobStore(format!("sync_db failed: {e}")))?;
        self.inner
            .shutdown()
            .await
            .map_err(|e| StoreError::BlobStore(format!("shutdown failed: {e}")))?;
        Ok(())
    }

    pub fn path_tag(store_key: &str) -> Vec<u8> {
        format!("{PATH_TAG_PREFIX}{store_key}").into_bytes()
    }

    pub fn meta_tag(store_key: &str) -> Vec<u8> {
        format!("{META_TAG_PREFIX}{store_key}").into_bytes()
    }

    /// Check whether `store_key` is pinned in the local `iroh-blobs` store.
    pub async fn has_path(&self, store_key: &str) -> Result<bool> {
        Ok(self.get_root_hash(store_key).await?.is_some())
    }

    /// Look up the root `HashSeq` hash for `store_key`.
    pub async fn get_root_hash(&self, store_key: &str) -> Result<Option<Hash>> {
        let tag = self
            .inner
            .tags()
            .get(Self::path_tag(store_key))
            .await
            .map_err(|e| StoreError::BlobStore(format!("tags.get failed: {e}")))?;
        Ok(tag.map(|t| t.hash))
    }

    /// Look up the [`SignedPathInfo`] for `store_key`.
    pub async fn get_path_info(&self, store_key: &str) -> Result<Option<SignedPathInfo>> {
        let Some(tag) = self
            .inner
            .tags()
            .get(Self::meta_tag(store_key))
            .await
            .map_err(|e| StoreError::BlobStore(format!("tags.get meta failed: {e}")))?
        else {
            return Ok(None);
        };
        let bytes = self
            .inner
            .blobs()
            .get_bytes(tag.hash)
            .await
            .map_err(|e| StoreError::BlobStore(format!("get_bytes meta failed: {e}")))?;
        let info: SignedPathInfo = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Serialization(format!("decode SignedPathInfo: {e}")))?;
        Ok(Some(info))
    }

    /// List all `store_key`s tracked by `rix:path:` tags.
    pub async fn list_paths(&self) -> Result<Vec<String>> {
        let mut stream = self
            .inner
            .tags()
            .list_prefix(PATH_TAG_PREFIX.as_bytes())
            .await
            .map_err(|e| StoreError::BlobStore(format!("list_prefix failed: {e}")))?;
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            let tag_info =
                item.map_err(|e| StoreError::BlobStore(format!("tag stream error: {e}")))?;
            if let Ok(name) = std::str::from_utf8(tag_info.name.as_ref())
                && let Some(key) = name.strip_prefix(PATH_TAG_PREFIX)
            {
                out.push(key.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Ingest an on-disk store path into `iroh-blobs`, returning its [`SignedPathInfo`].
    pub async fn ingest_path(
        &self,
        store_dir: &Path,
        store_key: &str,
        disk_path: &Path,
        drv_path: Option<String>,
        signer: Option<&iroh::SecretKey>,
    ) -> Result<SignedPathInfo> {
        if !is_valid_store_key(store_key) {
            return Err(StoreError::InvalidPath(format!(
                "invalid store_key '{store_key}'"
            )));
        }

        let meta = std::fs::symlink_metadata(disk_path)?;
        let ft = meta.file_type();

        let batch = self
            .inner
            .blobs()
            .batch()
            .await
            .map_err(|e| StoreError::BlobStore(format!("batch failed: {e}")))?;
        let mut temp_tags = Vec::new();
        let mut unique_blobs = BTreeSet::new();
        let mut total_size: u64 = 0;

        let kind = if ft.is_symlink() {
            let target = std::fs::read_link(disk_path)?;
            ManifestKind::Symlink {
                target: target.to_string_lossy().to_string(),
            }
        } else if ft.is_file() {
            let executable = is_executable(&meta);
            let size = meta.len();
            total_size = size;
            let tt = batch
                .add_path_with_opts(iroh_blobs::api::blobs::AddPathOptions {
                    path: disk_path.to_path_buf(),
                    format: BlobFormat::Raw,
                    mode: iroh_blobs::api::blobs::ImportMode::Copy,
                })
                .temp_tag()
                .await
                .map_err(|e| StoreError::BlobStore(format!("add_path failed: {e}")))?;
            let blob = tt.hash();
            unique_blobs.insert(blob);
            temp_tags.push(tt);
            ManifestKind::File {
                executable,
                size,
                blob,
            }
        } else if ft.is_dir() {
            let mut entries = Vec::new();
            ingest_dir_recursive(
                disk_path,
                disk_path,
                &batch,
                &mut entries,
                &mut unique_blobs,
                &mut temp_tags,
                &mut total_size,
            )
            .await?;
            ManifestKind::Directory { entries }
        } else {
            return Err(StoreError::InvalidPath(format!(
                "unsupported file type at {}",
                disk_path.display()
            )));
        };

        let references = scan_references(store_dir, disk_path, store_key)?;
        let manifest = StoreManifest {
            version: 1,
            store_key: store_key.to_string(),
            kind,
            total_size,
            references: references.clone(),
            drv_path: drv_path.clone(),
        };
        manifest.validate()?;

        let manifest_bytes = serde_json::to_vec(&manifest)
            .map_err(|e| StoreError::Serialization(format!("encode StoreManifest: {e}")))?;
        let manifest_tt = batch
            .add_bytes(Bytes::from(manifest_bytes))
            .temp_tag()
            .await
            .map_err(|e| StoreError::BlobStore(format!("add manifest bytes failed: {e}")))?;
        let manifest_hash = manifest_tt.hash();
        temp_tags.push(manifest_tt);

        let mut seq_hashes = Vec::with_capacity(1 + unique_blobs.len());
        seq_hashes.push(manifest_hash);
        seq_hashes.extend(unique_blobs);
        let hash_seq: HashSeq = seq_hashes.into_iter().collect();

        let root_tt = batch
            .add_bytes_with_opts(AddBytesOptions {
                data: hash_seq.into_inner(),
                format: BlobFormat::HashSeq,
            })
            .temp_tag()
            .await
            .map_err(|e| StoreError::BlobStore(format!("add HashSeq failed: {e}")))?;
        let root_hash = root_tt.hash();
        temp_tags.push(root_tt);

        let path_info = PathInfo::new(
            store_key.to_string(),
            root_hash,
            total_size,
            references,
            drv_path,
        );
        let mut signed = SignedPathInfo::new(path_info);
        if let Some(sk) = signer {
            signed.sign(sk)?;
        }

        let meta_bytes = serde_json::to_vec(&signed)
            .map_err(|e| StoreError::Serialization(format!("encode SignedPathInfo: {e}")))?;
        let meta_tt = batch
            .add_bytes(Bytes::from(meta_bytes))
            .temp_tag()
            .await
            .map_err(|e| StoreError::BlobStore(format!("add meta bytes failed: {e}")))?;
        let meta_hash = meta_tt.hash();
        temp_tags.push(meta_tt);

        self.inner
            .tags()
            .set(
                Self::path_tag(store_key),
                HashAndFormat::hash_seq(root_hash),
            )
            .await
            .map_err(|e| StoreError::BlobStore(format!("set path tag failed: {e}")))?;
        self.inner
            .tags()
            .set(Self::meta_tag(store_key), HashAndFormat::raw(meta_hash))
            .await
            .map_err(|e| StoreError::BlobStore(format!("set meta tag failed: {e}")))?;

        drop(temp_tags);
        drop(batch);
        Ok(signed)
    }

    /// Load and validate the [`StoreManifest`] from a locally stored `HashSeq` `root_hash`.
    pub async fn load_manifest(&self, root_hash: Hash) -> Result<StoreManifest> {
        let seq_bytes = self.inner.blobs().get_bytes(root_hash).await.map_err(|e| {
            StoreError::BlobStore(format!("failed to read HashSeq {root_hash}: {e}"))
        })?;
        let mut hash_seq = HashSeq::new(seq_bytes).ok_or_else(|| {
            StoreError::InvalidManifest(format!("blob {root_hash} is not a valid HashSeq"))
        })?;
        let manifest_hash = hash_seq
            .pop_front()
            .ok_or_else(|| StoreError::InvalidManifest(format!("HashSeq {root_hash} is empty")))?;
        let payload_hashes: HashSet<Hash> = hash_seq.into_iter().collect();

        let manifest_bytes = self
            .inner
            .blobs()
            .get_bytes(manifest_hash)
            .await
            .map_err(|e| {
                StoreError::BlobStore(format!("failed to read manifest blob {manifest_hash}: {e}"))
            })?;
        let manifest: StoreManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| StoreError::InvalidManifest(format!("invalid StoreManifest JSON: {e}")))?;
        manifest.validate()?;

        for blob in manifest.file_blobs() {
            if !payload_hashes.contains(&blob) {
                return Err(StoreError::InvalidManifest(format!(
                    "manifest references blob {blob} missing from HashSeq {root_hash}"
                )));
            }
        }
        Ok(manifest)
    }

    /// Validate a downloaded `HashSeq` and pin it under `rix:path:` and `rix:meta:` tags.
    pub async fn register_fetched(&self, signed_info: &SignedPathInfo) -> Result<StoreManifest> {
        let manifest = self.load_manifest(signed_info.info.root_hash).await?;
        if manifest.store_key != signed_info.info.store_key {
            return Err(StoreError::InvalidManifest(format!(
                "manifest store_key '{}' does not match PathInfo '{}'",
                manifest.store_key, signed_info.info.store_key
            )));
        }
        for blob in manifest.file_blobs() {
            let present = self
                .inner
                .blobs()
                .has(blob)
                .await
                .map_err(|e| StoreError::BlobStore(format!("has({blob}) failed: {e}")))?;
            if !present {
                return Err(StoreError::BlobStore(format!(
                    "missing payload blob {blob} for {}",
                    manifest.store_key
                )));
            }
        }

        let meta_bytes = serde_json::to_vec(signed_info)
            .map_err(|e| StoreError::Serialization(format!("encode SignedPathInfo: {e}")))?;
        let meta_tag = self
            .inner
            .blobs()
            .add_bytes(Bytes::from(meta_bytes))
            .temp_tag()
            .await
            .map_err(|e| StoreError::BlobStore(format!("add fetched meta failed: {e}")))?;

        self.inner
            .tags()
            .set(
                Self::path_tag(&manifest.store_key),
                HashAndFormat::hash_seq(signed_info.info.root_hash),
            )
            .await
            .map_err(|e| StoreError::BlobStore(format!("set path tag failed: {e}")))?;
        self.inner
            .tags()
            .set(
                Self::meta_tag(&manifest.store_key),
                HashAndFormat::raw(meta_tag.hash()),
            )
            .await
            .map_err(|e| StoreError::BlobStore(format!("set meta tag failed: {e}")))?;

        drop(meta_tag);
        Ok(manifest)
    }

    /// Materialize a stored path from `iroh-blobs` onto `dest_path` atomically.
    pub async fn materialize_path(&self, store_key: &str, dest_path: &Path) -> Result<()> {
        if path_already_populated(dest_path) {
            return Ok(());
        }
        let root_hash = self
            .get_root_hash(store_key)
            .await?
            .ok_or_else(|| StoreError::NotFound(store_key.to_string()))?;
        let manifest = self.load_manifest(root_hash).await?;
        if manifest.store_key != store_key {
            return Err(StoreError::InvalidManifest(format!(
                "stored manifest key '{}' != requested '{store_key}'",
                manifest.store_key
            )));
        }

        let parent = dest_path
            .parent()
            .ok_or_else(|| StoreError::InvalidPath("destination has no parent".to_string()))?;
        std::fs::create_dir_all(parent)?;

        let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(".rix-tmp-{store_key}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_file(&staging);

        let materialize_res: Result<()> = async {
            match &manifest.kind {
                ManifestKind::Symlink { target } => {
                    create_symlink(Path::new(target), &staging)?;
                }
                ManifestKind::File {
                    executable, blob, ..
                } => {
                    self.inner
                        .blobs()
                        .export(*blob, &staging)
                        .await
                        .map_err(|e| StoreError::BlobStore(format!("export {blob} failed: {e}")))?;
                    set_executable_mode(&staging, *executable)?;
                }
                ManifestKind::Directory { entries } => {
                    std::fs::create_dir_all(&staging)?;
                    for entry in entries {
                        match entry {
                            TreeEntry::Dir { path } => {
                                std::fs::create_dir_all(staging.join(path))?;
                            }
                            TreeEntry::File {
                                path,
                                executable,
                                blob,
                                ..
                            } => {
                                let target = staging.join(path);
                                if let Some(p) = target.parent() {
                                    std::fs::create_dir_all(p)?;
                                }
                                self.inner
                                    .blobs()
                                    .export(*blob, &target)
                                    .await
                                    .map_err(|e| {
                                        StoreError::BlobStore(format!("export {blob} failed: {e}"))
                                    })?;
                                set_executable_mode(&target, *executable)?;
                            }
                            TreeEntry::Symlink { path, target } => {
                                let link_path = staging.join(path);
                                if let Some(p) = link_path.parent() {
                                    std::fs::create_dir_all(p)?;
                                }
                                create_symlink(Path::new(target), &link_path)?;
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        .await;

        if let Err(err) = materialize_res {
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }

        // Remove an empty placeholder directory if one exists before atomic rename.
        if dest_path.is_dir() && !path_already_populated(dest_path) {
            let _ = std::fs::remove_dir(dest_path);
        }

        match std::fs::rename(&staging, dest_path) {
            Ok(()) => Ok(()),
            Err(e) if path_already_populated(dest_path) => {
                let _ = std::fs::remove_dir_all(&staging);
                let _ = std::fs::remove_file(&staging);
                let _ = e;
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                let _ = std::fs::remove_file(&staging);
                Err(e.into())
            }
        }
    }

    /// Verify that `store_key`'s `HashSeq`, `StoreManifest`, and all payload blobs are complete.
    pub async fn verify_path(&self, store_key: &str) -> Result<VerifyReport> {
        let root_hash = self
            .get_root_hash(store_key)
            .await?
            .ok_or_else(|| StoreError::NotFound(store_key.to_string()))?;
        let manifest = self.load_manifest(root_hash).await?;
        let blobs = manifest.file_blobs();
        for blob in &blobs {
            let ok = self
                .inner
                .blobs()
                .has(*blob)
                .await
                .map_err(|e| StoreError::BlobStore(format!("has({blob}) failed: {e}")))?;
            if !ok {
                return Err(StoreError::BlobStore(format!(
                    "corrupt or missing blob {blob} in {store_key}"
                )));
            }
        }
        Ok(VerifyReport {
            store_key: store_key.to_string(),
            root_hash,
            file_blob_count: blobs.len(),
            total_size: manifest.total_size,
        })
    }
}

async fn ingest_dir_recursive(
    root: &Path,
    dir: &Path,
    batch: &iroh_blobs::api::blobs::Batch<'_>,
    entries: &mut Vec<TreeEntry>,
    unique_blobs: &mut BTreeSet<Hash>,
    temp_tags: &mut Vec<iroh_blobs::api::TempTag>,
    total_size: &mut u64,
) -> Result<()> {
    let mut children: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .collect();
    children.sort();

    for child in children {
        let meta = std::fs::symlink_metadata(&child)?;
        let ft = meta.file_type();
        let rel = child
            .strip_prefix(root)
            .map_err(|e| StoreError::InvalidPath(format!("strip_prefix failed: {e}")))?
            .to_string_lossy()
            .replace('\\', "/");

        if ft.is_symlink() {
            let target = std::fs::read_link(&child)?;
            entries.push(TreeEntry::Symlink {
                path: rel,
                target: target.to_string_lossy().to_string(),
            });
        } else if ft.is_dir() {
            entries.push(TreeEntry::Dir { path: rel });
            Box::pin(ingest_dir_recursive(
                root,
                &child,
                batch,
                entries,
                unique_blobs,
                temp_tags,
                total_size,
            ))
            .await?;
        } else if ft.is_file() {
            let executable = is_executable(&meta);
            let size = meta.len();
            *total_size = total_size.saturating_add(size);
            let tt = batch
                .add_path_with_opts(iroh_blobs::api::blobs::AddPathOptions {
                    path: child,
                    format: BlobFormat::Raw,
                    mode: iroh_blobs::api::blobs::ImportMode::Copy,
                })
                .temp_tag()
                .await
                .map_err(|e| StoreError::BlobStore(format!("add_path failed: {e}")))?;
            let blob = tt.hash();
            unique_blobs.insert(blob);
            temp_tags.push(tt);
            entries.push(TreeEntry::File {
                path: rel,
                executable,
                size,
                blob,
            });
        }
    }
    Ok(())
}

fn is_executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (meta.permissions().mode() & 0o111) != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

fn set_executable_mode(path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
    }
    Ok(())
}

fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlinks are only supported on Unix",
        )))
    }
}

/// Check if an on-disk store path is already populated (matches Nix store validity semantics).
pub fn path_already_populated(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    let ft = meta.file_type();
    if ft.is_symlink() || ft.is_file() {
        return true;
    }
    if ft.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        return entries.count() > 0;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate_ephemeral_identity;
    use crate::path::compute_store_hash;

    #[tokio::test]
    async fn test_blob_store_ingest_verify_and_materialize() {
        let tmp = tempfile::tempdir().unwrap();
        let store_dir = tmp.path().join("store");
        let blobs_dir = tmp.path().join("blobs");
        std::fs::create_dir_all(&store_dir).unwrap();

        let dep_key = format!("{}-dep", compute_store_hash(b"dep"));
        let app_key = format!("{}-app", compute_store_hash(b"app"));
        let app_path = store_dir.join(&app_key);
        std::fs::create_dir_all(app_path.join("bin")).unwrap();
        std::fs::create_dir_all(app_path.join("empty_dir")).unwrap();
        let bin_path = app_path.join("bin/hello");
        std::fs::write(
            &bin_path,
            format!("#!/bin/sh\necho {}/{}\n", store_dir.display(), dep_key),
        )
        .unwrap();
        set_executable_mode(&bin_path, true).unwrap();
        std::fs::write(app_path.join("README"), b"readme data").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("hello", app_path.join("bin/hello-alias")).unwrap();

        let bs = BlobStore::open_fs(&blobs_dir).await.unwrap();
        let sk = generate_ephemeral_identity();
        let signed = bs
            .ingest_path(&store_dir, &app_key, &app_path, None, Some(&sk))
            .await
            .unwrap();
        assert_eq!(signed.info.references, vec![dep_key]);
        assert_eq!(signed.verify_any(&[sk.public()]).unwrap(), sk.public());

        let report = bs.verify_path(&app_key).await.unwrap();
        assert_eq!(report.file_blob_count, 2);

        // Delete materialized directory, then re-materialize from iroh-blobs!
        std::fs::remove_dir_all(&app_path).unwrap();
        assert!(!app_path.exists());

        bs.materialize_path(&app_key, &app_path).await.unwrap();
        assert!(app_path.join("empty_dir").is_dir());
        assert_eq!(
            std::fs::read(app_path.join("README")).unwrap(),
            b"readme data"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::symlink_metadata(app_path.join("bin/hello"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
            assert_eq!(
                std::fs::read_link(app_path.join("bin/hello-alias")).unwrap(),
                Path::new("hello")
            );
        }

        bs.shutdown().await.unwrap();
    }
}
