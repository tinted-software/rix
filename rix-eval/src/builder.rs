//! Derivation build execution engine for Rix

use crate::error::{Error, Result};
use crate::store::Store;
use crate::value::NixValue;
use colored::Colorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build configuration options
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub verbose: bool,
    pub keep_failed: bool,
    pub dry_run: bool,
    pub out_link: Option<PathBuf>,
    pub sandbox: crate::sandbox::SandboxConfig,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            verbose: false,
            keep_failed: false,
            dry_run: false,
            out_link: Some(PathBuf::from("result")),
            sandbox: crate::sandbox::SandboxConfig::default(),
        }
    }
}

/// Result of building a derivation
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub drv_name: String,
    pub drv_path: String,
    pub outputs: HashMap<String, PathBuf>,
    pub cached: bool,
}

/// Derivation Builder
pub struct Builder {
    pub store: Store,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            store: Store::new(),
        }
    }

    /// Build a derivation value or attribute set
    pub fn build(
        &self,
        evaluator: &crate::eval::Evaluator,
        value: &NixValue,
        options: &BuildOptions,
    ) -> Result<BuildResult> {
        self.store.ensure_store_dir().map_err(Error::IoError)?;

        let forced = value.clone().force(evaluator)?;
        let attrs = match &forced {
            NixValue::AttributeSet(map) => map,
            other => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("Cannot build non-attribute-set value: {}", other),
                });
            }
        };

        // Extract derivation attributes
        let name = match attrs.get("name") {
            Some(v) => v.clone().force(evaluator)?.as_string()?,
            None => {
                let pname = match attrs.get("pname") {
                    Some(v) => v.clone().force(evaluator)?.as_string()?,
                    None => "unnamed".to_string(),
                };
                let version = match attrs.get("version") {
                    Some(v) => v.clone().force(evaluator)?.as_string()?,
                    None => "0.0.0".to_string(),
                };
                format!("{}-{}", pname, version)
            }
        };

        let system = match attrs.get("system") {
            Some(v) => v.clone().force(evaluator)?.as_string()?,
            None => "unknown".to_string(),
        };

        let builder = match attrs.get("builder") {
            Some(v) => v.clone().force(evaluator)?.to_string(),
            None => "/bin/sh".to_string(),
        };

        let args = match attrs.get("args") {
            Some(v) => match v.clone().force(evaluator)? {
                NixValue::List(l) => {
                    let mut res = Vec::new();
                    for item in l {
                        res.push(item.force(evaluator)?.to_string());
                    }
                    res
                }
                _ => Vec::new(),
            },
            None => Vec::new(),
        };

        // Extract outputs
        let mut output_names = Vec::new();
        if let Some(v) = attrs.get("outputs")
            && let Ok(NixValue::List(l)) = v.clone().force(evaluator)
        {
            for item in l {
                if let Ok(s) = item.force(evaluator)?.as_string() {
                    output_names.push(s);
                }
            }
        }
        if output_names.is_empty() {
            output_names.push("out".to_string());
        }

        // Collect environment variables
        let mut env = HashMap::new();
        for (k, v) in attrs {
            let key = k.to_string();
            let forced_v = v.clone().force(evaluator)?;
            let str_val = match &forced_v {
                NixValue::String(s) => s.clone(),
                NixValue::Path(p) => p.to_string_lossy().to_string(),
                NixValue::StorePath(p) => p.clone(),
                NixValue::Integer(i) => i.to_string(),
                NixValue::Boolean(b) => {
                    if *b {
                        "1".to_string()
                    } else {
                        "".to_string()
                    }
                }
                NixValue::List(l) => {
                    let mut parts = Vec::new();
                    for item in l {
                        let forced_item = item.clone().force(evaluator)?;
                        if let NixValue::AttributeSet(sub) = &forced_item {
                            if let Some(out_p) = sub.get("outPath").or_else(|| sub.get("out")) {
                                if let Ok(forced_out) = out_p.clone().force(evaluator) {
                                    parts.push(forced_out.to_string());
                                    continue;
                                }
                            }
                        }
                        parts.push(forced_item.to_string());
                    }
                    parts.join(" ")
                }
                NixValue::AttributeSet(sub) => {
                    if let Some(out_p) = sub.get("outPath").or_else(|| sub.get("out")) {
                        out_p.clone().force(evaluator)?.to_string()
                    } else {
                        "".to_string()
                    }
                }
                _ => forced_v.to_string(),
            };
            env.insert(key, str_val);
        }

        // Use the store paths computed during derivation evaluation
        let drv_path = match attrs.get("drvPath") {
            Some(v) => v.clone().force(evaluator)?.as_string()?,
            None => {
                let (drv, _) = self
                    .store
                    .compute_derivation_paths(&name, &output_names, &env);
                drv
            }
        };

        let mut output_paths = HashMap::new();
        for out_name in &output_names {
            if let Some(v) = attrs.get(out_name)
                && let Ok(s) = v.clone().force(evaluator)?.as_string()
            {
                output_paths.insert(out_name.clone(), s);
                continue;
            }
            if out_name == "out"
                && let Some(v) = attrs.get("outPath")
                && let Ok(s) = v.clone().force(evaluator)?.as_string()
            {
                output_paths.insert("out".to_string(), s);
                continue;
            }
            let (_, computed_outs) =
                self.store
                    .compute_derivation_paths(&name, &output_names, &env);
            if let Some(p) = computed_outs.get(out_name) {
                output_paths.insert(out_name.clone(), p.clone());
            }
        }

        let out_path = output_paths
            .get("out")
            .cloned()
            .unwrap_or_else(|| drv_path.clone());

        // Check if outputs already exist in store
        let all_outputs_exist = output_paths
            .values()
            .all(|p| self.store.is_valid_path(Path::new(p)));

        if all_outputs_exist {
            if options.verbose {
                println!(
                    "{} Derivation {} is already cached in store at {}",
                    "✓".green(),
                    name.bold(),
                    out_path.cyan()
                );
            }
            if let Some(link) = &options.out_link {
                create_symlink(Path::new(&out_path), link)?;
            }
            return Ok(BuildResult {
                drv_name: name,
                drv_path,
                outputs: output_paths
                    .into_iter()
                    .map(|(k, v)| (k, PathBuf::from(v)))
                    .collect(),
                cached: true,
            });
        }

        if options.dry_run {
            println!("Would build derivation: {}", name);
            return Ok(BuildResult {
                drv_name: name,
                drv_path,
                outputs: output_paths
                    .into_iter()
                    .map(|(k, v)| (k, PathBuf::from(v)))
                    .collect(),
                cached: false,
            });
        }

        println!(
            "{} {}",
            "==> Building derivation:".cyan().bold(),
            name.bold()
        );

        // Build prerequisite input derivations first
        if let Some(inputs_val) = attrs.get("__inputs")
            && let Ok(NixValue::List(inputs)) = inputs_val.clone().force(evaluator)
        {
            for input_drv in inputs {
                self.build(evaluator, &input_drv, options)?;
            }
        }

        // Scan environment for input store paths and build dependencies if any
        self.resolve_and_build_inputs(evaluator, &env, options)?;

        // Prepare temporary sandbox directory
        let temp_dir =
            std::env::temp_dir().join(format!("rix-build-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).map_err(Error::IoError)?;

        // Ensure output directories exist in store
        for path_str in output_paths.values() {
            let p = Path::new(path_str);
            let _ = std::fs::remove_dir_all(p);
            std::fs::create_dir_all(p).map_err(Error::IoError)?;
        }

        // Setup execution environment
        let mut cmd_env = HashMap::new();
        for (k, v) in &env {
            if k.starts_with("__") || k == "inputDrvs" || k == "meta" {
                continue;
            }
            cmd_env.insert(k.clone(), v.clone());
        }
        for (out_name, out_p) in &output_paths {
            cmd_env.insert(out_name.clone(), out_p.clone());
        }
        cmd_env.insert("name".to_string(), name.clone());
        cmd_env.insert("system".to_string(), system.clone());
        cmd_env.insert("NIX_BUILD_CORES".to_string(), num_cpus().to_string());
        cmd_env.insert("TMPDIR".to_string(), temp_dir.to_string_lossy().to_string());
        cmd_env.insert("TEMP".to_string(), temp_dir.to_string_lossy().to_string());
        cmd_env.insert("TMP".to_string(), temp_dir.to_string_lossy().to_string());
        cmd_env.insert("HOME".to_string(), temp_dir.to_string_lossy().to_string());
        let sandbox = options.sandbox.resolve();

        // Set Darwin / macOS defaults (only if not sandboxed or if SDK is explicitly configured)
        if cfg!(target_os = "macos") && !sandbox.enabled {
            cmd_env.insert("MACOSX_DEPLOYMENT_TARGET".to_string(), "14.0".to_string());
            if let Ok(sdk) = get_macos_sdk_path() {
                cmd_env.insert("SDKROOT".to_string(), sdk);
            }
        }

        // Collect store bin directories from outputs, env, and input derivations
        let mut store_bins = Vec::new();
        for out_p in output_paths.values() {
            store_bins.push(format!("{}/bin", out_p));
        }
        for v in env.values() {
            for token in v.split_whitespace() {
                if token.contains("/store/") {
                    store_bins.push(format!("{}/bin", token));
                }
            }
        }
        if let Some(inputs_val) = attrs.get("__inputs")
            && let Ok(NixValue::List(inputs)) = inputs_val.clone().force(evaluator)
        {
            for input in inputs {
                if let Ok(NixValue::AttributeSet(m)) = input.force(evaluator)
                    && let Some(out_p) = m.get("outPath")
                    && let Ok(s) = out_p.clone().force(evaluator)?.as_string()
                {
                    store_bins.push(format!("{}/bin", s));
                }
            }
        }

        let (script_path, mut cmd, sandbox_root) = if sandbox.enabled {
            // Populate chroot root
            let root = sandbox
                .create_root(&temp_dir, &name)
                .map_err(Error::IoError)?;
            let inputs = crate::sandbox::collect_input_store_paths(
                &env,
                &self.store.store_dir,
                &output_paths,
            );
            let pop = sandbox
                .populate(&root, &self.store.store_dir, &inputs)
                .map_err(Error::IoError)?;
            for w in &pop.warnings {
                if options.verbose {
                    eprintln!("{} {}", "sandbox warning:".yellow(), w);
                }
            }

            // Remap PATH to the hermetic sandbox PATH (no host dirs)
            cmd_env.insert("PATH".to_string(), sandbox.sandbox_path(&store_bins));
            let cmd_env = crate::sandbox::rewrite_env_for_sandbox(&cmd_env, &pop.path_rewrites);

            // Build script lives inside the chroot root
            let build_script = generate_build_script(&cmd_env, &builder, &args);
            let host_script_path = root.join("build/builder.sh");
            std::fs::write(&host_script_path, &build_script).map_err(Error::IoError)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &host_script_path,
                    std::fs::Permissions::from_mode(0o755),
                );
            }

            let mut c = Command::new("chroot");
            c.arg(&root)
                .arg("/bin/bash")
                .arg("-e")
                .arg("/build/builder.sh");
            c.envs(&cmd_env);
            (host_script_path, c, Some(root))
        } else {
            // Non-sandboxed legacy PATH construction (host tools permitted)
            let mut path_entries = store_bins;
            path_entries.push("/usr/local/bin".to_string());
            path_entries.push("/opt/homebrew/bin".to_string());
            path_entries.push("/usr/bin".to_string());
            path_entries.push("/bin".to_string());
            path_entries.push("/usr/sbin".to_string());
            path_entries.push("/sbin".to_string());
            if let Ok(home) = std::env::var("HOME") {
                path_entries.push(format!("{}/.cargo/bin", home));
                path_entries.push(format!("{}/.local/bin", home));
                path_entries.push(format!("{}/.gentoo/usr/bin", home));
            }
            if let Ok(existing_path) = std::env::var("PATH") {
                path_entries.push(existing_path);
            }
            cmd_env.insert("PATH".to_string(), path_entries.join(":"));

            let build_script = generate_build_script(&cmd_env, &builder, &args);
            let script_path = temp_dir.join("builder.sh");
            std::fs::write(&script_path, &build_script).map_err(Error::IoError)?;

            let mut c = Command::new("/bin/bash");
            c.arg("-e").arg(&script_path);
            c.current_dir(&temp_dir);
            c.envs(&cmd_env);
            (script_path, c, None)
        };
        let _ = script_path;

        let output = cmd.output().map_err(Error::IoError)?;

        // If sandboxed and execution succeeded, copy outputs back to real store
        if output.status.success() {
            if let Some(root) = &sandbox_root {
                for p in output_paths.values() {
                    let target = Path::new(p);
                    let rel = target.strip_prefix("/").unwrap_or(target);
                    let in_sandbox = root.join(rel);
                    if in_sandbox.exists() {
                        let _ = std::fs::remove_dir_all(target);
                        if let Some(parent) = target.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::rename(&in_sandbox, target) {
                            // Rename fails across mountpoints; fall back to recursive copy
                            if let Err(copy_err) = copy_tree(&in_sandbox, target) {
                                eprintln!(
                                    "{} Failed to copy sandbox output {} -> {}: {} (rename: {})",
                                    "Error:".red().bold(),
                                    in_sandbox.display(),
                                    target.display(),
                                    copy_err,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().unwrap_or(1);

            eprintln!(
                "{} Build command failed with code {}:\n{}\n{}",
                "Error:".red().bold(),
                code,
                stdout,
                stderr
            );

            if !options.keep_failed {
                for p in output_paths.values() {
                    let _ = std::fs::remove_dir_all(p);
                }
                let _ = std::fs::remove_dir_all(&temp_dir);
            }

            return Err(Error::UnsupportedExpression {
                reason: format!("Derivation {} build failed with exit code {}", name, code),
            });
        }

        // Clean up temporary build sandbox
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Verify output exists and is populated
        let out_dir = Path::new(&out_path);
        if !out_dir.exists() {
            return Err(Error::UnsupportedExpression {
                reason: format!(
                    "Derivation {} succeeded but output path {} was not created",
                    name, out_path
                ),
            });
        }

        let size_str = compute_dir_size_str(out_dir);
        println!(
            "   {} Successfully built {} ({})",
            "✓".green(),
            name.bold(),
            size_str
        );
        println!("    Installed to: {}", out_path.blue());

        // Create result symlink
        if let Some(link) = &options.out_link {
            create_symlink(out_dir, link)?;
            println!(
                "   {} Symlink created: {} -> {}",
                "✓".green(),
                link.display(),
                out_path
            );
        }

        Ok(BuildResult {
            drv_name: name,
            drv_path,
            outputs: output_paths
                .into_iter()
                .map(|(k, v)| (k, PathBuf::from(v)))
                .collect(),
            cached: false,
        })
    }

    /// Scan environment variables for referenced store paths and ensure they exist
    fn resolve_and_build_inputs(
        &self,
        _evaluator: &crate::eval::Evaluator,
        env: &HashMap<String, String>,
        options: &BuildOptions,
    ) -> Result<()> {
        let store_prefix = self.store.store_dir.to_str().unwrap_or("/nix/store");
        for val in env.values() {
            if val.contains(store_prefix) {
                // If it references a store path that doesn't exist yet, we check if it's in evaluator
                for token in val.split_whitespace() {
                    if token.starts_with(store_prefix)
                        && !Path::new(token).exists()
                        && options.verbose
                    {
                        println!("Input store path not found: {}", token);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Generate default builder shell script covering standard phases
fn generate_build_script(
    env: &HashMap<String, String>,
    _builder: &str,
    _args: &[String],
) -> String {
    let mut script = String::new();
    script.push_str("#!/usr/bin/env bash\nset -e\n\n");

    // If a custom buildCommand is provided (like crane/cargo-vendor-dir)
    if let Some(cmd) = env.get("buildCommand") {
        script.push_str("# Custom buildCommand\n");
        script.push_str(cmd);
        script.push('\n');
        return script;
    }

    // Standard phases
    script.push_str(r#"
mkdir -p "$out"

# Unpack Phase
if [ -n "$unpackPhase" ]; then
    eval "$unpackPhase"
elif [ -n "$src" ]; then
    if [ -f "$src" ]; then
        mkdir -p source
        if tar -tf "$src" >/dev/null 2>&1; then
            tar -xf "$src" -C source --strip-components=1 2>/dev/null || tar -xf "$src" -C source 2>/dev/null || tar -xf "$src"
            if [ -d source ] && [ "$(ls -A source 2>/dev/null)" ]; then
                cd source
            fi
        elif unzip -t "$src" >/dev/null 2>&1; then
            unzip -q "$src" -d source 2>/dev/null || unzip -q "$src"
            if [ -d source ] && [ "$(ls -A source 2>/dev/null)" ]; then
                cd source
            fi
        else
            cp "$src" .
        fi
        # Only enter single subdirectory if configure/Makefile/etc is NOT in current directory
        if [ ! -f "configure" ] && [ ! -f "configure.py" ] && [ ! -f "Makefile" ] && [ ! -f "makefile" ] && [ ! -f "CMakeLists.txt" ] && [ ! -f "Cargo.toml" ] && [ ! -f "install.sh" ]; then
            shopt -s nullglob
            subdirs=(*/)
            if [ ${#subdirs[@]} -eq 1 ] && [ -d "${subdirs[0]}" ]; then
                cd "${subdirs[0]}"
            fi
        fi
    fi
fi

# Patch Phase
if [ -n "$patchPhase" ]; then
    eval "$patchPhase"
elif [ -n "$patches" ]; then
    for p in $patches; do
        patch -p1 < "$p" 2>/dev/null || patch -p0 < "$p"
    done
fi

# Configure Phase
if [ -n "$configurePhase" ]; then
    eval "$configurePhase"
elif [ -x "./configure" ]; then
    ./configure --prefix="$out" $configureFlags
fi

# Build Phase
if [ -n "$buildPhase" ]; then
    eval "$buildPhase"
elif [ -f "Makefile" ] || [ -f "makefile" ] || [ -f "GNUmakefile" ]; then
    make -j"$NIX_BUILD_CORES" $makeFlags
fi

# Check Phase
if [ "$doCheck" = "1" ] || [ "$doCheck" = "true" ]; then
    if [ -n "$checkPhase" ]; then
        eval "$checkPhase"
    elif [ -f "Makefile" ] || [ -f "makefile" ] || [ -f "GNUmakefile" ]; then
        make check $checkFlags 2>/dev/null || make test $checkFlags 2>/dev/null || true
    fi
fi

# Install Phase
if [ -n "$installPhase" ]; then
    eval "$installPhase"
elif [ -f "Makefile" ] || [ -f "makefile" ] || [ -f "GNUmakefile" ]; then
    make install $installFlags
fi

# Fixup Phase
if [ -n "$fixupPhase" ]; then
    eval "$fixupPhase"
fi
"#);

    script
}

fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if link.exists() || link.is_symlink() {
        let _ = std::fs::remove_file(link);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
    }
    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn get_macos_sdk_path() -> std::io::Result<String> {
    let output = Command::new("xcrun").args(["--show-sdk-path"]).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Ok("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk".to_string())
    }
}

fn compute_dir_size_str(path: &Path) -> String {
    fn dir_size(path: &Path) -> u64 {
        let mut total: u64 = 0;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        total += dir_size(&entry.path());
                    } else {
                        total += metadata.len();
                    }
                }
            }
        }
        total
    }

    let total_bytes = dir_size(path);
    if total_bytes > 1024 * 1024 {
        format!("{:.1} MB", total_bytes as f64 / (1024.0 * 1024.0))
    } else if total_bytes > 1024 {
        format!("{:.1} KB", total_bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", total_bytes)
    }
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
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
        std::fs::copy(src, dst)?;
    }
    Ok(())
}
