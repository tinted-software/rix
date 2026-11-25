use nix_eval::{BuildOptions, Builder, Evaluator, NixValue};
use rix_store::{
    BlobHash, P2pNode, RixConfig, Store, Substituter, compute_path_store_key, default_config_path,
    default_identity_path, extract_store_key, is_valid_store_key, load_identity,
    load_or_generate_identity, parse_public_key,
};
use rootcause::Report;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use usage::{Args, Cli, Subcommands};

/// Pure Rust Nix expression evaluator and package manager
#[derive(Cli, Debug, Clone)]
#[usage(
    name = "rix",
    bin = "rix",
    version,
    about = "Nix-compatible package manager"
)]
struct Cli {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// File to evaluate or build
    #[usage(short = 'f', long = "file", global)]
    file: Option<String>,

    /// Attribute path to select
    #[usage(short = 'A', long = "attr", global)]
    attr: Option<String>,

    /// Output symlink path (default: result)
    #[usage(short = 'o', long = "out-link", global)]
    out_link: Option<String>,

    /// Nix expression to evaluate
    #[usage(short = 'e', long = "expr", global)]
    expression: Option<String>,

    /// Build platform (for example x86_64-linux) exposed as builtins.currentSystem
    #[usage(long, global)]
    system: Option<String>,

    /// Host platform to cross compile for (for example x86_64-darwin)
    #[usage(long = "cross-system", global)]
    cross_system: Option<String>,

    /// Command used by packages to execute host-platform binaries during a cross build
    #[usage(long, global)]
    runner: Option<String>,

    /// Produce output in JSON format
    #[usage(long, global)]
    json: bool,

    /// Verbose output
    #[usage(short = 'v', long = "verbose", global)]
    verbose: bool,

    /// Keep failed build temporary directory
    #[usage(long = "keep-failed", global)]
    keep_failed: bool,

    /// Dry run (do not execute build commands)
    #[usage(long = "dry-run", global)]
    dry_run: bool,

    /// Disable P2P substitution for this invocation
    #[usage(long = "no-p2p", global)]
    no_p2p: bool,

    /// Extra peer EndpointId(s) to query for P2P substitution
    #[usage(long = "peer", global)]
    peers: Vec<String>,

    /// Extra trusted Ed25519 public key(s) for P2P path signatures
    #[usage(long = "trusted-key", global)]
    trusted_keys: Vec<String>,

    /// Allow substituting unsigned or untrusted P2P store paths
    #[usage(long = "allow-untrusted", global)]
    allow_untrusted: bool,
}

#[derive(Subcommands, Debug, Clone)]
enum Commands {
    /// Build a derivation from a Nix expression or file
    Build {
        /// File to build (default: default.nix)
        #[usage(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to build
        #[usage(short = 'A', long = "attr")]
        attr: Option<String>,

        /// Output symlink path (default: result)
        #[usage(short = 'o', long = "out-link")]
        out_link: Option<String>,

        /// Nix expression to evaluate and build
        #[usage(short = 'e', long = "expr")]
        expr: Option<String>,
    },

    /// Evaluate a Nix expression
    Eval {
        /// Nix expression to evaluate
        #[usage(short = 'e', long = "expr")]
        expr: Option<String>,

        /// File to evaluate
        #[usage(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to select
        #[usage(short = 'A', long = "attr")]
        attr: Option<String>,
    },

    /// Show derivation plan (ATerm or JSON)
    ShowDerivation {
        /// File to evaluate (default: default.nix)
        #[usage(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to show
        #[usage(short = 'A', long = "attr")]
        attr: Option<String>,
    },

    /// Opt-in P2P seeder serving local store paths over Iroh + Mainline DHT
    Seed(SeedArgs),

    /// Inspect, ingest, fetch, and verify paths in the iroh-blobs store
    Store(StoreArgs),

    /// Manage P2P identity, trusted public keys, and peer configuration
    P2p(P2pArgs),
}

