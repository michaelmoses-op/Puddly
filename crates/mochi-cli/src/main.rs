//! Mochi CLI. Every command is a thin wrapper over a library function.

use clap::{Parser, Subcommand};
use mochi_check::{check, format_check};
use mochi_core::{CLI_NAME, DISPLAY_NAME};
use mochi_diagnostics::explain_text;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = CLI_NAME, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Parse and validate `*.resource.ts` files
    Check {
        /// App root (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Emit stable JSON diagnostics
        #[arg(long)]
        json: bool,
    },
    /// Explain an error code with an example
    Explain {
        /// Error code, e.g. E0204
        code: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Check { path, json } => {
            let result = check(&path);
            let out = if result.ok && !json {
                let n = result.ir.as_ref().map(|i| i.resources.len()).unwrap_or(0);
                format!(
                    "{DISPLAY_NAME} check: ok — {n} resource{}\n",
                    if n == 1 { "" } else { "s" }
                )
            } else {
                format_check(&result, json)
            };
            if !out.is_empty() {
                print!("{out}");
                if !out.ends_with('\n') {
                    println!();
                }
            }
            if result.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Commands::Explain { code } => match explain_text(&code) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(1)
            }
        },
    }
}
