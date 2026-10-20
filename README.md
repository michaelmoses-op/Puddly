# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 6** — action-file loading + RPC skeleton (`POST /rpc/<resource>.<operation>`),
auth resolver in request context, access-rule enforcement on CRUD, CSRF custom-header on
mutations, thin `auth.login` / `auth.register` / `auth.logout`, opt-in `puddly auth scaffold`.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- migrate check examples/todo-slice
cargo run -p puddly-cli -- auth scaffold /tmp/puddly-app   # opt-in auth DDL migration
cargo test --workspace
```

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
  puddly-actions/      # load / index declared *.action.ts files
  puddly-rpc/          # RPC dispatch skeleton (CSRF, auth context, CRUD, auth ops)
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + manifest + migrations
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
