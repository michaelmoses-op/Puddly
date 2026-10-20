use puddly_check::check;
use puddly_manifest::{
    build_manifest, render_manifest_json, stale_status, write_manifest, StaleStatus,
};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn todo_slice_manifest_deterministic() {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    assert!(result.ok, "{}", result.text);
    let ir = result.ir.unwrap();
    let a = build_manifest(&root, &ir);
    let b = build_manifest(&root, &ir);
    assert_eq!(
        render_manifest_json(&a),
        render_manifest_json(&b),
        "manifest JSON must be byte-identical"
    );
    insta::assert_snapshot!("todo_slice_manifest", render_manifest_json(&a));
}

#[test]
fn stale_check_detects_missing_and_fresh() {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    let ir = result.ir.unwrap();
    let expected = build_manifest(&root, &ir);

    // Fresh after write to a temp copy would be ideal; for the committed example,
    // generate into a temp dir.
    let tmp = std::env::temp_dir().join(format!("puddly-manifest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // Copy resource tree lightly by checking against empty tmp → Missing
    assert_eq!(stale_status(&tmp, &expected), StaleStatus::Missing);

    write_manifest(&tmp, &expected).unwrap();
    assert_eq!(stale_status(&tmp, &expected), StaleStatus::Fresh);

    // Corrupt and ensure Stale
    let path = puddly_manifest::manifest_path(&tmp);
    std::fs::write(&path, "{}\n").unwrap();
    assert_eq!(stale_status(&tmp, &expected), StaleStatus::Stale);

    let _ = std::fs::remove_dir_all(&tmp);
}