#[derive(Args, Debug, Clone)]
struct SeedArgs {
    /// Disable Mainline DHT publishing (serve direct peers only)
    #[usage(long = "no-dht")]
    no_dht: bool,

    /// Seed only specific store path(s) or store key(s)
    #[usage(long = "path")]
    paths: Vec<String>,

    /// Bootstrap peer EndpointId(s) for peer exchange
    #[usage(long = "peer")]
    peers: Vec<String>,
}

#[derive(Args, Debug, Clone)]
struct StoreArgs {
    #[usage(subcommand)]
    command: StoreCommands,
}

#[derive(Subcommands, Debug, Clone)]
enum StoreCommands {
    /// List store paths registered in the iroh-blobs store
    List,

    /// Ingest a local file or directory into the iroh-blobs store
    Add(StoreAddArgs),

    /// Fetch a store path by store key or BLAKE3 root hash via P2P
    Fetch(StoreFetchArgs),

    /// Verify BLAKE3 integrity of stored collections
    Verify(StoreVerifyArgs),
}

#[derive(Args, Debug, Clone)]
struct StoreAddArgs {
    /// Name suffix for the resulting store path (defaults to file/dir name)
    #[usage(long)]
    name: Option<String>,

    /// Local file or directory to add to the store
    path: String,
}

#[derive(Args, Debug, Clone)]
struct StoreFetchArgs {
    /// Store key (<hash>-<name>), store path, or BLAKE3 root hash
    target: String,

    /// Peer EndpointId(s) to fetch from
    #[usage(long = "peer")]
    peers: Vec<String>,

    /// Trusted public key(s) to verify path signatures against
    #[usage(long = "trusted-key")]
    trusted_keys: Vec<String>,

    /// Allow fetching unsigned or untrusted store paths
    #[usage(long = "allow-untrusted")]
    allow_untrusted: bool,

    /// Disable Mainline DHT lookups
    #[usage(long = "no-dht")]
    no_dht: bool,
}

#[derive(Args, Debug, Clone)]
struct StoreVerifyArgs {
    /// Optional store key or path to verify (verifies all if omitted)
    store_key: Option<String>,
}

#[derive(Args, Debug, Clone)]
struct P2pArgs {
    #[usage(subcommand)]
    command: P2pCommands,
}

#[derive(Subcommands, Debug, Clone)]
enum P2pCommands {
    /// Show (or initialize with --init) the local Ed25519 EndpointId / signing key
    Id(P2pIdArgs),

    /// Add a trusted Ed25519 public key to config.toml
    Trust(P2pTrustArgs),

    /// Add a peer EndpointId to config.toml
    #[usage(name = "add-peer")]
    AddPeer(P2pAddPeerArgs),

    /// Print active store, identity, trusted keys, and P2P settings
    Status,
}

#[derive(Args, Debug, Clone)]
struct P2pIdArgs {
    /// Generate an identity key if one does not exist yet
    #[usage(long)]
    init: bool,
}

#[derive(Args, Debug, Clone)]
struct P2pTrustArgs {
    /// Ed25519 public key / EndpointId to trust
    pubkey: String,
}

#[derive(Args, Debug, Clone)]
struct P2pAddPeerArgs {
    /// Peer EndpointId to add
    endpoint_id: String,
}

