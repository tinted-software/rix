//! Ed25519 identity and key management built on `iroh::SecretKey` / `PublicKey`.

use crate::error::{Result, StoreError};
use std::path::Path;
use std::str::FromStr;

/// Load the local `iroh::SecretKey` from `path` if it exists, or return `Ok(None)`.
pub fn load_identity(path: &Path) -> Result<Option<iroh::SecretKey>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let trimmed = text.trim();
    let bytes = hex::decode(trimmed)
        .map_err(|e| StoreError::Crypto(format!("invalid hex in {}: {e}", path.display())))?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| {
        StoreError::Crypto(format!(
            "identity key in {} must be 32 bytes (64 hex chars)",
            path.display()
        ))
    })?;
    Ok(Some(iroh::SecretKey::from_bytes(&arr)))
}

/// Load an existing `iroh::SecretKey` from `path`, or generate and save a new one with `0o600` permissions.
pub fn load_or_generate_identity(path: &Path) -> Result<iroh::SecretKey> {
    if let Some(key) = load_identity(path)? {
        return Ok(key);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = generate_ephemeral_identity();
    let hex_key = hex::encode(key.to_bytes());
    std::fs::write(path, format!("{hex_key}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

/// Generate a fresh, non-persisted Ed25519 secret key for client/leecher sessions.
pub fn generate_ephemeral_identity() -> iroh::SecretKey {
    iroh::SecretKey::generate()
}

/// Convert an `iroh::SecretKey` into a `n0_mainline::SigningKey`.
pub fn to_mainline_signing_key(secret: &iroh::SecretKey) -> n0_mainline::SigningKey {
    n0_mainline::SigningKey::from_bytes(&secret.to_bytes())
}

/// Parse an `iroh::PublicKey` (`EndpointId`) from string.
pub fn parse_public_key(s: &str) -> Result<iroh::PublicKey> {
    iroh::PublicKey::from_str(s.trim())
        .map_err(|e| StoreError::Crypto(format!("invalid public key / EndpointId '{s}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PathInfo, SignedPathInfo};
    use crate::path::compute_store_hash;

    #[test]
    fn test_identity_roundtrip_and_signing() {
        let tmp = tempfile::tempdir().unwrap();
        let id_path = tmp.path().join("identity.key");
        let sk1 = load_or_generate_identity(&id_path).unwrap();
        let sk2 = load_identity(&id_path).unwrap().unwrap();
        assert_eq!(sk1.to_bytes(), sk2.to_bytes());

        let other = generate_ephemeral_identity();
        let store_key = format!("{}-pkg", compute_store_hash(b"pkg"));
        let info = PathInfo::new(
            store_key.clone(),
            iroh_blobs::Hash::new(b"root"),
            42,
            vec![],
            None,
        );
        let mut signed = SignedPathInfo::new(info);
        signed.sign(&sk1).unwrap();

        assert_eq!(signed.verify_any(&[sk1.public()]).unwrap(), sk1.public());
        assert!(signed.verify_any(&[other.public()]).is_err());

        // Tampering with timestamp or store_key invalidates the signature
        let mut tampered = signed.clone();
        tampered.info.total_size = 999;
        assert!(tampered.verify_any(&[sk1.public()]).is_err());
    }
}
