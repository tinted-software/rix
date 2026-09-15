//! Nix store path calculations, receipts, and management

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NIX_BASE32_CHARS: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Nix base32 encoding (little-endian, 5 bits per character)
pub fn nix_base32_encode(bytes: &[u8]) -> String {
    let len = 32;
    let mut result = String::with_capacity(len);
    for n in (0..len).rev() {
        let b = n * 5;
        let i = b / 8;
        let o = b % 8;
        let mut v = if i < bytes.len() {
            (bytes[i] >> o) as u32
        } else {
            0
        };
        if i + 1 < bytes.len() {
            v |= (bytes[i + 1] as u32) << (8 - o);
        }
        let c = (v & 0x1f) as usize;
        result.push(NIX_BASE32_CHARS[c] as char);
    }
    result
}

/// Compute a 32-character Nix store hash for arbitrary input bytes
pub fn compute_store_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    nix_base32_encode(&hash)
}

/// Nix Store manager
#[derive(Debug, Clone)]
pub struct Store {
    pub store_dir: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// Initialize the Nix store, determining the active store directory
    pub fn new() -> Self {
        let store_dir = if let Ok(dir) = std::env::var("NIX_STORE_DIR") {
            PathBuf::from(dir)
        } else if Path::new("/nix/store").is_dir() && is_dir_writable(Path::new("/nix/store")) {
            PathBuf::from("/nix/store")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".rix").join("store")
        };

        Self { store_dir }
    }

    /// Ensure the store directory exists
    pub fn ensure_store_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.store_dir)
    }

    /// Compute a store path for a named derivation or file
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

    /// Compute derivation store path and its output paths
    pub fn compute_derivation_paths(
        &self,
        name: &str,
        outputs: &[String],
        env: &HashMap<String, String>,
    ) -> (String, HashMap<String, String>) {
        // Collect sorted environment entries to produce a deterministic hash
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

    /// Check if a store path already exists and is populated
    pub fn is_valid_path(&self, path: &Path) -> bool {
        if !path.exists() {
            return false;
        }
        if path.is_file() {
            return true;
        }
        if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            return entries.count() > 0;
        }
        false
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nix_base32_hash() {
        let data = b"hello world";
        let hash = compute_store_hash(data);
        assert_eq!(hash.len(), 32);
        for c in hash.chars() {
            assert!(NIX_BASE32_CHARS.contains(&(c as u8)));
        }
    }
}