fn split_csv(items: &[String]) -> Vec<String> {
    items
        .iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn main() -> Result<(), Report> {
    let cli = Cli::parse();
    // SAFETY: these run before any threads are spawned and before the evaluator
    // reads its environment, so there is no concurrent access.
    unsafe {
        if let Some(system) = &cli.system {
            std::env::set_var("RIX_SYSTEM", system);
        }
        if let Some(cross_system) = &cli.cross_system {
            std::env::set_var("RIX_CROSS_SYSTEM", cross_system);
        }
        if let Some(runner) = &cli.runner {
            std::env::set_var("RIX_TARGET_RUNNER", runner);
        }
    }
    let evaluator = Evaluator::new();
    let result = run(&evaluator, &cli);
    if let Err(e) = result {
        eprintln!("{}", evaluator.render_error(&e));
        std::process::exit(1);
    }
    Ok(())
}

fn run(evaluator: &Evaluator, cli: &Cli) -> nix_eval::Result<()> {
    // Determine command: default to Build if -f or -A is provided at top-level or program name is nix-build
    let prog_name = std::env::var("RIX_PROG_NAME")
        .ok()
        .or_else(|| std::env::args().next())
        .unwrap_or_default();
    let prog_base = prog_name
        .rsplit('/')
        .next()
        .unwrap_or(&prog_name)
        .to_string();
    let is_nix_build = prog_base.contains("nix-build") || prog_base.contains("opendarwin-nix");
    let is_nix_instantiate = prog_base.contains("nix-instantiate");

    match cli.command.clone() {
        Some(Commands::Build {
            file,
            attr,
            out_link,
            expr,
        }) => run_build(
            evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            out_link.or(cli.out_link.clone()),
            expr.or(cli.expression.clone()),
            cli,
        ),
        Some(Commands::Eval { expr, file, attr }) => run_eval(
            evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            expr.or(cli.expression.clone()),
            cli,
        ),
        Some(Commands::ShowDerivation { file, attr }) => run_show_derivation(
            evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            cli,
        ),
        Some(Commands::Seed(args)) => run_seed(args, cli).map_err(store_err_to_eval),
        Some(Commands::Store(args)) => run_store(args, cli).map_err(store_err_to_eval),
        Some(Commands::P2p(args)) => run_p2p(args, cli).map_err(store_err_to_eval),
        None => {
            if is_nix_instantiate {
                run_show_derivation(evaluator, cli.file.clone(), cli.attr.clone(), cli)
            } else if is_nix_build || cli.file.is_some() || cli.attr.is_some() {
                run_build(
                    evaluator,
                    cli.file.clone(),
                    cli.attr.clone(),
                    cli.out_link.clone(),
                    cli.expression.clone(),
                    cli,
                )
            } else if let Some(expr) = cli.expression.clone() {
                run_eval(evaluator, None, None, Some(expr), cli)
            } else {
                // If nothing specified and default.nix exists, build default.nix
                if Path::new("default.nix").exists() {
                    run_build(
                        evaluator,
                        Some("default.nix".to_string()),
                        None,
                        cli.out_link.clone(),
                        None,
                        cli,
                    )
                } else {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;
                    if buffer.trim().is_empty() {
                        println!(
                            "Usage: rix [build|eval|show-derivation|seed|store|p2p] [-f FILE] [-A ATTR]"
                        );
                        return Ok(());
                    }
                    run_eval(evaluator, None, None, Some(buffer), cli)
                }
            }
        }
    }
}

fn store_err_to_eval(err: rix_store::StoreError) -> nix_eval::Error {
    nix_eval::Error::EvaluationError {
        reason: err.to_string(),
    }
}

fn run_tokio<F, T>(fut: F) -> rix_store::Result<T>
where
    F: std::future::Future<Output = rix_store::Result<T>>,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(rix_store::StoreError::Io)?;
    rt.block_on(fut)
}

