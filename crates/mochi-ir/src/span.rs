use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Byte offset span into a source file (UTF-8 indices). Inclusive start, exclusive end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }

    pub fn is_dummy(self) -> bool {
        self.start == 0 && self.end == 0
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// A source file loaded for parsing / diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
}

impl SourceFile {
    pub fn new(path: PathBuf, source: String) -> Self {
        Self { path, source }
    }

    /// 1-based line and column for a byte offset.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let offset = offset as usize;
        let mut line = 1u32;
        let mut col = 1u32;
        for (i, ch) in self.source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    pub fn line_text(&self, line: u32) -> &str {
        self.source
            .lines()
            .nth(line.saturating_sub(1) as usize)
            .unwrap_or("")
    }
}
