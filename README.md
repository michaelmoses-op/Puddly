# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 7** — minimal sync HTTP server (`puddly serve`) over the RPC library,
owner/member list SQL predicates, reserved `auth` name (E0211), optional register
`userId`, and `rule.member` placeholder docs (`puddly explain E0212`).

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- explain E0212
# HTTP RPC (needs Postgres):
# DATABASE_URL=postgres://... cargo run -p puddly-cli -- serve examples/todo-slice --bind 127.0.0.1:3000
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
  puddly-rpc/          # RPC dispatch (CSRF, auth context, CRUD, auth ops)
  puddly-server/       # sync HTTP adapter (ADR-024)
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + manifest + migrations
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
