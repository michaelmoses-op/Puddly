# puddly (npm)

Thin Node wrapper around the **prebuilt** `puddly` CLI binary.

- **Does not** compile the Rust workspace on `npm install`
- `postinstall` downloads the matching GitHub Release asset into `vendor/puddly`
- `bin` → `node_modules/.bin/puddly`

## Install (app authors)

```bash
# needs: Node 22+
npm i -g puddly
# or: npx puddly …
puddly init
# DATABASE_URL=…
puddly install && puddly migrate && puddly dev
```

Until the package is on the public npm registry, from a clone or release checkout:

```bash
npm i -g ./packages/puddly
# or: npm pack ./packages/puddly && npm i -g ./puddly-0.1.0.tgz
```

Local/CI without a Release yet:

```bash
cargo build -p puddly-cli --release
PUDDLY_BINARY="$(pwd)/target/release/puddly" npm i -g ./packages/puddly
```

## Supported Release targets

| Target | OS |
| --- | --- |
| `x86_64-unknown-linux-gnu` | Linux x64 |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-apple-darwin` | macOS Intel |

See the repo root `install.sh` for a curl one-liner that prefers the same assets.

## Env

| Variable | Meaning |
| --- | --- |
| `PUDDLY_SKIP_DOWNLOAD=1` | Do not fetch; use existing `vendor/puddly` |
| `PUDDLY_BINARY=/path` | Copy that file into `vendor/puddly` |
| `PUDDLY_VERSION` | Override asset version |
| `PUDDLY_REPO` | Override `owner/repo` for Releases |
