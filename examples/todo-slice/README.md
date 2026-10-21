# Todo slice (Puddly example)

Clickable vertical slice: **sign up → see list → create todo → toggle/archive → reload and data persists**.

## Prerequisites

- **`puddly` on PATH** — from the repo: `./install.sh` or `cargo install --path crates/puddly-cli --locked --force`
- **Node.js 22+** (custom actions use `--experimental-strip-types`)
- **Postgres** with a database you can write to
- **`DATABASE_URL`** — e.g. `postgres://puddly:puddly@127.0.0.1:5432/puddly`

Auth account/session tables are opt-in: write the scaffold SQL once, then apply it
(`puddly auth scaffold` → review → `puddly migrate`).

## Runbook

From **this directory** (`examples/todo-slice`):

```bash
export DATABASE_URL=postgres://USER:PASS@127.0.0.1:5432/DB

puddly install   # or: puddly build
puddly migrate   # apply pending SQL (also: migrate generate|check|apply)
puddly dev       # or: puddly serve

# demo one-shot (migrate + build + dev):
# puddly up
```

App-path args are optional when cwd (or a parent) is the app (`features/`, `.puddly/`).

Auth DDL once per database:

```bash
puddly auth scaffold
puddly migrate
```

Open the app:

1. Visit [http://127.0.0.1:3000/register](http://127.0.0.1:3000/register) — create an account.
2. You land on `/` (Todos). Nav: **Todos / Login / Register / Logout**.
3. Type a title → **Add**. First create ensures a personal `project` row for you (no hidden UUID).
4. **Toggle done** / **Archive** on a row. Archive deletes the row so it leaves the list.
   Toggle uses `data-puddly-reload="0"` so the same tab patches via RPC; other tabs get
   SSE (`GET /events`, `live: true` on the view).
5. Open a second tab of `/` — toggle in one; the other updates without a manual refresh.
6. Reload — remaining todos still load from Postgres. Empty list shows *No todos yet — add one above.*

CSRF (`X-Puddly-CSRF`) and session cookies are sent automatically by `tools/puddly-islands.js` (`credentials: "include"`). You do not set them by hand in the browser UI. Live pages auto-open `EventSource('/events')`.

## Layout

| Path | Role |
| --- | --- |
| `features/auth/*.view.tsx` | Login / register (static `load`) |
| `features/todos/todo.view.tsx` | Session-gated list + create form |
| `features/todos/todo-row.island.tsx` | Toggle + Archive |
| `features/todos/archive.action.ts` | Sidecar action; RPC then `DELETE`s the row |
| `migrations/0001_init.sql` | `user` / `project` / `todo` tables |

## Developing Puddly itself

Framework contributors iterating in-tree can still invoke the CLI via Cargo from the **repo root**:

```bash
cargo run -p puddly-cli -- check examples/todo-slice
cargo run -p puddly-cli -- migrate apply examples/todo-slice
cargo run -p puddly-cli -- build examples/todo-slice
cargo run -p puddly-cli -- serve examples/todo-slice --bind 127.0.0.1:3000
```

## Tests

RecordingExecutor (no live DB):

```bash
cargo test -p puddly-rpc --test step18_vertical_slice
cargo test --workspace
```
