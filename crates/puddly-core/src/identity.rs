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

/// Double-submit CSRF cookie (ADR-048 / Step 30). Not HttpOnly so JS can mirror it
/// into [`CSRF_HEADER_NAME`]. SameSite/Secure parity with the session cookie.
pub const CSRF_COOKIE_NAME: &str = "puddly_csrf";

/// CSRF custom header required on mutating RPC calls (ADR-008 / ADR-048).
/// Value must equal the [`CSRF_COOKIE_NAME`] cookie (double-submit).
pub const CSRF_HEADER_NAME: &str = "x-puddly-csrf";
