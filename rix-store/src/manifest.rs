//! Store path manifests (replacing NAR) and Ed25519-signed path info metadata.

use crate::error::{Result, StoreError};
use crate::path::is_valid_store_key;
use serde::{Deserialize, Serialize};
use std::path::Component;
use std::time::{SystemTime, UNIX_EPOCH};

/// Domain separator prepended before signing `PathInfo` bytes.
const PATH_INFO_SIGNING_DOMAIN: &[u8] = b"rix-path-info-v1\n";

/// Structural manifest for a store path stored in `iroh-blobs`.
///
/// Replaces Nix NAR archives: file payloads live as deduplicated, BLAKE3-verified
/// blobs in `iroh-blobs`, while directory layout, permissions, and symlinks are
/// recorded in this compact JSON manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreManifest {
    pub version: u32,
    pub store_key: String,
    pub kind: ManifestKind,
    pub total_size: u64,
    pub references: Vec<String>,
    pub drv_path: Option<String>,
}

/// Root filesystem kind of a store path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManifestKind {
    File {
        executable: bool,
        size: u64,
        blob: iroh_blobs::Hash,
    },
    Symlink {
        target: String,
    },
    Directory {
        entries: Vec<TreeEntry>,
    },
}

/// Entry inside a `Directory` store manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeEntry {
    Dir {
        path: String,
    },
    File {
        path: String,
        executable: bool,
        size: u64,
        blob: iroh_blobs::Hash,
    },
    Symlink {
        path: String,
        target: String,
    },
}

impl StoreManifest {
    /// Validate manifest invariants and ensure all relative paths are safe against traversal.
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(StoreError::InvalidManifest(format!(
                "unsupported manifest version {}",
                self.version
            )));
        }
        if !is_valid_store_key(&self.store_key) {
            return Err(StoreError::InvalidManifest(format!(
                "invalid store_key '{}'",
                self.store_key
            )));
        }
        for r in &self.references {
            if !is_valid_store_key(r) {
                return Err(StoreError::InvalidManifest(format!(
                    "invalid reference store_key '{r}'"
                )));
            }
        }
        if let ManifestKind::Directory { entries } = &self.kind {
            for entry in entries {
                let rel = match entry {
                    TreeEntry::Dir { path }
                    | TreeEntry::File { path, .. }
                    | TreeEntry::Symlink { path, .. } => path,
                };
                validate_relative_path(rel)?;
            }
        }
        Ok(())
    }

    /// Return every file blob hash referenced by this manifest.
    pub fn file_blobs(&self) -> Vec<iroh_blobs::Hash> {
        let mut blobs = Vec::new();
        match &self.kind {
            ManifestKind::File { blob, .. } => blobs.push(*blob),
            ManifestKind::Symlink { .. } => {}
            ManifestKind::Directory { entries } => {
                for entry in entries {
                    if let TreeEntry::File { blob, .. } = entry {
                        blobs.push(*blob);
                    }
                }
            }
        }
        blobs
    }
}

fn validate_relative_path(rel: &str) -> Result<()> {
    if rel.is_empty() || rel.contains('\0') || rel.starts_with('/') || rel.starts_with('\\') {
        return Err(StoreError::InvalidManifest(format!(
            "unsafe relative path '{rel}'"
        )));
    }
    let path = std::path::Path::new(rel);
    let mut has_normal = false;
    for comp in path.components() {
        match comp {
            Component::Normal(_) => has_normal = true,
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(StoreError::InvalidManifest(format!(
                    "path traversal or non-relative component in '{rel}'"
                )));
            }
        }
    }
    if !has_normal {
        return Err(StoreError::InvalidManifest(format!(
            "empty path components in '{rel}'"
        )));
    }
    Ok(())
}

/// Summary metadata for a store path, bound to a BLAKE3 `HashSeq` `root_hash`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathInfo {
    pub store_key: String,
    pub root_hash: iroh_blobs::Hash,
    pub total_size: u64,
    pub references: Vec<String>,
    pub drv_path: Option<String>,
    pub timestamp: u64,
}

