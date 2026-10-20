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

/// CSRF custom header required on mutating RPC calls (ADR-008 / Step 6).
/// Presence of a non-empty value is enough for the header-check defense.
pub const CSRF_HEADER_NAME: &str = "x-puddly-csrf";
