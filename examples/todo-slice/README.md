# Todo slice (Puddly example)

Clickable vertical slice: **sign up → see list → create todo → toggle/archive → reload and data persists**.

## Prerequisites

- **Rust** (stable toolchain; workspace builds with Cargo)
- **Node.js 22+** (custom actions use `--experimental-strip-types`)
- **Postgres** with a database you can write to
- **`DATABASE_URL`** — e.g. `postgres://puddly:puddly@127.0.0.1:5432/puddly`

Auth account/session tables are opt-in: write the scaffold SQL once, then apply it
(`puddly auth scaffold` → review → apply with your usual migrate/SQL path).

## Runbook

From the **repo root**:

```bash
# 1) Apply example migrations (needs DATABASE_URL)
export DATABASE_URL=postgres://USER:PASS@127.0.0.1:5432/DB
cargo run -p puddly-cli -- migrate apply examples/todo-slice

# Auth DDL once per database (writes a migration file under the app):
cargo run -p puddly-cli -- auth scaffold examples/todo-slice
# Then apply that new migration the same way (migrate apply / psql).

# 2) Package action sidecar + typed client into .puddly/generated/
cargo run -p puddly-cli -- build examples/todo-slice
# Or client only: cargo run -p puddly-cli -- client generate examples/todo-slice
# See client/ for a committed snapshot of the generated API shape.

# 3) Serve (or use `dev` for hot reload)
cargo run -p puddly-cli -- serve examples/todo-slice --bind 127.0.0.1:3000
# Dev alternative:
# cargo run -p puddly-cli -- dev examples/todo-slice --bind 127.0.0.1:3000
```

Open the app:

1. Visit [http://127.0.0.1:3000/register](http://127.0.0.1:3000/register) — create an account.
2. You land on `/` (Todos). Nav: **Todos / Login / Register / Logout**.
3. Type a title → **Add**. First create ensures a personal `project` row for you (no hidden UUID).
4. **Toggle done** / **Archive** on a row. Archive deletes the row so it leaves the list.
5. Reload — remaining todos still load from Postgres. Empty list shows *No todos yet — add one above.*

CSRF (`X-Puddly-CSRF`) and session cookies are sent automatically by `tools/puddly-islands.js` (`credentials: "include"`). You do not set them by hand in the browser UI.

## Layout

| Path | Role |
| --- | --- |
| `features/auth/*.view.tsx` | Login / register (static `load`) |
| `features/todos/todo.view.tsx` | Session-gated list + create form |
| `features/todos/todo-row.island.tsx` | Toggle + Archive |
| `features/todos/archive.action.ts` | Sidecar action; RPC then `DELETE`s the row |
| `migrations/0001_init.sql` | `user` / `project` / `todo` tables |

## Tests

RecordingExecutor (no live DB):

```bash
cargo test -p puddly-rpc --test step18_vertical_slice
cargo test --workspace
```
