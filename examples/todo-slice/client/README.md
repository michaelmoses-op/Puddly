# Typed client (generated)

Run from the app root (or any path that checks):

```bash
puddly client generate .
# or as part of:
puddly build .
```

That writes **gitignored** `.puddly/generated/client.ts` with `api.todo.list`,
`api.auth.login`, etc. (fetch + CSRF + credentials).

Committed snapshot for reading without generating:

- [`client.ts`](./client.ts) — same shape as the generator output for this example.
