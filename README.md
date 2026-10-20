# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 20** — typed TS client (ADR-038): `puddly client generate` /
`puddly build` emit `.puddly/generated/client.ts` (`api.todo.list`, `api.auth.login`,
…). Prior: Step 19 `include` batch (ADR-037). Next = `puddly init` + CONVENTIONS.

## Try the todo slice

Human click-path (Postgres + Node 22+): see **[`examples/todo-slice/README.md`](examples/todo-slice/README.md)** —
migrate apply → `puddly build` → `puddly serve` → open `/register`.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- explain E0212
# Package sidecar + typed client into gitignored .puddly/generated/:
cargo run -p puddly-cli -- build examples/todo-slice
cargo run -p puddly-cli -- client generate examples/todo-slice
# Dev server (/__dev; views with DB load need DATABASE_URL + session cookie):
cargo run -p puddly-cli -- dev examples/todo-slice --bind 127.0.0.1:3000
# Then: GET /login|/register → static auth forms (no DB); GET / without session → 401 + link
# Full RPC + view load in dev (needs Postgres + Node 22+ for custom actions):
# DATABASE_URL=postgres://... cargo run -p puddly-cli -- dev examples/todo-slice
# Production-ish serve (always needs DATABASE_URL); GET / uses load + session cookie:
# DATABASE_URL=postgres://... cargo run -p puddly-cli -- serve examples/todo-slice --bind 127.0.0.1:3000
cargo test --workspace
```

### Action sidecar

Custom actions run in a **long-lived** Node process speaking JSONL v2 on stdin/stdout
(correlated by `id`). Set `PUDDLY_ACTION_SIDECAR_ONESHOT=1` for process-per-call.

| How | Detail |
| --- | --- |
| Package | `puddly build` → `.puddly/generated/action-sidecar.mjs` + `client.ts` |
| Typed client | `puddly client generate` → `api.<resource>.<op>()` (ADR-038) |
| Default discovery | env → generated (walk-up) → `tools/puddly-action-sidecar.mjs` (walk-up) |
| Convenience | `IpcActionRunner::node_sidecar` / `::generated_sidecar` |
| Env override | `PUDDLY_ACTION_SIDECAR` = space-separated argv |
| Oneshot | `PUDDLY_ACTION_SIDECAR_ONESHOT=1` or `with_oneshot(true)` |
| CLI | `puddly serve` / `puddly dev` (with DB) use long-lived worker when configured |

Requires Node 22+ (`--experimental-strip-types` for `*.action.ts` imports).

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
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
