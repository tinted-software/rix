//! Nix-compatible store path hashing, parsing, and reference scanning.

use crate::error::Result;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

pub const NIX_BASE32_CHARS: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Nix base32 encoding (little-endian, 5 bits per character).
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

/// Compute a 32-character Nix store hash for arbitrary input bytes.
pub fn compute_store_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    nix_base32_encode(&hash)
}

/// Check whether `s` is a valid 32-character Nix base32 hash.
pub fn is_valid_nix_hash(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| NIX_BASE32_CHARS.contains(&b))
}

/// Split a store key (`<32-char-hash>-<name>`) into `(hash, name)`.
pub fn split_store_key(store_key: &str) -> Option<(&str, &str)> {
    let (hash, name) = store_key.split_once('-')?;
    if !is_valid_nix_hash(hash) || name.is_empty() || name.contains('/') || name.contains('\0') {
        return None;
    }
    if name == "." || name == ".." {
        return None;
    }
    Some((hash, name))
}

/// Validate that `store_key` has the form `<32-char-nix-base32>-<name>` (or `<32-char>-<name>.drv`).
pub fn is_valid_store_key(store_key: &str) -> bool {
    split_store_key(store_key).is_some()
}

/// Extract the `<hash>-<name>` store key from a full path inside `store_dir`.
///
/// Accepts either a direct child of `store_dir` or a subpath inside one, as well as
/// a bare store key if `full_path` has no parent directory and already validates.
pub fn extract_store_key(store_dir: &Path, full_path: &Path) -> Option<String> {
    if let Ok(rel) = full_path.strip_prefix(store_dir) {
        let first = rel.components().next()?;
        let key = first.as_os_str().to_str()?;
        if is_valid_store_key(key) {
            return Some(key.to_string());
        }
        return None;
    }
    if full_path.parent().is_none_or(|p| p.as_os_str().is_empty())
        && let Some(key) = full_path.to_str()
        && is_valid_store_key(key)
    {
        return Some(key.to_string());
    }
    None
}

/// Compute a deterministic `<hash>-<name>` store key for a local file or directory tree.
pub fn compute_path_store_key(path: &Path, name: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    fn visit(root: &Path, cur: &Path, hasher: &mut Sha256) -> Result<()> {
        let meta = std::fs::symlink_metadata(cur)?;
        let rel = cur.strip_prefix(root).unwrap_or(cur);
        hasher.update(rel.to_string_lossy().as_bytes());
        let ft = meta.file_type();
        if ft.is_symlink() {
            hasher.update(b"L:");
            hasher.update(std::fs::read_link(cur)?.to_string_lossy().as_bytes());
        } else if ft.is_file() {
            hasher.update(b"F:");
            let mut f = std::fs::File::open(cur)?;
            let mut buf = [0u8; 65536];
            loop {
                let n = std::io::Read::read(&mut f, &mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
        } else if ft.is_dir() {
            hasher.update(b"D:");
            let mut children: Vec<std::path::PathBuf> = std::fs::read_dir(cur)?
                .filter_map(|e| e.ok().map(|x| x.path()))
                .collect();
            children.sort();
            for child in children {
                visit(root, &child, hasher)?;
            }
        }
        Ok(())
    }
    visit(path, path, &mut hasher)?;
    let hash = nix_base32_encode(&hasher.finalize());
    Ok(format!("{hash}-{name}"))
}

/// Scan `root` (file, symlink, or directory tree) for references to other store keys
/// under `store_dir`. Does not follow symlinks; excludes `self_key`.
pub fn scan_references(store_dir: &Path, root: &Path, self_key: &str) -> Result<Vec<String>> {
    let prefix = format!("{}/", store_dir.to_string_lossy().trim_end_matches('/'));
    let prefix_bytes = prefix.as_bytes();
    let mut refs = BTreeSet::new();
    scan_path_recursive(root, prefix_bytes, self_key, &mut refs)?;
    Ok(refs.into_iter().collect())
}

fn scan_path_recursive(
    path: &Path,
    prefix: &[u8],
    self_key: &str,
    refs: &mut BTreeSet<String>,
) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        if let Ok(target) = std::fs::read_link(path) {
            scan_bytes(target.to_string_lossy().as_bytes(), prefix, self_key, refs);
        }
        return Ok(());
    }
    if ft.is_file() {
        // Stream-scan in overlapping chunks so arbitrarily large files don't blow memory.
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let overlap = prefix.len() + 256;
        let mut buf = vec![0u8; 64 * 1024 + overlap];
        let mut carry = 0usize;
        loop {
            let n = file.read(&mut buf[carry..])?;
            if n == 0 {
                if carry > 0 {
                    scan_bytes(&buf[..carry], prefix, self_key, refs);
                }
                break;
            }
            let total = carry + n;
            scan_bytes(&buf[..total], prefix, self_key, refs);
            if total > overlap {
                let tail_start = total - overlap;
                buf.copy_within(tail_start..total, 0);
                carry = overlap;
            } else {
                carry = total;
            }
        }
        return Ok(());
    }
    if ft.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            scan_path_recursive(&entry.path(), prefix, self_key, refs)?;
        }
    }
    Ok(())
}

