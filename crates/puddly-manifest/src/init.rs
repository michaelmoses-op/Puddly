//! `puddly init` — scaffold a minimal app (ADR-039 / Step 21).

use puddly_core::{CLI_NAME, DISPLAY_NAME, MANIFEST_DIR};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

/// Scaffold a minimal Puddly app at `root`.
///
/// Creates `features/`, a sample `note` resource, `.puddly/`, README, and a copy of
/// framework conventions. Refuses if `features/` already exists.
pub fn init_app(root: impl AsRef<Path>) -> Result<InitResult, InitError> {
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
    fs::write(&readme, sample_readme())?;
    written.push(readme);

    Ok(InitResult { root, written })
}

fn sample_readme() -> String {
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
# {CLI_NAME} build .
# {CLI_NAME} serve . --bind 127.0.0.1:3000
```

Sample resource: `features/notes/note.resource.ts`. Add views under `features/*/`.
"#
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
