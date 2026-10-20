//! Step 21: `puddly init` scaffold then `check` succeeds (ADR-039).

use puddly_check::check;
use puddly_manifest::init_app;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("puddly-init-{nanos}"))
}

#[test]
fn init_into_temp_dir_then_check_succeeds() {
    let root = temp_root();
    let _ = fs::remove_dir_all(&root);

    let result = init_app(&root).expect("init");
    assert!(root.join("features/notes/note.resource.ts").is_file());
    assert!(root.join("CONVENTIONS.md").is_file());
    assert!(root.join("README.md").is_file());
    assert!(root.join(".puddly").is_dir());
    assert!(result.written.len() >= 3);

    let checked = check(&root);
    assert!(
        checked.ok,
        "check after init must succeed:\n{}",
        checked.text
    );
    let ir = checked.ir.expect("ir");
    assert!(
        ir.resources.iter().any(|r| r.name == "note"),
        "expected note resource"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn init_refuses_existing_features() {
    let root = temp_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("features")).unwrap();
    let err = init_app(&root).expect_err("should refuse");
    assert!(
        err.to_string().contains("already exists"),
        "err={err}"
    );
    let _ = fs::remove_dir_all(&root);
}
