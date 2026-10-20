//! Puddly CLI. Every command is a thin wrapper over a library function.

use clap::{Parser, Subcommand};
use puddly_check::{check, format_check};
use puddly_core::{CLI_NAME, DISPLAY_NAME};
use puddly_diagnostics::explain_text;
use puddly_manifest::{
    build_manifest, stale_message, stale_status, write_manifest, StaleStatus,
};
use puddly_db::PostgresExecutor;
use puddly_migrate::{
    apply_migrations, drift_message, drift_status, generate_migration, DriftStatus,
};
use puddly_query::{
    format_impact, format_inspect, impact, inspect, QueryTarget,
};
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
    /// Generate or verify the committed `.puddly/manifest.json`
    Manifest {
        #[command(subcommand)]
        command: ManifestCmd,
    },
    /// Inspect a resource, field, or action from the manifest
    Inspect {
        /// Target: `todo`, `todo.title`, or `todo.action:archive`
        target: String,
        /// App root
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show impact / dependents of a resource or field
    Impact {
        /// Target: `todo` or `todo.owner`
        target: String,
        /// App root
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate or verify committed SQL migrations
    Migrate {
        #[command(subcommand)]
        command: MigrateCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ManifestCmd {
    /// Validate the app and write `.puddly/manifest.json`
    Generate {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Fail if the committed manifest is missing or stale (for CI)
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum MigrateCmd {
    /// Generate `migrations/*.sql` from the current schema IR
    Generate {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Allow DROP TABLE / DROP COLUMN in generated SQL
        #[arg(long)]
        allow_destructive: bool,
    },
    /// Fail if migrations are missing or the schema fingerprint drifted (CI)
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Apply pending SQL migrations (requires DATABASE_URL)
    Apply {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Postgres URL (defaults to $DATABASE_URL)
        #[arg(long)]
        database_url: Option<String>,
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
            print_out(&out);
            exit_bool(result.ok)
        }
        Commands::Explain { code } => match explain_text(&code) {
            Ok(text) => {
                print_out(&text);
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(1)
            }
        },
        Commands::Manifest { command } => match command {
            ManifestCmd::Generate { path } => {
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                let manifest = build_manifest(&path, &ir);
                match write_manifest(&path, &manifest) {
                    Ok(p) => {
                        println!(
                            "{DISPLAY_NAME} manifest: wrote {} ({} resources)",
                            p.display(),
                            manifest.resources.len()
                        );
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("error: failed to write manifest: {e}");
                        ExitCode::from(1)
                    }
                }
            }
            ManifestCmd::Check { path, json } => {
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, json));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                let expected = build_manifest(&path, &ir);
                let status = stale_status(&path, &expected);
                if json {
                    let body = serde_json::json!({
                        "ok": status == StaleStatus::Fresh,
                        "status": match status {
                            StaleStatus::Fresh => "fresh",
                            StaleStatus::Stale => "stale",
                            StaleStatus::Missing => "missing",
                        },
                        "message": stale_message(&status),
                    });
                    print_out(&format!("{}\n", serde_json::to_string_pretty(&body).unwrap()));
                } else {
                    println!("{}", stale_message(&status));
                }
                exit_bool(status == StaleStatus::Fresh)
            }
        },
        Commands::Inspect { target, path, json } => {
            match load_manifest_for_query(&path) {
                Ok(manifest) => match QueryTarget::parse(&target) {
                    Ok(t) => {
                        let result = inspect(&manifest, &t);
                        print_out(&format_inspect(&result, json));
                        exit_bool(result.ok)
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                },
                Err(code) => code,
            }
        }
        Commands::Impact { target, path, json } => {
            match load_manifest_for_query(&path) {
                Ok(manifest) => match QueryTarget::parse(&target) {
                    Ok(t) => {
                        let result = impact(&manifest, &t);
                        print_out(&format_impact(&result, json));
                        exit_bool(result.ok)
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                },
                Err(code) => code,
            }
        }
        Commands::Migrate { command } => match command {
            MigrateCmd::Generate {
                path,
                allow_destructive,
            } => {
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                match generate_migration(&path, &ir, allow_destructive) {
                    Ok(gen) => {
                        for w in &gen.warnings {
                            eprintln!("warning: {w}");
                        }
                        println!("{DISPLAY_NAME} migrate: {}", gen.message);
                        if gen.message.contains("refusing") {
                            ExitCode::from(1)
                        } else {
                            ExitCode::SUCCESS
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                }
            }
            MigrateCmd::Check { path, json } => {
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, json));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                let status = drift_status(&path, &ir);
                if json {
                    let body = serde_json::json!({
                        "ok": matches!(status, DriftStatus::Fresh),
                        "status": match &status {
                            DriftStatus::Fresh => "fresh",
                            DriftStatus::Missing => "missing",
                            DriftStatus::Drift { .. } => "drift",
                        },
                        "message": drift_message(&status),
                    });
                    print_out(&format!("{}\n", serde_json::to_string_pretty(&body).unwrap()));
                } else {
                    println!("{}", drift_message(&status));
                }
                exit_bool(matches!(status, DriftStatus::Fresh))
            }
            MigrateCmd::Apply { path, database_url } => {
                let url = database_url
                    .or_else(|| std::env::var("DATABASE_URL").ok())
                    .filter(|s| !s.is_empty());
                let Some(url) = url else {
                    eprintln!(
                        "error: migrate apply requires --database-url or $DATABASE_URL"
                    );
                    return ExitCode::from(1);
                };
                // Refuse silent apply when migrations are drifted (fingerprint check).
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                let status = drift_status(&path, &ir);
                if !matches!(status, DriftStatus::Fresh) {
                    eprintln!("{}", drift_message(&status));
                    eprintln!("error: refuse to apply while migrations are missing or drifted");
                    return ExitCode::from(1);
                }
                match PostgresExecutor::connect(&url) {
                    Ok(mut exec) => match apply_migrations(&path, &mut exec) {
                        Ok(r) => {
                            println!("{DISPLAY_NAME} migrate apply: {}", r.message);
                            for name in &r.applied {
                                println!("  applied {name}");
                            }
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("error: {e}");
                            ExitCode::from(1)
                        }
                    },
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                }
            }
        },
    }
}

/// Prefer on-disk committed manifest; if missing, build from sources (dev convenience).
fn load_manifest_for_query(
    path: &std::path::Path,
) -> Result<puddly_manifest::Manifest, ExitCode> {
    match puddly_manifest::read_manifest(path) {
        Ok(Some(m)) => Ok(m),
        Ok(None) => {
            let result = check(path);
            if !result.ok {
                eprint!("{}", format_check(&result, false));
                return Err(ExitCode::from(1));
            }
            Ok(build_manifest(path, result.ir.as_ref().unwrap()))
        }
        Err(e) => {
            eprintln!("error: {e}");
            Err(ExitCode::from(1))
        }
    }
}

fn print_out(out: &str) {
    if out.is_empty() {
        return;
    }
    print!("{out}");
    if !out.ends_with('\n') {
        println!();
    }
}

fn exit_bool(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
