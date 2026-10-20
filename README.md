# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 3** — Postgres schema IR → committed SQL migrations with
`migrate check` drift detection. Steps 1–2 (parser/IR, manifest, inspect/impact) remain.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- manifest generate examples/todo-slice
cargo run -p puddly-cli -- migrate generate examples/todo-slice
cargo run -p puddly-cli -- migrate check examples/todo-slice
cargo run -p puddly-cli -- inspect todo --path examples/todo-slice
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
  puddly-migrate/      # SQL migration generate + drift check
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + .puddly/manifest.json + migrations/
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml)
runs `check`, `manifest check`, and `migrate check`.

Design docs live under `docs/` locally (gitignored).
