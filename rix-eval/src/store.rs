//! Nix store path calculations, receipts, and management (backed by `rix-store`)

pub use rix_store::{Store, SubstitutionConfig, compute_store_hash, nix_base32_encode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nix_base32_hash() {
        let data = b"hello world";
        let hash = compute_store_hash(data);
        assert_eq!(hash.len(), 32);
        for c in hash.chars() {
            assert!(rix_store::path::NIX_BASE32_CHARS.contains(&(c as u8)));
        }
    }
}
