# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 9** — in-repo Node action sidecar (`tools/puddly-action-sidecar.mjs`)
executes `*.action.ts` over the frozen JSONL IPC protocol; RPC wires
`RpcContext.user` → sidecar `ctx.user` (ADR-027).

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- explain E0212
# HTTP RPC (needs Postgres + Node 22+ for custom actions):
# DATABASE_URL=postgres://... cargo run -p puddly-cli -- serve examples/todo-slice --bind 127.0.0.1:3000
# Optional sidecar override (space-separated argv):
# PUDDLY_ACTION_SIDECAR="node --experimental-strip-types $(pwd)/tools/puddly-action-sidecar.mjs"
cargo test --workspace
```

### Action sidecar

Custom actions run in a short-lived Node process speaking JSONL on stdin/stdout.

| How | Detail |
| --- | --- |
| Default | `IpcActionRunner::from_env_or_workspace(app_root)` walks up for `tools/puddly-action-sidecar.mjs` |
| Convenience | `IpcActionRunner::node_sidecar(workspace_root)` |
| Env override | `PUDDLY_ACTION_SIDECAR` = space-separated argv |
| Legacy | `IpcActionRunner::command(program)` / `::argv([...])` |
| CLI | `puddly serve` uses `RpcServer::from_app_with_sidecar` |

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
  puddly-db/           # Executor trait + Postgres + recording drivers
  puddly-data/         # compiled statements + closed-vocabulary builder
  puddly-auth/         # email/password, cookie sessions, access rules, auth scaffold
  puddly-actions/      # load *.action.ts + Node IPC sidecar runner
  tools/               # puddly-action-sidecar.mjs (JSONL action host)
  puddly-rpc/          # RPC dispatch (CSRF, auth, CRUD, actions)
  puddly-server/       # sync HTTP adapter (ADR-024)
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + manifest + migrations
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