fn scan_bytes(haystack: &[u8], prefix: &[u8], self_key: &str, refs: &mut BTreeSet<String>) {
    if prefix.is_empty() || haystack.len() < prefix.len() + 34 {
        return;
    }
    let mut i = 0;
    while let Some(rel) = find_subslice(&haystack[i..], prefix) {
        let start = i + rel + prefix.len();
        if start + 34 <= haystack.len() {
            let hash_slice = &haystack[start..start + 32];
            if hash_slice.iter().all(|b| NIX_BASE32_CHARS.contains(b))
                && haystack.get(start + 32) == Some(&b'-')
            {
                let mut end = start + 33;
                while end < haystack.len() && is_store_name_byte(haystack[end]) {
                    end += 1;
                }
                if end > start + 33
                    && let Ok(key) = std::str::from_utf8(&haystack[start..end])
                    && key != self_key
                    && !key.ends_with(".drv")
                    && is_valid_store_key(key)
                {
                    refs.insert(key.to_string());
                }
                i = end;
                continue;
            }
        }
        i = start;
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn is_store_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.' | b'_' | b'?' | b'=')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_key_validation_and_extraction() {
        let hash = compute_store_hash(b"hello");
        let key = format!("{hash}-hello-1.0");
        assert!(is_valid_store_key(&key));
        assert_eq!(split_store_key(&key), Some((hash.as_str(), "hello-1.0")));

        let store_dir = Path::new("/nix/store");
        let full = store_dir.join(&key).join("bin/hello");
        assert_eq!(
            extract_store_key(store_dir, &full).as_deref(),
            Some(key.as_str())
        );

        assert!(!is_valid_store_key("invalid-key"));
        assert!(!is_valid_store_key(&format!("{hash}-")));
        assert!(!is_valid_store_key(&format!("{hash}-..")));
    }

    #[test]
    fn test_scan_references() {
        let tmp = tempfile::tempdir().unwrap();
        let store_dir = tmp.path().join("store");
        let dep_hash = compute_store_hash(b"dep");
        let app_hash = compute_store_hash(b"app");
        let dep_key = format!("{dep_hash}-dep-1.0");
        let app_key = format!("{app_hash}-app-1.0");

        let app_dir = store_dir.join(&app_key);
        std::fs::create_dir_all(app_dir.join("bin")).unwrap();
        let script = format!(
            "#!/bin/sh\nexec {}/{}/bin/tool \"$@\"\n",
            store_dir.display(),
            dep_key
        );
        std::fs::write(app_dir.join("bin/run"), script).unwrap();
        // Self-reference should be excluded
        std::fs::write(
            app_dir.join("self.txt"),
            format!("{}/{}", store_dir.display(), app_key),
        )
        .unwrap();

        let refs = scan_references(&store_dir, &app_dir, &app_key).unwrap();
        assert_eq!(refs, vec![dep_key]);
    }
}
