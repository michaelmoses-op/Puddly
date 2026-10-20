# Puddly

Server-first web framework. CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 2** — deterministic `.puddly/manifest.json`, plus `inspect` / `impact`
query library APIs (CLI thin wrappers). Step 1 parser/validator remains in place.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- manifest generate examples/todo-slice
cargo run -p puddly-cli -- manifest check examples/todo-slice
cargo run -p puddly-cli -- inspect todo --path examples/todo-slice
cargo run -p puddly-cli -- impact user --path examples/todo-slice --json
cargo run -p puddly-cli -- explain E0204
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
  puddly-cli/          # `puddly` binary
examples/todo-slice/   # sample app + committed .puddly/manifest.json
tests/fixtures/        # invalid fixtures per error code
```

CI sketch: [`.github/workflows/puddly-check.yml`](.github/workflows/puddly-check.yml)
runs `check` + `manifest check` so stale committed manifests fail the build.

Design docs live under `docs/` locally (gitignored).
