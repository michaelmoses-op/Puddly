# Mochi

Server-first web framework (working name). CLI: `mochi`.

Display name and CLI binary are defined in one place:
[`crates/mochi-core/src/identity.rs`](crates/mochi-core/src/identity.rs).

## Status

**Build plan Step 1** — parse `*.resource.ts` → validate → IR, with `mochi check [--json]`
and `mochi explain <code>`.

## Quick start

```bash
cargo run -p mochi-cli -- check examples/todo-slice
cargo run -p mochi-cli -- check examples/todo-slice --json
cargo run -p mochi-cli -- explain E0204
cargo test -p mochi-check
```

## Workspace

```
crates/
  mochi-core/         # identity constants
  mochi-diagnostics/  # codes, rendering, explain registry
  mochi-ir/           # owned IR
  mochi-parse/        # Oxc-backed *.resource.ts parser
  mochi-validate/     # cross-file validation
  mochi-check/        # library entry for `check`
  mochi-cli/          # `mochi` binary
examples/todo-slice/  # user / project / todo resources
tests/fixtures/       # invalid fixtures per error code
```

Design docs live under `docs/` locally (gitignored; see `.gitignore`). Materialize or copy
them from the project kickoff brief if missing.