fn run_seed(args: SeedArgs, cli: &Cli) -> rix_store::Result<()> {
    let store = Store::new();
    store.ensure_store_dir()?;
    let secret_key = load_or_generate_identity(&default_identity_path())?;
    let enable_dht = store.config.p2p.enable_dht && !args.no_dht;

    let mut allowed: Option<HashSet<String>> = if args.paths.is_empty() {
        None
    } else {
        let mut set = HashSet::new();
        for p in split_csv(&args.paths) {
            if is_valid_store_key(&p) {
                set.insert(p);
            } else if let Some(k) = extract_store_key(&store.store_dir, Path::new(&p)) {
                set.insert(k);
            } else {
                return Err(rix_store::StoreError::InvalidPath(format!(
                    "not a valid store path or key: {p}"
                )));
            }
        }
        Some(set)
    };
    if let Some(ref mut set) = allowed
        && set.is_empty()
    {
        return Err(rix_store::StoreError::InvalidPath(
            "no valid store paths specified for --path".to_string(),
        ));
    }

    let mut bootstrap_peers = Vec::new();
    for s in store
        .config
        .p2p
        .peers
        .iter()
        .cloned()
        .chain(split_csv(&cli.peers))
        .chain(split_csv(&args.peers))
    {
        let pk = parse_public_key(&s)?;
        if !bootstrap_peers.contains(&pk) {
            bootstrap_peers.push(pk);
        }
    }

    run_tokio(async move {
        let bs = store.open_blob_store().await?;
        let mut paths = bs.list_paths().await?;
        if let Some(ref filter) = allowed {
            paths.retain(|k| filter.contains(k));
        }

        let handle = P2pNode::spawn_seeder(
            secret_key,
            bs.clone(),
            enable_dht,
            true,
            allowed,
            bootstrap_peers,
        )
        .await?;

        println!("Rix P2P seeder active");
        println!("  EndpointId / Public Key: {}", handle.endpoint_id());
        println!("  Store dir:               {}", store.store_dir.display());
        println!("  Blobs dir:               {}", store.blobs_dir.display());
        println!(
            "  Mainline DHT:            {}",
            if enable_dht { "enabled" } else { "disabled" }
        );
        println!("  Seeded store paths:      {}", paths.len());
        println!("Press Ctrl-C to stop seeding...");

        let _ = tokio::signal::ctrl_c().await;
        println!("\nShutting down seeder...");
        // Router::shutdown calls BlobsProtocol::shutdown on the underlying FsStore.
        handle.shutdown().await?;
        let _ = bs.shutdown().await;
        Ok(())
    })
}

