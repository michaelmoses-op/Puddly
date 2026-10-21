//! Puddly CLI. Every command is a thin wrapper over a library function.

use clap::{Parser, Subcommand};
use puddly_auth::scaffold_auth_migration;
use puddly_check::{check, format_check};
use puddly_core::{CLI_NAME, DISPLAY_NAME};
use puddly_diagnostics::explain_text;
use puddly_manifest::{
    build_manifest, init_app_with_options, stale_message, stale_status, write_manifest,
    InitOptions, StaleStatus,
};
use puddly_db::{PgPool, PoolConfig, PostgresExecutor};
use puddly_migrate::{
    apply_migrations, apply_migrations_dry_run, drift_message, drift_status, generate_migration,
    DriftStatus,
};
use puddly_query::{
    format_impact, format_inspect, impact, inspect, QueryTarget,
};
use puddly_actions::{package_action_sidecar, ACTION_SIDECAR_ENV, GENERATED_SIDECAR_SCRIPT};
use puddly_rpc::{write_ts_client, RpcServer, GENERATED_CLIENT_TS};
use puddly_server::{
    errors_from_check, resolve_serve_env, serve, serve_dev, watch_dev_loop, DeployEnv,
    DevHandles, DevState, ServerConfig,
};
use puddly_views::ViewBundle;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use tokio::sync::Mutex;

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
    /// Built-in auth helpers (opt-in DDL scaffold)
    Auth {
        #[command(subcommand)]
        command: AuthCmd,
    },
    /// Run the minimal sync HTTP RPC server (ADR-024)
    Serve {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Bind address (default 127.0.0.1:3000)
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Postgres URL (defaults to $DATABASE_URL)
        #[arg(long)]
        database_url: Option<String>,
        /// Set Secure on session cookies
        #[arg(long)]
        secure_cookies: bool,
    },
    /// Incremental compile + hot reload + `/__dev` panel (ADR-018 / ADR-030)
    Dev {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Bind address (default 127.0.0.1:3000)
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Postgres URL (defaults to $DATABASE_URL). Optional: without it, serve
        /// views + `/__dev` only (no RPC pool).
        #[arg(long)]
        database_url: Option<String>,
        /// Set Secure on session cookies (ignored in view-only mode)
        #[arg(long)]
        secure_cookies: bool,
    },
    /// Package action sidecar + typed TS client into `.puddly/generated/` (ADR-031 / ADR-038)
    Build {
        /// App root (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Typed TypeScript RPC client (ADR-038 / Step 20)
    Client {
        #[command(subcommand)]
        command: ClientCmd,
    },
    /// Scaffold a minimal app (features + sample resource + CONVENTIONS) (ADR-039)
    Init {
        /// Directory to create / fill (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also write Dockerfile + docker-compose.yml (explicit migrate apply) (ADR-041)
        #[arg(long)]
        docker: bool,
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
        /// List pending migrations without executing SQL or writing the ledger
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCmd {
    /// Commit opt-in `migrations/*_puddly_auth.sql` from `AUTH_SCHEMA_SQL`
    Scaffold {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ClientCmd {
    /// Write `.puddly/generated/client.ts` from the app IR
    Generate {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
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
            MigrateCmd::Apply {
                path,
                database_url,
                dry_run,
            } => {
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
                match PostgresExecutor::connect(&url).await {
                    Ok(mut exec) => {
                        let outcome = if dry_run {
                            apply_migrations_dry_run(&path, &mut exec).await
                        } else {
                            apply_migrations(&path, &mut exec).await
                        };
                        match outcome {
                            Ok(r) => {
                                println!("{DISPLAY_NAME} migrate apply: {}", r.message);
                                for name in &r.applied {
                                    if dry_run {
                                        println!("  would apply {name}");
                                    } else {
                                        println!("  applied {name}");
                                    }
                                }
                                for name in &r.skipped {
                                    println!("  skipped {name}");
                                }
                                ExitCode::SUCCESS
                            }
                            Err(e) => {
                                eprintln!("error: {e}");
                                ExitCode::from(1)
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                }
            }
        },
        Commands::Auth { command } => match command {
            AuthCmd::Scaffold { path } => match scaffold_auth_migration(&path) {
                Ok(r) => {
                    println!("{DISPLAY_NAME} auth: {}", r.message);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            },
        },
        Commands::Serve {
            path,
            bind,
            database_url,
            secure_cookies,
        } => {
            let env = match resolve_serve_env(database_url, secure_cookies) {
                Ok(e) => e,
                Err(msg) => {
                    eprintln!("error: {msg}");
                    return ExitCode::from(1);
                }
            };
            if env.deploy == DeployEnv::Production && !secure_cookies {
                eprintln!(
                    "{DISPLAY_NAME} serve: PUDDLY_ENV=production — forcing Secure session cookies"
                );
            }
            let result = check(&path);
            if !result.ok {
                print_out(&format_check(&result, false));
                return ExitCode::from(1);
            }
            let ir = result.ir.expect("ok implies ir");
            // Prefer generated `.puddly/generated/action-sidecar.mjs`, then
            // `tools/puddly-action-sidecar.mjs`, or `$PUDDLY_ACTION_SIDECAR`.
            // Long-lived worker by default (oneshot via PUDDLY_ACTION_SIDECAR_ONESHOT=1).
            // Migrations are never applied here — run `puddly migrate apply` first.
            let mut rpc = match RpcServer::from_app_with_sidecar(&path, ir) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                }
            };
            if !rpc.runner.is_configured() {
                eprintln!(
                    "warning: action sidecar not configured — run `puddly build`, set {ACTION_SIDECAR_ENV}, or keep tools/puddly-action-sidecar.mjs"
                );
            } else if !rpc.runner.is_oneshot() {
                eprintln!(
                    "{DISPLAY_NAME} serve: long-lived action sidecar (protocol v2); oneshot via PUDDLY_ACTION_SIDECAR_ONESHOT=1"
                );
            }
            rpc.secure_cookies = env.secure_cookies;
            let pool = match PgPool::connect(&PoolConfig::from_url(&env.database_url)).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                }
            };
            let views = match ViewBundle::load(&path) {
                Ok(v) => Arc::new(v),
                Err(e) => {
                    eprintln!("error: loading views: {e}");
                    return ExitCode::from(1);
                }
            };
            let config = ServerConfig {
                bind,
                root: path,
                secure_cookies: env.secure_cookies,
                ..ServerConfig::default()
            };
            eprintln!(
                "{DISPLAY_NAME} serve: env={} (migrate apply is explicit; /health /ready /metrics /events)",
                env.deploy
            );
            match serve(&config, Arc::new(Mutex::new(rpc)), pool, views).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Commands::Dev {
            path,
            bind,
            database_url,
            secure_cookies,
        } => {
            let url = database_url
                .or_else(|| std::env::var("DATABASE_URL").ok())
                .filter(|s| !s.is_empty());

            let views = match ViewBundle::load(&path) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: loading views: {e}");
                    return ExitCode::from(1);
                }
            };
            let mut state = DevState::new(path.clone());
            state.mark_reload();

            let (rpc, pool) = if let Some(url) = url {
                let result = check(&path);
                if !result.ok {
                    // Still start in a degraded way? Prefer fail-fast when DB mode requested.
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                let mut rpc = match RpcServer::from_app_with_sidecar(&path, ir) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::from(1);
                    }
                };
                if !rpc.runner.is_configured() {
                    eprintln!(
                        "warning: action sidecar not configured — run `puddly build`, set {ACTION_SIDECAR_ENV}, or keep tools/puddly-action-sidecar.mjs"
                    );
                } else if !rpc.runner.is_oneshot() {
                    eprintln!(
                        "{DISPLAY_NAME} dev: long-lived action sidecar (protocol v2)"
                    );
                }
                rpc.secure_cookies = secure_cookies;
                let pool = match PgPool::connect(&PoolConfig::from_url(&url)).await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::from(1);
                    }
                };
                (Some(rpc), Some(pool))
            } else {
                eprintln!(
                    "{DISPLAY_NAME} dev: no DATABASE_URL — view-only mode (SSR + /__dev; RPC disabled)"
                );
                // Soft-check resources for initial /__dev state (do not fail startup).
                let result = check(&path);
                if !result.ok {
                    state.ok = false;
                    state.errors = errors_from_check(&result);
                    eprintln!("{DISPLAY_NAME} check: initial diagnostics present (see /__dev)");
                }
                (None, None)
            };

            let handles = Arc::new(DevHandles::new(views, state, rpc));
            let config = ServerConfig {
                bind: bind.clone(),
                root: path.clone(),
                secure_cookies,
                ..ServerConfig::default()
            };

            let watch_handles = Arc::clone(&handles);
            let watch_root = path.clone();
            tokio::spawn(async move {
                if let Err(e) = watch_dev_loop(watch_root, watch_handles).await {
                    eprintln!("error: watcher stopped: {e}");
                }
            });

            match serve_dev(&config, handles, pool).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Commands::Build { path } => {
            match package_action_sidecar(&path) {
                Ok(dest) => {
                    println!(
                        "{DISPLAY_NAME} build: wrote {} (template → {GENERATED_SIDECAR_SCRIPT})",
                        dest.display()
                    );
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                }
            }
            let result = check(&path);
            if !result.ok {
                print_out(&format_check(&result, false));
                return ExitCode::from(1);
            }
            let ir = result.ir.expect("ok implies ir");
            match write_ts_client(&path, &ir) {
                Ok(dest) => {
                    println!(
                        "{DISPLAY_NAME} build: wrote {} ({GENERATED_CLIENT_TS})",
                        dest.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: write client: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Commands::Client { command } => match command {
            ClientCmd::Generate { path } => {
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                match write_ts_client(&path, &ir) {
                    Ok(dest) => {
                        println!(
                            "{DISPLAY_NAME} client: wrote {} ({GENERATED_CLIENT_TS})",
                            dest.display()
                        );
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                }
            }
        },
        Commands::Init { path, docker } => {
            match init_app_with_options(&path, &InitOptions { docker }) {
                Ok(init) => {
                    for p in &init.written {
                        println!("  wrote {}", p.display());
                    }
                    let result = check(&path);
                    if !result.ok {
                        eprintln!(
                            "error: scaffolded app failed check:\n{}",
                            format_check(&result, false)
                        );
                        return ExitCode::from(1);
                    }
                    let ir = result.ir.expect("ok implies ir");
                    match write_manifest(&path, &build_manifest(&path, &ir)) {
                        Ok(manifest_path) => {
                            println!("  wrote {}", manifest_path.display());
                            println!(
                                "{DISPLAY_NAME} init: ok — {} resource(s) at {}",
                                ir.resources.len(),
                                path.display()
                            );
                            if docker {
                                println!(
                                    "{DISPLAY_NAME} init: docker files written — copy a linux `puddly` binary next to Dockerfile before compose up"
                                );
                            }
                            println!(
                                "See CONVENTIONS.md in the app (and the framework repo root)."
                            );
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("error: write manifest: {e}");
                            ExitCode::from(1)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
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
