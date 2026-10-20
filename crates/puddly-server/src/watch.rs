//! Filesystem watcher for `puddly dev` (notify + debounce).

use crate::dev::{
    apply_changes, classify_watch_path, ChangeKind, DevHandles, DEV_DEBOUNCE_MS,
};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use puddly_actions::ActionRunner;
use puddly_auth::SessionStore;
use puddly_core::DISPLAY_NAME;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;

/// Watch `root` recursively; on relevant changes, debounce and call [`apply_changes`].
pub async fn watch_dev_loop<S, R>(
    root: PathBuf,
    handles: Arc<DevHandles<S, R>>,
) -> Result<(), String>
where
    S: SessionStore + 'static,
    R: ActionRunner + 'static,
{
    let (fs_tx, fs_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = fs_tx.send(res);
        },
        notify::Config::default(),
    )
    .map_err(|e| e.to_string())?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    // Bridge sync notify channel → async via a blocking task + tokio mpsc.
    let (tx, mut rx) = tokio_mpsc::unbounded_channel::<PathBuf>();
    std::thread::spawn(move || {
        while let Ok(res) = fs_rx.recv() {
            match res {
                Ok(event) => {
                    if !is_interesting_event(&event.kind) {
                        continue;
                    }
                    for path in event.paths {
                        let _ = tx.send(path);
                    }
                }
                Err(e) => eprintln!("{DISPLAY_NAME} watch error: {e}"),
            }
        }
    });

    eprintln!(
        "{DISPLAY_NAME} watching {} (debounce {DEV_DEBOUNCE_MS}ms)",
        root.display()
    );

    let mut pending: HashSet<ChangeKind> = HashSet::new();
    let debounce = Duration::from_millis(DEV_DEBOUNCE_MS);

    loop {
        // Wait for first event, then debounce window.
        let first = rx.recv().await;
        let Some(path) = first else {
            break;
        };
        collect_kind(&path, &root, &mut pending);

        loop {
            match tokio::time::timeout(debounce, rx.recv()).await {
                Ok(Some(path)) => collect_kind(&path, &root, &mut pending),
                Ok(None) => break,
                Err(_) => break, // quiet for debounce window
            }
        }

        if pending.is_empty() {
            continue;
        }
        let kinds: Vec<ChangeKind> = pending.drain().collect();
        let outcome = apply_changes(
            &root,
            &kinds,
            &handles.views,
            &handles.state,
            handles.rpc.as_ref(),
        )
        .await;
        for msg in &outcome.messages {
            eprintln!("{DISPLAY_NAME} reload: {msg}");
        }
    }
    Ok(())
}

fn is_interesting_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

fn collect_kind(path: &Path, root: &Path, pending: &mut HashSet<ChangeKind>) {
    // Ignore paths outside root and noise under .puddly / target / node_modules.
    let Ok(canon_root) = root.canonicalize() else {
        if let Some(k) = classify_watch_path(path) {
            pending.insert(k);
        }
        return;
    };
    let path_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path_canon.starts_with(&canon_root) {
        return;
    }
    let rel = path_canon.strip_prefix(&canon_root).unwrap_or(&path_canon);
    let rel_str = rel.to_string_lossy();
    if rel_str.contains("node_modules")
        || rel_str.contains("/target/")
        || rel_str.starts_with("target/")
        || rel_str.contains("/.puddly/")
        || rel_str.starts_with(".puddly/")
        || rel_str.contains("/migrations/")
        || rel_str.starts_with("migrations/")
    {
        return;
    }
    if let Some(k) = classify_watch_path(&path_canon) {
        pending.insert(k);
    }
}