fn run_store(args: StoreArgs, cli: &Cli) -> rix_store::Result<()> {
    let store = Store::new();
    store.ensure_store_dir()?;

    match args.command {
        StoreCommands::List => run_tokio(async move {
            let bs = store.open_blob_store().await?;
            let keys = bs.list_paths().await?;
            let mut rows = Vec::with_capacity(keys.len());
            for key in keys {
                let info = bs.get_path_info(&key).await?;
                let root = bs.get_root_hash(&key).await?;
                rows.push((key, root, info));
            }
            if cli.json {
                let json_rows: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|(k, r, info)| {
                        serde_json::json!({
                            "store_key": k,
                            "root_hash": r.map(|h| h.to_string()),
                            "total_size": info.as_ref().map(|i| i.info.total_size),
                            "signatures": info.as_ref().map(|i| i.signatures.len()).unwrap_or(0),
                            "references": info.as_ref().map(|i| i.info.references.clone()).unwrap_or_default(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_rows)
                        .map_err(|e| { rix_store::StoreError::Serialization(e.to_string()) })?
                );
            } else if rows.is_empty() {
                println!("No store paths registered in {}", store.blobs_dir.display());
            } else {
                for (k, r, info) in rows {
                    let hash_str = r
                        .map(|h| h.to_string())
                        .unwrap_or_else(|| "<missing>".to_string());
                    let size = info.as_ref().map(|i| i.info.total_size).unwrap_or(0);
                    let sigs = info.as_ref().map(|i| i.signatures.len()).unwrap_or(0);
                    println!("{k}  hash={hash_str}  size={size}B  sigs={sigs}");
                }
            }
            bs.shutdown().await?;
            Ok(())
        }),

        StoreCommands::Add(add_args) => {
            let src = PathBuf::from(&add_args.path);
            if !src.exists() && src.symlink_metadata().is_err() {
                return Err(rix_store::StoreError::NotFound(add_args.path));
            }
            let name = add_args.name.unwrap_or_else(|| {
                src.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "source".to_string())
            });
            let store_key = compute_path_store_key(&src, &name)?;
            let dest = store.store_dir.join(&store_key);

            if !store.is_valid_path(&dest) {
                copy_recursive(&src, &dest)?;
            }

            let signer = load_identity(&default_identity_path())?;
            run_tokio(async move {
                let bs = store.open_blob_store().await?;
                let signed = bs
                    .ingest_path(&store.store_dir, &store_key, &dest, None, signer.as_ref())
                    .await?;
                bs.shutdown().await?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "store_path": dest,
                            "store_key": store_key,
                            "root_hash": signed.info.root_hash.to_string(),
                            "total_size": signed.info.total_size,
                            "signatures": signed.signatures.len(),
                        }))
                        .map_err(|e| rix_store::StoreError::Serialization(e.to_string()))?
                    );
                } else {
                    println!("{}", dest.display());
                    if cli.verbose {
                        println!("  root_hash: {}", signed.info.root_hash);
                        println!("  size:      {} bytes", signed.info.total_size);
                    }
                }
                Ok(())
            })
        }

        StoreCommands::Fetch(fetch_args) => {
            let mut extra_peers = split_csv(&cli.peers);
            extra_peers.extend(split_csv(&fetch_args.peers));
            let mut extra_keys = split_csv(&cli.trusted_keys);
            extra_keys.extend(split_csv(&fetch_args.trusted_keys));

            let mut cfg = store.substitution_config(
                &extra_peers,
                &extra_keys,
                cli.allow_untrusted || fetch_args.allow_untrusted,
            )?;
            if fetch_args.no_dht {
                cfg.enable_dht = false;
            }

            let target = fetch_args.target.trim().to_string();
            let store_key = if is_valid_store_key(&target) {
                Some(target.clone())
            } else {
                extract_store_key(&store.store_dir, Path::new(&target))
            };

            run_tokio(async move {
                let bs = store.open_blob_store().await?;
                let sub = Substituter::new(bs.clone(), cfg.clone()).await?;
                let res = if let Some(key) = store_key {
                    let ok = sub.substitute_path(&store.store_dir, &key).await?;
                    if ok {
                        let out = store.store_dir.join(&key);
                        println!("{}", out.display());
                        Ok(())
                    } else {
                        Err(rix_store::StoreError::NotFound(key))
                    }
                } else {
                    let hash = BlobHash::from_str(&target).map_err(|e| {
                        rix_store::StoreError::InvalidPath(format!(
                            "target '{target}' is neither a valid store key nor a BLAKE3 hash: {e}"
                        ))
                    })?;
                    let out = sub
                        .fetch_by_hash(hash, cfg.peers.clone(), &store.store_dir)
                        .await?;
                    println!("{}", out.display());
                    Ok(())
                };
                sub.close().await;
                bs.shutdown().await?;
                res
            })
        }

        StoreCommands::Verify(verify_args) => run_tokio(async move {
            let bs = store.open_blob_store().await?;
            let keys = if let Some(target) = verify_args.store_key {
                let key = if is_valid_store_key(&target) {
                    target
                } else {
                    extract_store_key(&store.store_dir, Path::new(&target)).ok_or_else(|| {
                        rix_store::StoreError::InvalidPath(format!(
                            "invalid store key or path '{target}'"
                        ))
                    })?
                };
                vec![key]
            } else {
                bs.list_paths().await?
            };

            let mut verified = 0usize;
            for key in &keys {
                let report = bs.verify_path(key).await?;
                verified += 1;
                if cli.verbose {
                    println!(
                        "ok  {}  root={}  blobs={}  size={}",
                        report.store_key,
                        report.root_hash,
                        report.file_blob_count,
                        report.total_size
                    );
                }
            }
            println!("Verified {verified} store path(s) cleanly.");
            bs.shutdown().await?;
            Ok(())
        }),
    }
}

