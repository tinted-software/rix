//! Chroot-based build sandbox for Rix.
//!
//! Goal: builds run against a standalone bootstrap toolchain instead of
//! assuming host tools exist on `PATH` (`/usr/bin`, `/bin`, Homebrew, …).
//!
//! Design (deliberately boring, root-required for now):
//! - Caller assembles a chroot root containing the bootstrap tarball
//!   (llvm 23.1.1 + mold-macho + bash + coreutils + xcbuild + curl +
//!   libarchive/bsdtar) plus the derivation's input store paths.
//! - The build script runs under `chroot <root> /bin/bash …` (requires root;
//!   a privileged daemon can own this invocation later without changing the
//!   layout).
//! - Outputs are written to sandbox-local paths (`/out`, `/tmp`) and copied
//!   back to the real store after the child exits. No bind mounts.
//!
//! dyld + Libc hermeticity is explicitly out of scope for this pass.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Files every sandboxed build expects, relative to the chroot root.
pub const SANDBOX_BASH: &str = "bin/bash";

/// Environment knob pointing at a prebuilt bootstrap tarball:
/// `llvm 23.1.1 + mold-macho + bash + coreutils + xcbuild + curl + bsdtar`.
pub const BOOTSTRAP_TARBALL_ENV: &str = "RIX_BOOTSTRAP_TARBALL";

/// Sandbox configuration resolved from CLI flags / environment.
#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    /// Whether to run this build inside the chroot sandbox.
    pub enabled: bool,
    /// Optional bootstrap tarball (`.tar.gz`/`.tar.xz`/`.tar.zst`). Falls back
    /// to `$RIX_BOOTSTRAP_TARBALL` when `None`.
    pub bootstrap_tarball: Option<PathBuf>,
}

