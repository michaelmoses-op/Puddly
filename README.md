# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 31** — IPC / action errors → HTTP status (ADR-049). Prior: Step 30
CSRF double-submit (ADR-048); Step 29 `rule.member` (ADR-047).

## For app authors

Needs **Node 22+**. Rust is only required if you use the Cargo fallback (no Release binary yet).

```bash
# pick one:
npm i -g puddly
# OR: curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash
# OR: npx puddly …

puddly init
# DATABASE_URL=postgres://USER:PASS@127.0.0.1:5432/DB
puddly install && puddly migrate && puddly dev
```

- **`npm i -g puddly`** — thin wrapper ([`packages/puddly`](packages/puddly)); `postinstall` downloads a **prebuilt** GitHub Release binary (does **not** compile Rust). Until the package is on the public registry, use `npm i -g ./packages/puddly` from a checkout, or `npm pack` + install the tarball (see package README).
- **`install.sh`** — prefers the latest Release asset for your OS/arch; falls back to `cargo install --git` only if no matching asset exists.
- **Published targets** (Release workflow): Linux x64, macOS aarch64, macOS x64.

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
| Action `ctx` | `{ user, db }` — `ctx.db.*` reuses host CRUD/access (ADR-046) |
| Access `member` | Join-table membership via `rule.member({ via, on?, user? })` (ADR-047) |
| Env override | `PUDDLY_ACTION_SIDECAR` = space-separated argv |
| Oneshot | `PUDDLY_ACTION_SIDECAR_ONESHOT=1` or `with_oneshot(true)` |
| CLI | `puddly serve` / `puddly dev` (with DB) use long-lived worker when configured |

Requires Node 22+ (`--experimental-strip-types` for `*.action.ts` imports).

## Developing Puddly itself

Framework contributors work in this Cargo workspace. Prefer the installed `puddly` binary for app commands; use Cargo when iterating on the CLI/crates:

```bash
git clone https://github.com/michaelmoses-op/Puddly.git
cd Puddly
./install.sh                    # or: cargo install --path crates/puddly-cli --locked --force
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- explain E0212
cargo run -p puddly-cli -- init ./my-app
cargo test --workspace
```

Re-install after CLI changes: `./install.sh` or `cargo install --path crates/puddly-cli --locked --force`.

Force Cargo even when Releases exist: `PUDDLY_FORCE_CARGO=1 ./install.sh`.

### Releases & npm

- Tag `v*` (e.g. `v0.1.0`) → [`.github/workflows/release.yml`](.github/workflows/release.yml) builds and uploads tarballs.
- npm package lives in [`packages/puddly`](packages/puddly). Publishing to the public registry needs an owner `NPM_TOKEN` (see that package’s README / PR notes).

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
packages/puddly/       # npm thin wrapper (downloads Release binary)
examples/todo-slice/   # sample app + manifest + migrations + todo view/island
tools/                 # action sidecar + puddly-islands.js (hydrate + RPC mutations)
tests/fixtures/        # invalid fixtures per error code
install.sh             # Release binary preferred; Cargo fallback
```

CI: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml) · Releases: [`.github/workflows/release.yml`](.github/workflows/release.yml).

Design docs live under `docs/` locally (gitignored).
