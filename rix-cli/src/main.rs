use clap::Parser;
use nix_eval::Evaluator;
use rootcause::{Report, report};
use serde_json;
use std::fs;
use std::io::{self, Read};

/// A pure Rust Nix expression evaluator
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Nix expression to evaluate (or file path if --file is used)
    #[arg(short, long = "expr", value_name = "EXPRESSION")]
    expression: Option<String>,

    /// Read expression from file instead of command line
    #[arg(short, long)]
    file: bool,

    /// Produce output in JSON format, suitable for consumption by another program.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<(), Report> {
    let args = Args::parse();

    // Get the Nix expression to evaluate
    let expr = if args.file {
        // Read from file
        let path = args
            .expression
            .ok_or_else(|| report!("File path required when using --file"))?;
        fs::read_to_string(&path)?
    } else if let Some(expr) = args.expression {
        // Use provided expression
        expr
    } else {
        // Read from stdin
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    // Create evaluator and evaluate
    let evaluator = Evaluator::new();
    let value = evaluator.evaluate(&expr).map_err(|e| report!("{}", e))?;

    // Output the result
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", value);
    }

    Ok(())
}