impl PathInfo {
    /// Create a `PathInfo` with the current UNIX timestamp and sorted references.
    pub fn new(
        store_key: String,
        root_hash: iroh_blobs::Hash,
        total_size: u64,
        mut references: Vec<String>,
        drv_path: Option<String>,
    ) -> Self {
        references.sort();
        references.dedup();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            store_key,
            root_hash,
            total_size,
            references,
            drv_path,
            timestamp,
        }
    }

    /// Canonical byte representation used for Ed25519 signing and verification.
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>> {
        let mut normalized = self.clone();
        normalized.references.sort();
        normalized.references.dedup();
        let encoded = postcard::to_allocvec(&normalized)
            .map_err(|e| StoreError::Serialization(format!("postcard encode PathInfo: {e}")))?;
        let mut out = Vec::with_capacity(PATH_INFO_SIGNING_DOMAIN.len() + encoded.len());
        out.extend_from_slice(PATH_INFO_SIGNING_DOMAIN);
        out.extend_from_slice(&encoded);
        Ok(out)
    }
}

/// Ed25519 signature over `PathInfo::canonical_signing_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathSignature {
    pub signer: iroh::PublicKey,
    /// Hex-encoded 64-byte Ed25519 signature.
    pub signature: String,
}

/// Store path metadata paired with zero or more Ed25519 signatures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedPathInfo {
    pub info: PathInfo,
    pub signatures: Vec<PathSignature>,
}

impl SignedPathInfo {
    pub fn new(info: PathInfo) -> Self {
        Self {
            info,
            signatures: Vec::new(),
        }
    }

    /// Sign `self.info` with `secret_key`, replacing any existing signature by the same signer.
    pub fn sign(&mut self, secret_key: &iroh::SecretKey) -> Result<()> {
        let msg = self.info.canonical_signing_bytes()?;
        let sig = secret_key.sign(&msg);
        let signer = secret_key.public();
        self.signatures.retain(|s| s.signer != signer);
        self.signatures.push(PathSignature {
            signer,
            signature: hex::encode(sig.to_bytes()),
        });
        Ok(())
    }

    /// Verify all signatures and return the list of cryptographically valid signers.
    pub fn verify_all_signatures(&self) -> Vec<iroh::PublicKey> {
        let Ok(msg) = self.info.canonical_signing_bytes() else {
            return Vec::new();
        };
        let mut valid = Vec::new();
        for entry in &self.signatures {
            let Ok(sig_bytes) = hex::decode(&entry.signature) else {
                continue;
            };
            let Ok(sig_arr): std::result::Result<[u8; 64], _> = sig_bytes.try_into() else {
                continue;
            };
            let sig = iroh::Signature::from_bytes(&sig_arr);
            if entry.signer.verify(&msg, &sig).is_ok() {
                valid.push(entry.signer);
            }
        }
        valid
    }

    /// Verify that at least one valid signature belongs to `trusted_keys`.
    pub fn verify_any(&self, trusted_keys: &[iroh::PublicKey]) -> Result<iroh::PublicKey> {
        let valid = self.verify_all_signatures();
        for signer in valid {
            if trusted_keys.contains(&signer) {
                return Ok(signer);
            }
        }
        Err(StoreError::UntrustedPath(self.info.store_key.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_traversal_rejected() {
        let m = StoreManifest {
            version: 1,
            store_key: format!("{}-foo", crate::path::compute_store_hash(b"foo")),
            kind: ManifestKind::Directory {
                entries: vec![TreeEntry::File {
                    path: "../escape".to_string(),
                    executable: false,
                    size: 1,
                    blob: iroh_blobs::Hash::EMPTY,
                }],
            },
            total_size: 1,
            references: vec![],
            drv_path: None,
        };
        assert!(m.validate().is_err());

        let abs = StoreManifest {
            kind: ManifestKind::Directory {
                entries: vec![TreeEntry::Dir {
                    path: "/etc/passwd".to_string(),
                }],
            },
            ..m
        };
        assert!(abs.validate().is_err());
    }
}
