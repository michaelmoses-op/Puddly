# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 4** — runtime data access (compiled prepared statements + restricted
query builder) behind a Postgres `Executor` interface, plus `migrate apply` sketch.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- migrate check examples/todo-slice
# Apply requires a live Postgres:
# DATABASE_URL=postgres://... cargo run -p puddly-cli -- migrate apply examples/todo-slice
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
  puddly-migrate/      # SQL generate / drift check / apply sketch
  puddly-db/           # Executor trait + Postgres + recording drivers
  puddly-data/         # compiled statements + closed-vocabulary builder
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + manifest + migrations
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml).

Design docs live under `docs/` locally (gitignored).
