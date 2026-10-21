//! Shared identity and helpers for the Puddly framework.
//!
//! Keep the display name and CLI command in [`identity`] so renaming is one edit.

pub mod identity;

pub use identity::{
    CLI_NAME, CSRF_COOKIE_NAME, CSRF_HEADER_NAME, DISPLAY_NAME, MANIFEST_DIR, MANIFEST_FILE,
    MIGRATIONS_DIR, SESSION_COOKIE_NAME,
};
