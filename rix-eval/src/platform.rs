//! Build/host platform handling for cross compilation.
//!
//! Rix uses the same "system triple" vocabulary as Nix: a `cpu-os` pair such
//! as `x86_64-linux`, `aarch64-darwin` or `x86_64-windows`.  Two platforms are
//! tracked during a build:
//!
//! * the *build* platform is the machine performing the build (the machine
//!   running `rix`), and
//! * the *host* platform is the machine the produced package runs on.
//!
//! When they differ the build is a *cross* build and, depending on the pair,
//! binaries for the host platform cannot be executed natively while the build
//! runs.  [`ExecutionStrategy`] captures how (if at all) that can be worked
//! around.

use std::fmt;

/// A parsed Nix system triple such as `x86_64-linux`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Platform {
    /// CPU component, e.g. `x86_64`, `aarch64`, `riscv64`.
    pub cpu: String,
    /// Operating system component, e.g. `linux`, `darwin`, `windows`.
    pub os: String,
}

impl Platform {
    /// Parse a `cpu-os` triple.  Unknown components are accepted so that new
    /// platforms do not require an evaluator change; only a missing separator
    /// is rejected.
    pub fn parse(triple: &str) -> Option<Self> {
        let (cpu, os) = triple.split_once('-')?;
        if cpu.is_empty() || os.is_empty() {
            return None;
        }
        Some(Self {
            cpu: cpu.to_string(),
            os: os.to_string(),
        })
    }

    /// The platform `rix` itself is running on, derived from its own target
    /// triple.  This mirrors `builtins.currentSystem`.
    pub fn current() -> Self {
        let cpu = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "riscv64") {
            "riscv64"
        } else {
            "unknown"
        };
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        };
        Self {
            cpu: cpu.to_string(),
            os: os.to_string(),
        }
    }

    pub fn is_linux(&self) -> bool {
        self.os == "linux"
    }

    pub fn is_darwin(&self) -> bool {
        self.os == "darwin"
    }

    pub fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    /// True when a binary built for `self` can run directly on the machine
    /// performing the build.
    pub fn is_native_to(&self, build: &Platform) -> bool {
        self.cpu == build.cpu && self.os == build.os
    }

    /// The canonical `cpu-os` spelling.
    pub fn triple(&self) -> String {
        format!("{}-{}", self.cpu, self.os)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.triple())
    }
}

/// How host-platform binaries can be executed while a cross build is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Build and host platforms match; binaries run directly.
    Native,
    /// A user-supplied runner command prefixes the target executable, e.g.
    /// `darling` for Darwin binaries on Linux or `wine` for Windows binaries.
    Runner { command: String },
    /// A `qemu-user-static` style emulator handles a foreign CPU running the
    /// same OS (or a Linux userspace) via binfmt_misc.  The value is the
    /// emulator binary that is transparently invoked by the kernel.
    Emulated { emulator: String },
    /// No known way to execute host binaries on this build platform.
    Unsupported { reason: String },
}

/// Decide how (if at all) host-platform binaries can run on the build machine.
///
/// `runner` is an explicit user override (`--runner`) and always wins; Rix
/// deliberately does not guess a runner from `PATH` so that a misconfigured
/// cross build fails loudly instead of silently doing the wrong thing.
pub fn execution_strategy(
    build: &Platform,
    host: &Platform,
    runner: Option<&str>,
) -> ExecutionStrategy {
    if host.is_native_to(build) {
        return ExecutionStrategy::Native;
    }

    if let Some(runner) = runner.map(str::trim).filter(|r| !r.is_empty()) {
        return ExecutionStrategy::Runner {
            command: runner.to_string(),
        };
    }

    // Same OS, different CPU: a qemu-user-static style emulator registered with
    // binfmt_misc can run the binary transparently.
    if build.os == host.os && build.cpu != host.cpu {
        let arch = match host.cpu.as_str() {
            "aarch64" => "aarch64",
            "x86_64" => "x86_64",
            "riscv64" => "riscv64",
            "armv7l" | "armv7" => "arm",
            other => other,
        };
        return ExecutionStrategy::Emulated {
            emulator: format!("qemu-{arch}-static"),
        };
    }

    // Foreign OS: only a userspace compatibility layer can help.
    if build.is_linux() && host.is_darwin() {
        return ExecutionStrategy::Unsupported {
            reason: "running Darwin binaries on a Linux build host requires a Darwin \
                     userspace runner such as Darling; pass --runner darling (or a \
                     Darling wrapper) to enable it"
                .to_string(),
        };
    }

    if build.is_linux() && host.is_windows() {
        return ExecutionStrategy::Unsupported {
            reason: "running Windows binaries on a Linux build host requires Wine; \
                     pass --runner wine (or a Wine wrapper) to enable it"
                .to_string(),
        };
    }

    ExecutionStrategy::Unsupported {
        reason: format!(
            "no known way to execute {}-{} binaries on a {} build host; pass --runner \
             to provide one",
            host.cpu, host.os, build
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(triple: &str) -> Platform {
        Platform::parse(triple).unwrap()
    }

    #[test]
    fn parses_and_prints_triples() {
        let plat = p("x86_64-darwin");
        assert_eq!(plat.cpu, "x86_64");
        assert_eq!(plat.os, "darwin");
        assert_eq!(plat.triple(), "x86_64-darwin");
        assert!(Platform::parse("x86_64").is_none());
        assert!(Platform::parse("-darwin").is_none());
        assert!(Platform::parse("x86_64-").is_none());
    }

    #[test]
    fn native_build_needs_no_runner() {
        let build = p("x86_64-linux");
        let host = p("x86_64-linux");
        assert_eq!(
            execution_strategy(&build, &host, None),
            ExecutionStrategy::Native
        );
    }

    #[test]
    fn explicit_runner_wins_for_cross_builds() {
        let build = p("x86_64-linux");
        let host = p("x86_64-darwin");
        assert_eq!(
            execution_strategy(&build, &host, Some("darling")),
            ExecutionStrategy::Runner {
                command: "darling".to_string()
            }
        );
    }

    #[test]
    fn darwin_without_runner_is_unsupported() {
        let build = p("x86_64-linux");
        let host = p("x86_64-darwin");
        match execution_strategy(&build, &host, None) {
            ExecutionStrategy::Unsupported { reason } => assert!(reason.contains("Darling")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn foreign_cpu_same_os_uses_qemu() {
        let build = p("x86_64-linux");
        let host = p("aarch64-linux");
        assert_eq!(
            execution_strategy(&build, &host, None),
            ExecutionStrategy::Emulated {
                emulator: "qemu-aarch64-static".to_string()
            }
        );
    }

    #[test]
    fn windows_without_runner_is_unsupported() {
        let build = p("x86_64-linux");
        let host = p("x86_64-windows");
        match execution_strategy(&build, &host, None) {
            ExecutionStrategy::Unsupported { reason } => assert!(reason.contains("Wine")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn current_is_a_valid_triple() {
        let current = Platform::current();
        assert_eq!(Platform::parse(&current.triple()).unwrap(), current);
    }
}
