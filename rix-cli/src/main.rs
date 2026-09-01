use clap::Parser;
use nix_eval::Evaluator;
use rootcause::{Report, report};
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

    // Create evaluator
    let evaluator = Evaluator::new();

    let value = if args.file {
        let path = args
            .expression
            .ok_or_else(|| report!("File path required when using --file"))?;
        evaluator
            .evaluate_from_file(std::path::Path::new(&path))
            .map_err(|e| report!("{}", e))?
    } else {
        let expr = if let Some(expr) = args.expression {
            expr
        } else {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        };
        evaluator.evaluate(&expr).map_err(|e| report!("{}", e))?
    };

    // Output the result
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", value);
    }

    Ok(())
}
