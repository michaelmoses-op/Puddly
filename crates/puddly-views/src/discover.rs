//! Discover `*.view.tsx` and `*.island.tsx` under an app root.

use std::path::{Path, PathBuf};

/// Discover view and island files, sorted by path for determinism.
pub fn discover_view_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    visit(root, &mut files, ".view.tsx")?;
    files.sort();
    Ok(files)
}

pub fn discover_island_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    visit(root, &mut files, ".island.tsx")?;
    files.sort();
    Ok(files)
}

fn visit(dir: &Path, out: &mut Vec<PathBuf>, suffix: &str) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(name, "node_modules" | "target" | ".git" | ".puddly") {
                continue;
            }
            visit(&path, out, suffix)?;
        } else if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// `todo.view.tsx` → `todo`
pub fn view_name_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.strip_suffix(".view.tsx")?.to_string())
}

/// `todo-row.island.tsx` → `TodoRow` (kebab → PascalCase)
pub fn island_name_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".island.tsx")?;
    Some(kebab_to_pascal(stem))
}

fn kebab_to_pascal(s: &str) -> String {
    s.split('-')
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            view_name_from_path(Path::new("features/todos/todo.view.tsx")).as_deref(),
            Some("todo")
        );
        assert_eq!(
            island_name_from_path(Path::new("todo-row.island.tsx")).as_deref(),
            Some("TodoRow")
        );
    }
}