fn run_p2p(args: P2pArgs, cli: &Cli) -> rix_store::Result<()> {
    let id_path = default_identity_path();
    let cfg_path = default_config_path();

    match args.command {
        P2pCommands::Id(id_args) => {
            if id_args.init {
                let sk = load_or_generate_identity(&id_path)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "endpoint_id": sk.public().to_string(),
                            "identity_path": id_path,
                        }))
                        .map_err(|e| rix_store::StoreError::Serialization(e.to_string()))?
                    );
                } else {
                    println!("{}", sk.public());
                }
            } else if let Some(sk) = load_identity(&id_path)? {
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "endpoint_id": sk.public().to_string(),
                            "identity_path": id_path,
                        }))
                        .map_err(|e| rix_store::StoreError::Serialization(e.to_string()))?
                    );
                } else {
                    println!("{}", sk.public());
                }
            } else {
                return Err(rix_store::StoreError::NotFound(format!(
                    "no identity key at {} (pass --init to create one)",
                    id_path.display()
                )));
            }
            Ok(())
        }

        P2pCommands::Trust(trust_args) => {
            let pk = parse_public_key(&trust_args.pubkey)?;
            let pk_str = pk.to_string();
            let mut cfg = RixConfig::load_from(&cfg_path)?;
            if !cfg.p2p.trusted_keys.contains(&pk_str) {
                cfg.p2p.trusted_keys.push(pk_str.clone());
                cfg.save_to(&cfg_path)?;
            }
            println!("Trusted key {pk_str} saved to {}", cfg_path.display());
            Ok(())
        }

        P2pCommands::AddPeer(peer_args) => {
            let pk = parse_public_key(&peer_args.endpoint_id)?;
            let id_str = pk.to_string();
            let mut cfg = RixConfig::load_from(&cfg_path)?;
            if !cfg.p2p.peers.contains(&id_str) {
                cfg.p2p.peers.push(id_str.clone());
                cfg.save_to(&cfg_path)?;
            }
            println!("Peer {id_str} saved to {}", cfg_path.display());
            Ok(())
        }

        P2pCommands::Status => {
            let store = Store::new();
            let identity = load_identity(&id_path)?.map(|sk| sk.public().to_string());
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "store_dir": store.store_dir,
                        "blobs_dir": store.blobs_dir,
                        "config_path": cfg_path,
                        "identity_path": id_path,
                        "endpoint_id": identity,
                        "p2p": store.config.p2p,
                    }))
                    .map_err(|e| rix_store::StoreError::Serialization(e.to_string()))?
                );
            } else {
                println!("Rix Store & P2P Status");
                println!("  Store dir:       {}", store.store_dir.display());
                println!("  Blobs dir:       {}", store.blobs_dir.display());
                println!("  Config path:     {}", cfg_path.display());
                println!(
                    "  Identity:        {}",
                    identity.unwrap_or_else(|| "<not initialized>".to_string())
                );
                println!("  P2P enabled:     {}", store.config.p2p.enabled);
                println!("  Opt-in seeding:  {}", store.config.p2p.seed);
                println!("  Mainline DHT:    {}", store.config.p2p.enable_dht);
                println!("  Allow untrusted: {}", store.config.p2p.allow_untrusted);
                println!("  Trusted keys:    {:?}", store.config.p2p.trusted_keys);
                println!("  Configured peers:{:?}", store.config.p2p.peers);
            }
            Ok(())
        }
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> rix_store::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = std::fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst)?;
        return Ok(());
    }
    if ft.is_file() {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(src, dst)?;
        #[cfg(unix)]
        std::fs::set_permissions(dst, meta.permissions())?;
        return Ok(());
    }
    if ft.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn resolve_expression(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    expr: Option<String>,
) -> nix_eval::Result<NixValue> {
    let mut root_val = if let Some(path_str) = file {
        evaluator.evaluate_from_file(Path::new(&path_str))?
    } else if let Some(expression_str) = expr {
        evaluator.evaluate(&expression_str)?
    } else if Path::new("default.nix").exists() {
        evaluator.evaluate_from_file(Path::new("default.nix"))?
    } else {
        return Err(nix_eval::Error::EvaluationError {
            reason: "No expression or file provided to evaluate".to_string(),
        });
    };

    // Auto-call function if root value is a function taking an attribute set
    root_val = unwrap_function(evaluator, root_val)?;

    // Select attribute path if requested
    if let Some(attr_path) = attr {
        for part in attr_path.split('.') {
            root_val = root_val.force(evaluator)?;
            match root_val {
                NixValue::AttributeSet(map) => {
                    let next =
                        map.get(part)
                            .cloned()
                            .ok_or_else(|| nix_eval::Error::EvaluationError {
                                reason: format!("Attribute '{}' not found in package set", part),
                            })?;
                    root_val = unwrap_function(evaluator, next)?;
                }
                other => {
                    return Err(nix_eval::Error::EvaluationError {
                        reason: format!(
                            "Cannot select attribute '{}' from non-attribute-set: {}",
                            part, other
                        ),
                    });
                }
            }
        }
    }

    unwrap_function(evaluator, root_val)
}

