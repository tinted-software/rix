use clap::{Parser, Subcommand};
use nix_eval::{BuildOptions, Builder, Evaluator, NixValue};
use rootcause::{Report, report};
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Pure Rust Nix expression evaluator and package manager
#[derive(Parser, Debug, Clone)]
#[command(name = "rix", author, version, about = "Nix-compatible package manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// File to evaluate or build
    #[arg(short = 'f', long = "file", global = true)]
    file: Option<String>,

    /// Attribute path to select
    #[arg(short = 'A', long = "attr", global = true)]
    attr: Option<String>,

    /// Output symlink path (default: result)
    #[arg(short = 'o', long = "out-link", global = true)]
    out_link: Option<String>,

    /// Nix expression to evaluate
    #[arg(short = 'e', long = "expr", global = true)]
    expression: Option<String>,

    /// Produce output in JSON format
    #[arg(long, global = true)]
    json: bool,

    /// Verbose output
    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,

    /// Keep failed build temporary directory
    #[arg(long = "keep-failed", global = true)]
    keep_failed: bool,

    /// Dry run (do not execute build commands)
    #[arg(long = "dry-run", global = true)]
    dry_run: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Build a derivation from a Nix expression or file
    Build {
        /// File to build (default: default.nix)
        #[arg(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to build
        #[arg(short = 'A', long = "attr")]
        attr: Option<String>,

        /// Output symlink path (default: result)
        #[arg(short = 'o', long = "out-link")]
        out_link: Option<String>,

        /// Nix expression to evaluate and build
        #[arg(short = 'e', long = "expr")]
        expr: Option<String>,
    },

    /// Evaluate a Nix expression
    Eval {
        /// Nix expression to evaluate
        #[arg(short = 'e', long = "expr")]
        expr: Option<String>,

        /// File to evaluate
        #[arg(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to select
        #[arg(short = 'A', long = "attr")]
        attr: Option<String>,
    },

    /// Show derivation plan (ATerm or JSON)
    ShowDerivation {
        /// File to evaluate (default: default.nix)
        #[arg(short = 'f', long = "file")]
        file: Option<String>,

        /// Attribute path to show
        #[arg(short = 'A', long = "attr")]
        attr: Option<String>,
    },
}

fn main() -> Result<(), Report> {
    let cli = Cli::parse();
    let evaluator = Evaluator::new();

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
            &evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            out_link.or(cli.out_link.clone()),
            expr.or(cli.expression.clone()),
            &cli,
        ),
        Some(Commands::Eval { expr, file, attr }) => run_eval(
            &evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            expr.or(cli.expression.clone()),
            &cli,
        ),
        Some(Commands::ShowDerivation { file, attr }) => run_show_derivation(
            &evaluator,
            file.or(cli.file.clone()),
            attr.or(cli.attr.clone()),
            &cli,
        ),
        None => {
            if is_nix_instantiate {
                run_show_derivation(&evaluator, cli.file.clone(), cli.attr.clone(), &cli)
            } else if is_nix_build || cli.file.is_some() || cli.attr.is_some() {
                run_build(
                    &evaluator,
                    cli.file.clone(),
                    cli.attr.clone(),
                    cli.out_link.clone(),
                    cli.expression.clone(),
                    &cli,
                )
            } else if let Some(expr) = cli.expression.clone() {
                run_eval(&evaluator, None, None, Some(expr), &cli)
            } else {
                // If nothing specified and default.nix exists, build default.nix
                if Path::new("default.nix").exists() {
                    run_build(
                        &evaluator,
                        Some("default.nix".to_string()),
                        None,
                        cli.out_link.clone(),
                        None,
                        &cli,
                    )
                } else {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;
                    if buffer.trim().is_empty() {
                        println!("Usage: rix [build|eval|show-derivation] [-f FILE] [-A ATTR]");
                        return Ok(());
                    }
                    run_eval(&evaluator, None, None, Some(buffer), &cli)
                }
            }
        }
    }
}

fn resolve_expression(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    expr: Option<String>,
) -> Result<NixValue, Report> {
    let mut root_val = if let Some(path_str) = file {
        evaluator
            .evaluate_from_file(Path::new(&path_str))
            .map_err(|e| report!("{}", e))?
    } else if let Some(expression_str) = expr {
        evaluator
            .evaluate(&expression_str)
            .map_err(|e| report!("{}", e))?
    } else if Path::new("default.nix").exists() {
        evaluator
            .evaluate_from_file(Path::new("default.nix"))
            .map_err(|e| report!("{}", e))?
    } else {
        return Err(report!("No expression or file provided to evaluate"));
    };

    // Auto-call function if root value is a function taking an attribute set
    root_val = unwrap_function(evaluator, root_val)?;

    // Select attribute path if requested
    if let Some(attr_path) = attr {
        for part in attr_path.split('.') {
            root_val = root_val.force(evaluator).map_err(|e| report!("{}", e))?;
            match root_val {
                NixValue::AttributeSet(map) => {
                    let next = map
                        .get(part)
                        .cloned()
                        .ok_or_else(|| report!("Attribute '{}' not found in package set", part))?;
                    root_val = unwrap_function(evaluator, next)?;
                }
                other => {
                    return Err(report!(
                        "Cannot select attribute '{}' from non-attribute-set: {}",
                        part,
                        other
                    ));
                }
            }
        }
    }

    unwrap_function(evaluator, root_val)
}

fn unwrap_function(evaluator: &Evaluator, mut val: NixValue) -> Result<NixValue, Report> {
    val = val.force(evaluator).map_err(|e| report!("{}", e))?;
    while matches!(val, NixValue::Function(_)) {
        if let NixValue::Function(func) = val {
            let empty_set = NixValue::AttributeSet(HashMap::new());
            val = func
                .apply(evaluator, empty_set)
                .map_err(|e| report!("{}", e))?;
            val = val.force(evaluator).map_err(|e| report!("{}", e))?;
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
) -> Result<(), Report> {
    let value = resolve_expression(evaluator, file, attr, expr)?;

    let builder = Builder::new();
    let options = BuildOptions {
        verbose: cli.verbose,
        keep_failed: cli.keep_failed,
        dry_run: cli.dry_run,
        out_link: out_link
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("result"))),
    };

    let result = builder
        .build(evaluator, &value, &options)
        .map_err(|e| report!("{}", e))?;

    if cli.json {
        let json_map: HashMap<String, String> = result
            .outputs
            .into_iter()
            .map(|(k, v)| (k, v.to_string_lossy().to_string()))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_map)?);
    }

    Ok(())
}

fn run_eval(
    evaluator: &Evaluator,
    file: Option<String>,
    attr: Option<String>,
    expr: Option<String>,
    cli: &Cli,
) -> Result<(), Report> {
    let value = resolve_expression(evaluator, file, attr, expr)?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
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
) -> Result<(), Report> {
    let value = resolve_expression(evaluator, file, attr, None)?;
    let forced = value.force(evaluator).map_err(|e| report!("{}", e))?;

    match forced {
        NixValue::AttributeSet(map) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&map)?);
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
