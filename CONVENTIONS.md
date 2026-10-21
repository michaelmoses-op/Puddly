# Puddly conventions

Short rules for apps (and for AI). Details live in gitignored `docs/` ADRs.

## Layout

- App root has `features/`, optional `migrations/`, and committed `.puddly/manifest.json`.
- Generated artifacts: `.puddly/generated/` (sidecar, typed TS client, Rust `rows.rs`) — **gitignored**.
- One feature area per folder under `features/<area>/`.

## File naming

| Kind | Pattern |
| --- | --- |
| Resource | `*.resource.ts` |
| View | `*.view.tsx` |
| Island | `*.island.tsx` |
| Action | `*.action.ts` (path referenced from the resource) |

## Resources

- Declare with `resource("name", { fields, access, … })`.
- Table name = resource name. PK = `field.id()` (UUID).
- Refs: `field.ref("otherResource")`. Use `include` on list/load to batch-fetch (no JOIN).
- Access: one rule per op (`authenticated` / `owner(field)` / …).

## Views & load

- `export const load = { key: { list: "resource" } }` or `get` + `idFromQuery`, or `value`.
- List load may set `include: ["refField"]` (or a string).
- Optional realtime: `export const live = true` (or `live: true` in `load`) → SSR
  `data-puddly-live="1"` + `EventSource('/events')` (SSE; multi-instance via Postgres
  `LISTEN/NOTIFY` on `puddly_live` — ADR-040 / ADR-043).
- Default export is the page. Collections: one-level `.map((item) => …)`.
- Islands are separate modules; SSR emits `data-puddly-island` + props.
- Toggle without full reload on live pages: `data-puddly-reload="0"`.

## Actions & RPC

- Custom actions: `actions: { name: { input, run: "./file.action.ts", access? } }`.
- RPC: `POST /rpc/<resource>.<op>` JSON body. Mutations need `X-Puddly-CSRF` + session cookie.
- Islands: `data-puddly-rpc` / forms, or generated `api.<resource>.<op>()` client.

## Auth

- Built-in `auth.register` / `auth.login` / `auth.logout`.
- Opt-in DDL: `puddly auth scaffold` then migrate apply.
- Session cookie: `puddly_session` (Lax; Secure in prod).

## Migrate & check

- `puddly check` — parse/validate resources.
- `puddly migrate` — apply pending SQL (default). Also `migrate generate` / `check` / `apply`.
- Never auto-migrate on startup; `puddly dev` only hints.

## Dev & build

- Install CLI once: `./install.sh` or `cargo install --path crates/puddly-cli` (see repo README).
- App cwd defaults: when cwd (or a parent) has `features/`, `.puddly/`, or `puddly.toml`, omit app-path args.
- `puddly install` — Node 22+ doctor, then same packaging as `build`.
- `puddly build` — package sidecar + typed TS client + Rust rows into `.puddly/generated/`.
- `puddly up` — migrate apply → build → `dev` (or `--serve`).
- `puddly dev` — watch, SSR, `/__dev`; needs `DATABASE_URL` for DB loads.
- `puddly client generate` — TS client + Rust `rows.rs` (DbRow maps remain at the executor).
- `puddly init [path]` — scaffold a minimal app (this file + sample resource).
- `puddly init --docker` — also write Dockerfile + compose (migrate apply is a one-shot, never on serve start).
- Production serve: `GET /health` (liveness), `GET /ready` (pool), `GET /metrics` (JSON counters); SIGTERM drains SSE.
