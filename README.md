# Puddly

Server-first web framework (working name). CLI: `puddly`.

Display name and CLI binary are defined in one place:
[`crates/puddly-core/src/identity.rs`](crates/puddly-core/src/identity.rs).

## Status

**Build plan Step 1** — parse `*.resource.ts` → validate → IR, with `puddly check [--json]`
and `puddly explain <code>`.

## Quick start

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- check examples/todo-slice --json
cargo run -p puddly-cli -- explain E0204
cargo test -p puddly-check
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
  puddly-cli/          # `puddly` binary
examples/todo-slice/  # user / project / todo resources
tests/fixtures/       # invalid fixtures per error code
```

Design docs live under `docs/` locally (gitignored; see `.gitignore`). Materialize or copy
them from the project kickoff brief if missing.
