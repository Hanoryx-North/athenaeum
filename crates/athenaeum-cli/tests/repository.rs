use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn cli(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_athenaeum"))
        .args(["-C", project.to_str().expect("UTF-8 test path")])
        .args(args)
        .output()
        .expect("run athenaeum")
}

#[test]
fn index_command_exposes_deterministic_repository_graph_as_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "mod auth;\npub fn start() {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/auth.rs"), "pub struct AuthService;\n").unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());

    let output = cli(dir.path(), &["--json", "index", "--max-commits", "0"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files"], 2);
    assert_eq!(report["resolvedImports"], 1);
    assert!(report["nodesCreated"].as_u64().unwrap() >= 4);

    let output = cli(dir.path(), &["--json", "search", "AuthService"]);
    assert!(output.status.success());
    let hits: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(hits[0]["node"]["type"], "symbol");
}

#[test]
fn relocate_command_is_available_without_any_anchored_evidence() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    let output = cli(dir.path(), &["--json", "relocate"]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["exact"], 0);
    assert_eq!(report["stale"], 0);
}
