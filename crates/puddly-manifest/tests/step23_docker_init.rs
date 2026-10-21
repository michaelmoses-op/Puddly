//! Step 23: `puddly init --docker` scaffold (ADR-041).

use puddly_check::check;
use puddly_manifest::{init_app_with_options, InitOptions};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("puddly-init-docker-{nanos}"))
}

#[test]
fn init_with_docker_writes_compose_files() {
    let root = temp_root();
    let _ = fs::remove_dir_all(&root);

    let result = init_app_with_options(&root, &InitOptions { docker: true }).expect("init");
    assert!(root.join("Dockerfile").is_file());
    assert!(root.join("docker-compose.yml").is_file());
    assert!(root.join(".dockerignore").is_file());
    assert!(result.written.iter().any(|p| p.ends_with("Dockerfile")));

    let dockerfile = fs::read_to_string(root.join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("serve"));
    assert!(
        dockerfile.contains("NOT run on startup") || dockerfile.contains("Explicit migrate"),
        "Dockerfile must document no startup migrate"
    );
    assert!(
        !dockerfile.contains("CMD [\"puddly\", \"migrate\""),
        "Dockerfile CMD must not migrate"
    );

    let compose = fs::read_to_string(root.join("docker-compose.yml")).unwrap();
    assert!(compose.contains("\"migrate\""));
    assert!(compose.contains("\"apply\""));
    assert!(compose.contains("postgres:"));

    let checked = check(&root);
    assert!(checked.ok, "check after docker init:\n{}", checked.text);

    let _ = fs::remove_dir_all(&root);
}
