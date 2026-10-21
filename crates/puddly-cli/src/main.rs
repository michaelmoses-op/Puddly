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
use puddly_rpc::{
    write_openapi, write_rs_rows, write_ts_client, RpcServer, GENERATED_CLIENT_TS,
    GENERATED_OPENAPI_JSON, GENERATED_ROWS_RS,
};
use puddly_server::{
    errors_from_check, resolve_serve_env, serve, serve_dev, watch_dev_loop, DeployEnv,
    DevHandles, DevState, LiveBridge, ServerConfig,
};
use puddly_views::ViewBundle;
use std::path::{Path, PathBuf};
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
    /// Apply SQL migrations (default), or `generate` / `check` / `apply`
    #[command(args_conflicts_with_subcommands = true)]
    Migrate {
        #[command(subcommand)]
        command: Option<MigrateCmd>,
        /// App root for bare `puddly migrate` (apply). Defaults to cwd app.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Postgres URL (defaults to $DATABASE_URL)
        #[arg(long)]
        database_url: Option<String>,
        /// List pending migrations without executing SQL or writing the ledger
        #[arg(long)]
        dry_run: bool,
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
        /// Opt-in REST `/api/*` + `GET /openapi.json` beside RPC (ADR-045)
        #[arg(long)]
        rest: bool,
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
    /// Package sidecar + typed TS client + Rust rows into `.puddly/generated/` (ADR-031 / 038 / 042)
    Build {
        /// App root (defaults to current directory / nearest app)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Doctor checks (Node 22+) then `puddly build` for the app
    Install {
        /// App root (defaults to current directory / nearest app)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// One-shot demo: migrate apply → build → dev (or `--serve`)
    Up {
        /// App root (defaults to current directory / nearest app)
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
        /// Use `serve` instead of `dev` after build
        #[arg(long)]
        serve: bool,
        /// Opt-in REST when using `--serve` (ADR-045)
        #[arg(long)]
        rest: bool,
    },
    /// Typed TypeScript RPC client + Rust row structs (ADR-038 / ADR-042)
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
    /// Write `.puddly/generated/client.ts` + `rows.rs` from the app IR
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
            let path = resolve_app_path(&path);
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
                let path = resolve_app_path(&path);
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
                let path = resolve_app_path(&path);
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
            let path = resolve_app_path(&path);
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
            let path = resolve_app_path(&path);
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
        Commands::Migrate {
            command,
            path,
            database_url,
            dry_run,
        } => match command {
            None => {
                let path = resolve_app_path(&path);
                cmd_migrate_apply(&path, database_url, dry_run).await
            }
            Some(MigrateCmd::Generate {
                path,
                allow_destructive,
            }) => {
                let path = resolve_app_path(&path);
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
            Some(MigrateCmd::Check { path, json }) => {
                let path = resolve_app_path(&path);
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
                    print_out(&format!("{}
", serde_json::to_string_pretty(&body).unwrap()));
                } else {
                    println!("{}", drift_message(&status));
                }
                exit_bool(matches!(status, DriftStatus::Fresh))
            }
            Some(MigrateCmd::Apply {
                path,
                database_url,
                dry_run,
            }) => {
                let path = resolve_app_path(&path);
                cmd_migrate_apply(&path, database_url, dry_run).await
            }
        },
        Commands::Auth { command } => match command {
            AuthCmd::Scaffold { path } => {
                let path = resolve_app_path(&path);
                match scaffold_auth_migration(&path) {
                Ok(r) => {
                    println!("{DISPLAY_NAME} auth: {}", r.message);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
            }
        },
        Commands::Serve {
            path,
            bind,
            database_url,
            secure_cookies,
            rest,
        } => {
            let path = resolve_app_path(&path);
            let rest = rest
                || std::env::var("PUDDLY_REST")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
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
                database_url: Some(env.database_url.clone()),
                live_fanout: true,
                rest,
                ..ServerConfig::default()
            };
            eprintln!(
                "{DISPLAY_NAME} serve: env={} (migrate apply is explicit; /health /ready /metrics /events; LISTEN puddly_live{})",
                env.deploy,
                if rest { "; REST /api + /openapi.json" } else { "" }
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
            let path = resolve_app_path(&path);
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

            let live = LiveBridge::shared(pool.is_some());
            let handles = Arc::new(DevHandles::new(views, state, rpc).with_live(Arc::clone(&live)));
            let config = ServerConfig {
                bind: bind.clone(),
                root: path.clone(),
                secure_cookies,
                database_url: pool.as_ref().map(|p| p.connection_string().to_string()),
                live_fanout: pool.is_some(),
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
            let path = resolve_app_path(&path);
            cmd_build(&path)
        }
        Commands::Install { path } => {
            let path = resolve_app_path(&path);
            if let Err(code) = doctor_checks() {
                return code;
            }
            println!("{DISPLAY_NAME} install: packaging app artifacts…");
            cmd_build(&path)
        }
        Commands::Up {
            path,
            bind,
            database_url,
            secure_cookies,
            serve: serve_mode,
            rest,
        } => {
            let path = resolve_app_path(&path);
            println!("{DISPLAY_NAME} up: migrate apply…");
            let code = cmd_migrate_apply(&path, database_url.clone(), false).await;
            if code != ExitCode::SUCCESS {
                return code;
            }
            println!("{DISPLAY_NAME} up: build…");
            let code = cmd_build(&path);
            if code != ExitCode::SUCCESS {
                return code;
            }
            if serve_mode {
                println!("{DISPLAY_NAME} up: serve…");
                let rest = rest
                    || std::env::var("PUDDLY_REST")
                        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                        .unwrap_or(false);
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
                    database_url: Some(env.database_url.clone()),
                    live_fanout: true,
                    rest,
                    ..ServerConfig::default()
                };
                eprintln!(
                    "{DISPLAY_NAME} serve: env={} (migrate apply is explicit; /health /ready /metrics /events; LISTEN puddly_live{})",
                    env.deploy,
                    if rest { "; REST /api + /openapi.json" } else { "" }
                );
                return match serve(&config, Arc::new(Mutex::new(rpc)), pool, views).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("error: {e}");
                        ExitCode::from(1)
                    }
                };
            }
            println!("{DISPLAY_NAME} up: dev…");
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
                    eprintln!("{DISPLAY_NAME} dev: long-lived action sidecar (protocol v2)");
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
                let result = check(&path);
                if !result.ok {
                    state.ok = false;
                    state.errors = errors_from_check(&result);
                    eprintln!("{DISPLAY_NAME} check: initial diagnostics present (see /__dev)");
                }
                (None, None)
            };
            let live = LiveBridge::shared(pool.is_some());
            let handles = Arc::new(DevHandles::new(views, state, rpc).with_live(Arc::clone(&live)));
            let config = ServerConfig {
                bind: bind.clone(),
                root: path.clone(),
                secure_cookies,
                database_url: pool.as_ref().map(|p| p.connection_string().to_string()),
                live_fanout: pool.is_some(),
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
        Commands::Client { command } => match command {
            ClientCmd::Generate { path } => {
                let path = resolve_app_path(&path);
                let result = check(&path);
                if !result.ok {
                    print_out(&format_check(&result, false));
                    return ExitCode::from(1);
                }
                let ir = result.ir.expect("ok implies ir");
                if let Err(e) = write_ts_client(&path, &ir) {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                } else {
                    println!(
                        "{DISPLAY_NAME} client: wrote {} ({GENERATED_CLIENT_TS})",
                        path.join(GENERATED_CLIENT_TS).display()
                    );
                }
                match write_rs_rows(&path, &ir) {
                    Ok(dest) => {
                        println!(
                            "{DISPLAY_NAME} client: wrote {} ({GENERATED_ROWS_RS})",
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


/// True when `path` looks like a Puddly app root.
fn looks_like_app_root(path: &Path) -> bool {
    path.join("puddly.toml").is_file()
        || path.join("features").is_dir()
        || path.join(".puddly").is_dir()
}

/// Resolve an app path: explicit paths win; `.` walks up from cwd for app markers.
fn resolve_app_path(path: &Path) -> PathBuf {
    let explicit = path != Path::new(".");
    if explicit {
        return path.to_path_buf();
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut cur = cwd.clone();
    loop {
        if looks_like_app_root(&cur) {
            return cur;
        }
        if !cur.pop() {
            break;
        }
    }
    cwd
}

/// Doctor-ish preflight for `puddly install` (Node 22+).
fn doctor_checks() -> Result<(), ExitCode> {
    match std::process::Command::new("node").arg("-v").output() {
        Ok(out) if out.status.success() => {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let major = v
                .trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if major < 22 {
                eprintln!(
                    "error: Node.js 22+ required for action sidecar (found {v}). Install Node 22+ and retry."
                );
                return Err(ExitCode::from(1));
            }
            println!("{DISPLAY_NAME} install: Node {v} ok");
            Ok(())
        }
        Ok(_) | Err(_) => {
            eprintln!(
                "error: `node` not found on PATH — Node.js 22+ is required for custom actions."
            );
            Err(ExitCode::from(1))
        }
    }
}

/// Package sidecar + typed client + rows + OpenAPI (same as `puddly build`).
fn cmd_build(path: &Path) -> ExitCode {
    match package_action_sidecar(path) {
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
    let result = check(path);
    if !result.ok {
        print_out(&format_check(&result, false));
        return ExitCode::from(1);
    }
    let ir = result.ir.expect("ok implies ir");
    match write_ts_client(path, &ir) {
        Ok(dest) => {
            println!(
                "{DISPLAY_NAME} build: wrote {} ({GENERATED_CLIENT_TS})",
                dest.display()
            );
        }
        Err(e) => {
            eprintln!("error: write client: {e}");
            return ExitCode::from(1);
        }
    }
    if let Err(e) = write_rs_rows(path, &ir) {
        eprintln!("error: write rows: {e}");
        return ExitCode::from(1);
    }
    println!(
        "{DISPLAY_NAME} build: wrote {} ({GENERATED_ROWS_RS})",
        path.join(GENERATED_ROWS_RS).display()
    );
    match write_openapi(path, &ir) {
        Ok(dest) => {
            println!(
                "{DISPLAY_NAME} build: wrote {} ({GENERATED_OPENAPI_JSON})",
                dest.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: write openapi: {e}");
            ExitCode::from(1)
        }
    }
}

async fn cmd_migrate_apply(
    path: &Path,
    database_url: Option<String>,
    dry_run: bool,
) -> ExitCode {
    let url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .filter(|s| !s.is_empty());
    let Some(url) = url else {
        eprintln!("error: migrate apply requires --database-url or $DATABASE_URL");
        return ExitCode::from(1);
    };
    let result = check(path);
    if !result.ok {
        print_out(&format_check(&result, false));
        return ExitCode::from(1);
    }
    let ir = result.ir.expect("ok implies ir");
    let status = drift_status(path, &ir);
    if !matches!(status, DriftStatus::Fresh) {
        eprintln!("{}", drift_message(&status));
        eprintln!("error: refuse to apply while migrations are missing or drifted");
        return ExitCode::from(1);
    }
    match PostgresExecutor::connect(&url).await {
        Ok(mut exec) => {
            let outcome = if dry_run {
                apply_migrations_dry_run(path, &mut exec).await
            } else {
                apply_migrations(path, &mut exec).await
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
