//! Single place for the framework display name and CLI binary name.
//!
//! Change these constants (and the `puddly-cli` package binary name if needed) to rename.

/// Human-facing product name.
pub const DISPLAY_NAME: &str = "Puddly";

/// CLI command / binary name users type in the shell.
pub const CLI_NAME: &str = "puddly";

/// Directory (under an app root) for generated framework artifacts that are committed.
pub const MANIFEST_DIR: &str = ".puddly";

/// Committed manifest file name inside [`MANIFEST_DIR`].
pub const MANIFEST_FILE: &str = "manifest.json";

/// Directory (under an app root) for committed SQL migration files.
pub const MIGRATIONS_DIR: &str = "migrations";

/// Default session cookie name for server-side sessions.
pub const SESSION_COOKIE_NAME: &str = "puddly_session";
