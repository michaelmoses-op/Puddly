//! Owned IR for a Mochi application.
//!
//! Spans are byte offsets into the original source (UTF-8). Serialization is deterministic:
//! maps are stored as sorted vectors.

mod span;
mod types;

pub use span::{SourceFile, Span};
pub use types::*;
