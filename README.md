# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 27** — opt-in REST `/api/*` + OpenAPI (ADR-045); `puddly serve --rest`.
Prior: Step 26 IR row decode in CRUD (ADR-044).

## Install the CLI

One-time (needs [Rust/Cargo](https://rustup.rs)):

```bash
# from a clone
./install.sh
# or: cargo install --path crates/puddly-cli --locked --force

# or without cloning first
curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash
```

That puts `puddly` on PATH (usually `~/.cargo/bin`). App authors use short `puddly …` commands after that — no daily `cargo run`.

## Quick start (app authors)

```bash
cd my-app   # or examples/todo-slice
puddly install   # Node 22+ doctor + build (sidecar, client, rows)
# or: puddly build
export DATABASE_URL=postgres://USER:PASS@127.0.0.1:5432/DB
puddly migrate   # apply (also: migrate generate|check|apply)
puddly dev       # or: puddly serve
# demo one-shot: puddly up
```

When your cwd is an app (`features/`, `.puddly/`, or `puddly.toml`), app-path args are optional — `puddly` walks up from cwd to find the app root.

New app: `puddly init ./my-app` (add `--docker` for Dockerfile + compose; migrate apply stays explicit).

Human click-path for the sample: **[`examples/todo-slice/README.md`](examples/todo-slice/README.md)**.

### Action sidecar

Custom actions run in a **long-lived** Node process speaking JSONL v2 on stdin/stdout
(correlated by `id`). Set `PUDDLY_ACTION_SIDECAR_ONESHOT=1` for process-per-call.

| How | Detail |
| --- | --- |
| Package | `puddly build` / `puddly install` → `.puddly/generated/action-sidecar.mjs` + `client.ts` + rows |
| Typed client | `puddly client generate` → `api.<resource>.<op>()` (ADR-038) |
| Default discovery | env → generated (walk-up) → `tools/puddly-action-sidecar.mjs` (walk-up) |
| Convenience | `IpcActionRunner::node_sidecar` / `::generated_sidecar` |
| Env override | `PUDDLY_ACTION_SIDECAR` = space-separated argv |
| Oneshot | `PUDDLY_ACTION_SIDECAR_ONESHOT=1` or `with_oneshot(true)` |
| CLI | `puddly serve` / `puddly dev` (with DB) use long-lived worker when configured |

Requires Node 22+ (`--experimental-strip-types` for `*.action.ts` imports).

## Developing Puddly itself

Framework contributors work in this Cargo workspace. Prefer the installed `puddly` binary for app commands; use Cargo when iterating on the CLI/crates:

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- explain E0212
cargo run -p puddly-cli -- init ./my-app
cargo test --workspace
```

Re-install after CLI changes: `./install.sh` or `cargo install --path crates/puddly-cli --locked --force`.

## Workspace

```
crates/
  puddly-core/         # identity constants
  puddly-diagnostics/  # codes, rendering, explain registry
  puddly-ir/           # owned IR
  puddly-parse/        # Oxc-backed *.resource.ts parser
  puddly-validate/     # cross-file validation
  puddly-check/        # library entry for `check`
  puddly-manifest/     # deterministic manifest.json
  puddly-query/        # inspect / impact library
  puddly-schema/       # Postgres schema IR
  puddly-migrate/      # SQL generate / drift check / apply (+ dry-run)
  puddly-db/           # async Executor + PgPool (tokio-postgres/deadpool) + Recording
  puddly-data/         # compiled statements + closed-vocabulary builder
  puddly-auth/         # email/password, cookie sessions, access rules, auth scaffold
  puddly-actions/      # load *.action.ts + Node IPC v2 sidecar (long-lived / oneshot)
  puddly-rpc/          # async RPC dispatch (CSRF, auth, CRUD, actions)
  puddly-server/       # async HTTP + SSR GETs + puddly dev (watcher, /__dev, overlay)
  puddly-views/        # *.view.tsx / *.island.tsx discover + Oxc compile + SSR
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + manifest + migrations + todo view/island
tools/                 # action sidecar + puddly-islands.js (hydrate + RPC mutations)
tests/fixtures/        # invalid fixtures per error code
install.sh             # cargo-install the puddly CLI onto PATH
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
