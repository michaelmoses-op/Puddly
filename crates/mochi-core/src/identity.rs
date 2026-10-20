//! Single place for the framework display name and CLI binary name.
//!
//! Change these constants (and the `mochi-cli` package binary name if needed) to rename.

/// Human-facing product name.
pub const DISPLAY_NAME: &str = "Mochi";

/// CLI command / binary name users type in the shell.
pub const CLI_NAME: &str = "mochi";

/// Directory (under an app root) for generated framework artifacts that are committed.
pub const MANIFEST_DIR: &str = ".mochi";

/// Committed manifest file name inside [`MANIFEST_DIR`].
pub const MANIFEST_FILE: &str = "manifest.json";
