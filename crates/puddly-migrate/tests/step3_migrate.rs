use puddly_check::check;
use puddly_migrate::{
    drift_status, generate_migration, render_schema_sql, DriftStatus,
};
use puddly_schema::schema_from_app;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn init_migration_deterministic_and_fresh() {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    assert!(result.ok, "{}", result.text);
    let ir = result.ir.unwrap();
    let schema = schema_from_app(&ir);
    let sql = render_schema_sql(&schema);
    insta::assert_snapshot!("todo_slice_schema_sql", sql);

    assert!(
        matches!(drift_status(&root, &ir), DriftStatus::Fresh),
        "committed migrations must match current schema"
    );

    let gen = generate_migration(&root, &ir, false).unwrap();
    assert!(gen.path.is_none());
    assert!(gen.message.contains("up to date"));
}

#[test]
fn generate_init_in_temp_and_detect_drift() {
    let src = workspace_root().join("examples/todo-slice");
    let result = check(&src);
    let ir = result.ir.unwrap();

    let tmp = std::env::temp_dir().join(format!("puddly-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    assert!(matches!(drift_status(&tmp, &ir), DriftStatus::Missing));
    let gen = generate_migration(&tmp, &ir, false).unwrap();
    assert!(gen.path.is_some());
    assert!(matches!(drift_status(&tmp, &ir), DriftStatus::Fresh));

    // Corrupt fingerprint → drift
    let mig = gen.path.unwrap();
    let mut text = std::fs::read_to_string(&mig).unwrap();
    text = text.replace(
        &format!("-- puddly-schema-fingerprint: {}", gen.fingerprint),
        "-- puddly-schema-fingerprint: deadbeef",
    );
    std::fs::write(&mig, text).unwrap();
    assert!(matches!(drift_status(&tmp, &ir), DriftStatus::Drift { .. }));

    let _ = std::fs::remove_dir_all(&tmp);
}