impl SandboxConfig {
    pub fn resolve(&self) -> ResolvedSandbox {
        let tarball = self.bootstrap_tarball.clone().or_else(|| {
            std::env::var(BOOTSTRAP_TARBALL_ENV)
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
        ResolvedSandbox {
            enabled: self.enabled,
            bootstrap_tarball: tarball,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedSandbox {
    pub enabled: bool,
    pub bootstrap_tarball: Option<PathBuf>,
}

impl ResolvedSandbox {
    /// PATH visible inside the sandbox: bootstrap + derivation inputs only.
    /// No host directories.
    pub fn sandbox_path(&self, input_bins: &[String]) -> String {
        let mut entries: Vec<String> = vec![
            "/bin".to_string(),
            "/usr/bin".to_string(),
            "/toolchain/bin".to_string(),
        ];
        for bin in input_bins {
            if !entries.contains(bin) {
                entries.push(bin.clone());
            }
        }
        entries.join(":")
    }

    /// Materialize an empty chroot root with std dirs. Returns the root path.
    pub fn create_root(&self, workdir: &Path, name: &str) -> std::io::Result<PathBuf> {
        let root = workdir.join(format!("sandbox-root-{}", sanitize(name)));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "bin",
            "usr/bin",
            "toolchain/bin",
            "tmp",
            "out",
            "build",
            "nix/store",
            "etc",
        ] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        Ok(root)
    }

    /// Unpack the bootstrap tarball into the chroot root (if configured) and
    /// hardlink-tree input store paths under their same absolute location.
    ///
    /// Absolute symlinks would re-resolve inside the chroot and loop onto
    /// themselves, so inputs are hardlinked (cheap, same fs) with a full-copy
    /// fallback across devices.
    pub fn populate(
        &self,
        root: &Path,
        store_dir: &Path,
        input_store_paths: &[PathBuf],
    ) -> std::io::Result<Population> {
        let mut warnings = Vec::new();
        let mut rewrites: HashMap<String, String> = HashMap::new();

        if let Some(tarball) = &self.bootstrap_tarball {
            if tarball.exists() {
                extract_tarball(tarball, root)?;
            } else {
                warnings.push(format!(
                    "bootstrap tarball not found: {}",
                    tarball.display()
                ));
            }
        } else {
            warnings.push(format!(
                "no bootstrap tarball configured (set ${} or --bootstrap-tarball); falling back to input store paths only",
                BOOTSTRAP_TARBALL_ENV
            ));
        }

        let store_rel = store_dir.strip_prefix("/").unwrap_or(store_dir);
        for input in input_store_paths {
            let rel = match input.strip_prefix("/") {
                Ok(r) => r.to_path_buf(),
                Err(_) => PathBuf::from(input.file_name().unwrap_or_default()),
            };
            if rel.starts_with(store_rel) || store_rel.as_os_str().is_empty() {
                let dest = root.join(&rel);
                if dest.exists() || dest.is_symlink() {
                    continue;
                }
                if Path::new(input).exists() {
                    link_or_copy_recursively(input, &dest)?;
                }
                // Identity mapping: mirrored at the same absolute path.
                rewrites.insert(
                    input.to_string_lossy().to_string(),
                    format!("/{}", rel.to_string_lossy()),
                );
            } else {
                warnings.push(format!(
                    "input outside store dir, not mirrored: {}",
                    input.display()
                ));
            }
        }

        // Last-resort shell for non-bootstrap dev flows: copy bytes, never
        // symlink (a symlink to /bin/bash loops inside the chroot).
        if !root.join(SANDBOX_BASH).exists() {
            warnings.push("bootstrap has no bin/bash; copying host /bin/bash".to_string());
            if Path::new("/bin/bash").exists()
                && let Some(parent) = Path::new(SANDBOX_BASH).parent()
            {
                let _ = std::fs::create_dir_all(root.join(parent));
                let _ = std::fs::copy("/bin/bash", root.join(SANDBOX_BASH));
            }
        }

        Ok(Population {
            warnings,
            path_rewrites: rewrites,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Population {
    pub warnings: Vec<String>,
    pub path_rewrites: HashMap<String, String>,
}

/// Rewrite absolute store paths in an env map using the population rewrites.
/// Longest-prefix match so nested paths resolve correctly.
pub fn rewrite_env_for_sandbox(
    env: &HashMap<String, String>,
    rewrites: &HashMap<String, String>,
) -> HashMap<String, String> {
    if rewrites.is_empty() {
        return env.clone();
    }
    let mut keys: Vec<&String> = rewrites.keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    env.iter()
        .map(|(k, v)| {
            let mut out = v.clone();
            for old in &keys {
                if out.contains(old.as_str()) {
                    out = out.replace(old.as_str(), &rewrites[*old]);
                }
            }
            (k.clone(), out)
        })
        .collect()
}

/// Collect store-path-looking tokens from an env map plus explicit output
/// paths. Used to decide which inputs to mirror into the chroot.
pub fn collect_input_store_paths(
    env: &HashMap<String, String>,
    store_dir: &Path,
    output_paths: &HashMap<String, String>,
) -> Vec<PathBuf> {
    let prefix = store_dir.to_string_lossy().to_string();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: &str| {
        let pb = PathBuf::from(p);
        if !out.contains(&pb) {
            out.push(pb);
        }
    };
    for v in env.values() {
        for token in v.split(|c: char| c.is_whitespace() || c == ':') {
            if token.starts_with(&prefix) {
                // Trim trailing /bin, /lib suffixes back to the store entry.
                let entry = store_entry_for(token, &prefix);
                push(&entry);
            }
        }
    }
    for p in output_paths.values() {
        let _ = p;
    }
    out.sort();
    out
}

fn store_entry_for(token: &str, prefix: &str) -> String {
    let rest = token.strip_prefix(prefix).unwrap_or(token);
    let rest = rest.trim_start_matches('/');
    let first = rest.split('/').next().unwrap_or("");
    if first.is_empty() {
        token.to_string()
    } else {
        format!("{}/{}", prefix.trim_end_matches('/'), first)
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn extract_tarball(tarball: &Path, root: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(tarball)
        .arg("-C")
        .arg(root)
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(std::io::Error::other(format!(
            "cannot extract bootstrap tarball {} (tar failed)",
            tarball.display()
        ))),
    }
}

fn link_or_copy_recursively(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            link_or_copy_recursively(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if src.is_symlink() {
        let target = std::fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dst)?;
        #[cfg(not(unix))]
        std::fs::copy(src, dst)?;
    } else {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        if std::fs::hard_link(src, dst).is_err() {
            std::fs::copy(src, dst)?;
        }
        #[cfg(unix)]
        {
            if let Ok(meta) = std::fs::metadata(src) {
                let _ = std::fs::set_permissions(dst, meta.permissions());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_path_has_no_host_dirs() {
        let cfg = ResolvedSandbox {
            enabled: true,
            bootstrap_tarball: None,
        };
        let path = cfg.sandbox_path(&["/nix/store/abc/bin".to_string()]);
        assert!(path.contains("/nix/store/abc/bin"));
        for host in ["/usr/local/bin", "/opt/homebrew/bin", "/sbin"] {
            assert!(!path.split(':').any(|e| e == host));
        }
    }

    #[test]
    fn collect_finds_store_entries() {
        let env = HashMap::from([
            (
                "CC".to_string(),
                "/nix/store/abc-clang/bin/clang".to_string(),
            ),
            (
                "P".to_string(),
                "/nix/store/abc-clang/bin:/nix/store/def-make/bin".to_string(),
            ),
        ]);
        let found = collect_input_store_paths(&env, Path::new("/nix/store"), &HashMap::new());
        assert!(found.contains(&PathBuf::from("/nix/store/abc-clang")));
        assert!(found.contains(&PathBuf::from("/nix/store/def-make")));
    }

    #[test]
    fn bootstrap_env_fallback_does_not_panic() {
        let cfg = SandboxConfig {
            enabled: true,
            bootstrap_tarball: None,
        };
        let resolved = cfg.resolve();
        assert!(resolved.enabled);
    }
}
