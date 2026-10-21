//! `puddly init` — scaffold a minimal app (ADR-039 / Step 21).
//! Optional `--docker` adds run-anywhere files (ADR-041 / Step 23).

use puddly_core::{CLI_NAME, DISPLAY_NAME, MANIFEST_DIR};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Options for [`init_app_with_options`].
#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    /// Write `Dockerfile`, `docker-compose.yml`, and `.dockerignore`.
    pub docker: bool,
}

/// Files written by a successful [`init_app`].
#[derive(Debug, Clone)]
pub struct InitResult {
    pub root: PathBuf,
    pub written: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum InitError {
    Io(io::Error),
    Message(String),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for InitError {}

impl From<io::Error> for InitError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Scaffold a minimal Puddly app at `root` (no Docker files).
pub fn init_app(root: impl AsRef<Path>) -> Result<InitResult, InitError> {
    init_app_with_options(root, &InitOptions::default())
}

/// Scaffold a minimal Puddly app at `root`.
///
/// Creates `features/`, a sample `note` resource, `.puddly/`, README, and a copy of
/// framework conventions. With [`InitOptions::docker`], also writes container files.
/// Refuses if `features/` already exists.
pub fn init_app_with_options(
    root: impl AsRef<Path>,
    opts: &InitOptions,
) -> Result<InitResult, InitError> {
    let root = root.as_ref().to_path_buf();
    if root.exists() {
        if !root.is_dir() {
            return Err(InitError::Message(format!(
                "{} exists and is not a directory",
                root.display()
            )));
        }
        let features = root.join("features");
        if features.exists() {
            return Err(InitError::Message(format!(
                "refusing to init: {} already exists",
                features.display()
            )));
        }
    } else {
        fs::create_dir_all(&root)?;
    }

    let mut written = Vec::new();

    let note_dir = root.join("features/notes");
    fs::create_dir_all(&note_dir)?;
    let note_path = note_dir.join("note.resource.ts");
    fs::write(&note_path, SAMPLE_NOTE_RESOURCE)?;
    written.push(note_path);

    let puddly = root.join(MANIFEST_DIR);
    fs::create_dir_all(&puddly)?;
    let gitkeep = puddly.join(".gitkeep");
    fs::write(&gitkeep, "")?;
    written.push(gitkeep);

    let conventions = root.join("CONVENTIONS.md");
    fs::write(&conventions, include_str!("../../../CONVENTIONS.md"))?;
    written.push(conventions);

    let readme = root.join("README.md");
    fs::write(&readme, sample_readme(opts.docker))?;
    written.push(readme);

    if opts.docker {
        let dockerfile = root.join("Dockerfile");
        fs::write(&dockerfile, DOCKERFILE)?;
        written.push(dockerfile);

        let compose = root.join("docker-compose.yml");
        fs::write(&compose, DOCKER_COMPOSE)?;
        written.push(compose);

        let dockerignore = root.join(".dockerignore");
        fs::write(&dockerignore, DOCKERIGNORE)?;
        written.push(dockerignore);
    }

    Ok(InitResult { root, written })
}

fn sample_readme(docker: bool) -> String {
    let docker_section = if docker {
        format!(
            r#"
## Docker

Requires a `puddly` binary on the image build context (copy from a release build of
the {DISPLAY_NAME} CLI). Migrations are **explicit** — compose runs
`{CLI_NAME} migrate apply` as a one-shot before `serve` (never on process start).

```bash
# Place a linux `puddly` binary next to the Dockerfile, then:
docker compose up --build
```

Endpoints: `GET /health`, `GET /ready`, `GET /metrics`, `GET /events`.
"#
        )
    } else {
        String::new()
    };

    format!(
        r#"# Puddly app

Scaffolded by `{CLI_NAME} init`.

Follow **[CONVENTIONS.md](./CONVENTIONS.md)** for file naming, resources, views, actions,
RPC, auth, migrate, load, and islands ({DISPLAY_NAME}).

```bash
{CLI_NAME} check .
{CLI_NAME} manifest generate .
{CLI_NAME} migrate generate .
# After DATABASE_URL + auth scaffold as needed:
# {CLI_NAME} migrate apply .
# {CLI_NAME} build .
# {CLI_NAME} serve . --bind 127.0.0.1:3000
```

Sample resource: `features/notes/note.resource.ts`. Add views under `features/*/`.
{docker_section}"#
    )
}

const SAMPLE_NOTE_RESOURCE: &str = r#"export const Note = resource("note", {
  fields: {
    id: field.id(),
    title: field.text({ max: 200 }),
    done: field.bool({ default: false, filterable: true }),
    createdAt: field.timestamp({ auto: true, sortable: true }),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
"#;

const DOCKERFILE: &str = r#"# Puddly app image (ADR-041).
# Build context must include a linux `puddly` CLI binary next to this file.
# Migrations are NOT run on startup — use compose `migrate` service or:
#   docker compose run --rm migrate
FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY puddly /usr/local/bin/puddly
RUN chmod +x /usr/local/bin/puddly

WORKDIR /app
COPY . /app

ENV PUDDLY_ENV=production
EXPOSE 3000

# Explicit migrate only — never auto-apply here.
CMD ["puddly", "serve", ".", "--bind", "0.0.0.0:3000"]
"#;

const DOCKER_COMPOSE: &str = r#"# Run-anywhere stack: Postgres + explicit migrate + serve (ADR-041).
# Place a linux `puddly` binary in this directory before `docker compose up --build`.
services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: puddly
      POSTGRES_PASSWORD: puddly
      POSTGRES_DB: puddly
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U puddly -d puddly"]
      interval: 2s
      timeout: 5s
      retries: 15

  migrate:
    build: .
    environment:
      DATABASE_URL: postgres://puddly:puddly@db:5432/puddly
      PUDDLY_ENV: production
    command: ["puddly", "migrate", "apply", "."]
    depends_on:
      db:
        condition: service_healthy
    restart: "no"

  app:
    build: .
    environment:
      DATABASE_URL: postgres://puddly:puddly@db:5432/puddly
      PUDDLY_ENV: production
    ports:
      - "3000:3000"
    depends_on:
      migrate:
        condition: service_completed_successfully
"#;

const DOCKERIGNORE: &str = r#"target/
.git/
**/.puddly/cache/
**/.puddly/generated/
**/node_modules/
docs/
"#;