fn unwrap_function(evaluator: &Evaluator, mut val: NixValue) -> nix_eval::Result<NixValue> {
    val = val.force(evaluator)?;
    while matches!(val, NixValue::Function(_)) {
        if let NixValue::Function(func) = val {
            let empty_set = NixValue::AttributeSet(HashMap::new());
            val = func.apply(evaluator, empty_set)?;
            val = val.force(evaluator)?;
        }
    }
    Ok(val)
}

fn run_build(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    out_link: Option<String>,
    expr: Option<String>,
    cli: &Cli,
) -> nix_eval::Result<()> {
    let value = resolve_expression(evaluator, file, attr, expr)?;

    let builder = Builder::new();
    let options = BuildOptions {
        verbose: cli.verbose,
        keep_failed: cli.keep_failed,
        dry_run: cli.dry_run,
        out_link: out_link
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("result"))),
        build_platform: cli.system.clone(),
        host_platform: cli.cross_system.clone(),
        runner: cli.runner.clone(),
        p2p: !cli.no_p2p,
        peers: split_csv(&cli.peers),
        trusted_keys: split_csv(&cli.trusted_keys),
        allow_untrusted: cli.allow_untrusted,
    };

    let result = builder.build(evaluator, &value, &options)?;

    if cli.json {
        let json_map: HashMap<String, String> = result
            .outputs
            .into_iter()
            .map(|(k, v)| (k, v.to_string_lossy().to_string()))
            .collect();
        let json_str = serde_json::to_string_pretty(&json_map).map_err(|e| {
            nix_eval::Error::EvaluationError {
                reason: e.to_string(),
            }
        })?;
        println!("{}", json_str);
    }

    Ok(())
}

fn run_eval(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    expr: Option<String>,
    cli: &Cli,
) -> nix_eval::Result<()> {
    let value = resolve_expression(evaluator, file, attr, expr)?;
    if cli.json {
        let json_str =
            serde_json::to_string_pretty(&value).map_err(|e| nix_eval::Error::EvaluationError {
                reason: e.to_string(),
            })?;
        println!("{}", json_str);
    } else {
        println!("{}", value);
    }
    Ok(())
}

fn run_show_derivation(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    cli: &Cli,
) -> nix_eval::Result<()> {
    let value = resolve_expression(evaluator, file, attr, None)?;
    let forced = value.force(evaluator)?;

    match forced {
        NixValue::AttributeSet(map) => {
            if cli.json {
                let json_str = serde_json::to_string_pretty(&map).map_err(|e| {
                    nix_eval::Error::EvaluationError {
                        reason: e.to_string(),
                    }
                })?;
                println!("{}", json_str);
            } else {
                println!("{}", NixValue::AttributeSet(map));
            }
        }
        other => {
            println!("{}", other);
        }
    }
    Ok(())
}
