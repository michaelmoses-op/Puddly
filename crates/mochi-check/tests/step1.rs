//! Step 1 acceptance tests: IR snapshots, every error code, determinism, no-panic.

use mochi_check::check;
use mochi_diagnostics::{ErrorCode, ERROR_REGISTRY};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

#[test]
fn valid_todo_slice_ok() {
    let result = check(fixture("examples/todo-slice"));
    assert!(result.ok, "expected ok, got:\n{}", result.text);
    let ir = result.ir.expect("ir");
    assert_eq!(ir.resources.len(), 3);
    let names: Vec<_> = ir.resources.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["project", "todo", "user"]);

    // Deterministic serialization of resources only (skip file bodies).
    let slim = serde_json::json!({
        "resources": ir.resources.iter().map(|r| {
            serde_json::json!({
                "name": r.name,
                "export_name": r.export_name,
                "path": normalize_paths(&r.path.to_string_lossy()),
                "fields": r.fields.iter().map(|f| serde_json::json!({
                    "name": f.name,
                    "kind": f.kind,
                    "options": f.options,
                })).collect::<Vec<_>>(),
                "constraints": r.constraints,
                "access": r.access.operations.iter().map(|op| serde_json::json!({
                    "name": op.name,
                    "rule": rule_name(&op.rule),
                })).collect::<Vec<_>>(),
                "actions": r.actions.iter().map(|a| serde_json::json!({
                    "name": a.name,
                    "run": a.run,
                    "input": a.input.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    });
    insta::assert_snapshot!(
        "todo_slice_ir",
        serde_json::to_string_pretty(&slim).unwrap()
    );
}

fn rule_name(rule: &mochi_ir::RuleIr) -> String {
    match rule {
        mochi_ir::RuleIr::Authenticated { .. } => "authenticated".into(),
        mochi_ir::RuleIr::Owner { field, .. } => format!("owner:{field}"),
        mochi_ir::RuleIr::Role { role, .. } => format!("role:{role}"),
        mochi_ir::RuleIr::Member { field, .. } => format!("member:{field}"),
    }
}

#[test]
fn every_error_code_has_fixture() {
    let codes_with_fixtures = [
        ErrorCode::E0001,
        ErrorCode::E0101,
        ErrorCode::E0201,
        ErrorCode::E0202,
        ErrorCode::E0203,
        ErrorCode::E0204,
        ErrorCode::E0205,
        ErrorCode::E0206,
        ErrorCode::E0207,
        ErrorCode::E0208,
        ErrorCode::E0209,
        ErrorCode::E0210,
    ];
    for code in codes_with_fixtures {
        let dir = fixture(&format!("tests/fixtures/invalid/{code}"));
        let result = check(&dir);
        assert!(!result.ok, "{code} fixture should fail");
        let primary: Vec<_> = result
            .report
            .primary()
            .map(|d| d.code)
            .collect();
        assert!(
            primary.contains(&code),
            "{code} fixture missing expected code; got {primary:?}\n{}",
            result.text
        );
        insta::assert_snapshot!(
            format!("error_{code}"),
            normalize_paths(&result.json)
        );
    }

    // Registry completeness for explain
    for doc in ERROR_REGISTRY {
        let text = mochi_diagnostics::format_explain(doc.code);
        assert!(text.contains(doc.code.as_str()));
        assert!(!doc.example_good.is_empty());
    }
}

#[test]
fn determinism_byte_identical() {
    let root = fixture("examples/todo-slice");
    let a = check(&root);
    let b = check(&root);
    assert_eq!(a.json, b.json, "json diagnostics must be byte-identical");
    let ir_a = serde_json::to_string(a.ir.as_ref().unwrap()).unwrap();
    let ir_b = serde_json::to_string(b.ir.as_ref().unwrap()).unwrap();
    assert_eq!(ir_a, ir_b, "IR must be byte-identical across runs");

    let bad = fixture("tests/fixtures/invalid/E0204");
    let c = check(&bad);
    let d = check(&bad);
    assert_eq!(c.json, d.json);
    assert_eq!(c.text, d.text);
}

#[test]
fn never_panics_on_malformed_input() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let repeated = "export const X = resource()\n".repeat(100);
    let cases: Vec<&str> = vec![
        "",
        "{",
        "export const X = ",
        "\0\0\0",
        repeated.as_str(),
        "🔥🔥 export const X = resource(\"x\", { fields: {",
    ];

    // Write temp files and check — catch_unwind around the whole check.
    let tmp = tempfile_dir();
    for (i, src) in cases.iter().enumerate() {
        let path = tmp.join(format!("case{i}.resource.ts"));
        std::fs::write(&path, src).unwrap();
        let root = tmp.clone();
        let result = catch_unwind(AssertUnwindSafe(|| check(&root)));
        assert!(
            result.is_ok(),
            "check panicked on malformed case {i}: {:?}",
            &src.chars().take(40).collect::<String>()
        );
        let _ = std::fs::remove_file(&path);
    }

    // Also invoke parser directly on outrageous strings
    for src in &cases {
        let result = catch_unwind(AssertUnwindSafe(|| {
            mochi_parse::parse_resource_file(Path::new("fuzz.resource.ts"), src)
        }));
        assert!(
            result.is_ok(),
            "parse panicked on: {:?}",
            &src.chars().take(40).collect::<String>()
        );
    }
}

#[test]
fn json_output_is_stable_shape() {
    let result = check(fixture("tests/fixtures/invalid/E0204"));
    let v: Value = serde_json::from_str(&result.json).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["error_count"].as_u64().unwrap() >= 1);
    assert!(v["diagnostics"].as_array().unwrap()[0]["code"] == "E0204");
}

fn normalize_paths(s: &str) -> String {
    let root = workspace_root();
    let root_str = root.to_string_lossy();
    s.replace(&*root_str, "<WORKSPACE>")
        .replace('\\', "/")
}

fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mochi-nopanic-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
