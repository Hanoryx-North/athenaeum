//! The portable repository, driven through the real binary: accepted knowledge is a page, an
//! index can be deleted without losing it, and a repository survives export and import.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn cli(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_athenaeum"))
        .args(["-C", project.to_str().unwrap()])
        .args(args)
        .env("ATHENAEUM_CONFIG_DIR", project.join(".test-config"))
        .output()
        .expect("run athenaeum")
}

fn ok(project: &Path, args: &[&str]) -> String {
    let output = cli(project, args);
    assert!(
        output.status.success(),
        "{args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn bridge(project: &Path, request: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_athenaeum"))
        .args(["-C", project.to_str().unwrap(), "bridge"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.as_mut().unwrap(), &request).unwrap();
    child.stdin.take().unwrap().flush().unwrap();
    let output = child.wait_with_output().unwrap();
    serde_json::from_slice(&output.stdout).expect("JSON response")
}

fn call(project: &Path, request: Value) -> Value {
    let envelope = bridge(
        project,
        json!({ "protocolVersion": 1, "requestId": "t", "request": request }),
    );
    assert_eq!(envelope["status"], "ok", "{envelope}");
    envelope["response"].clone()
}

fn delete_index(project: &Path) {
    for name in ["knowledge.db", "knowledge.db-wal", "knowledge.db-shm"] {
        let _ = std::fs::remove_file(project.join(".athenaeum").join(name));
    }
}

#[test]
fn a_statement_is_a_page_and_survives_deleting_the_database() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init"]);
    ok(
        dir.path(),
        &[
            "add",
            "--stated",
            "--type",
            "decision",
            "--id",
            "Decision_042",
            "Use SQLite",
            "--summary",
            "SQLite is the store.",
        ],
    );
    let page = dir.path().join("wiki/decisions/use-sqlite.md");
    assert!(page.exists(), "a stated node must be written to the vault");
    assert!(std::fs::read_to_string(&page)
        .unwrap()
        .contains("id: Decision_042"));

    delete_index(dir.path());
    ok(dir.path(), &["index-vault"]);
    let shown = ok(dir.path(), &["--json", "show", "Decision_042"]);
    assert!(shown.contains("Use SQLite"), "{shown}");
}

#[test]
fn a_proposal_becomes_permanent_only_when_accepted() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init"]);

    let created = call(
        dir.path(),
        json!({
            "method": "create",
            "input": { "type": "gotcha", "title": "Migrations run twice in CI", "summary": "The CI job retries the migration step." },
            "caller": { "model": "curator", "session": "sess_1" }
        }),
    );
    let id = created["node"]["id"].as_str().unwrap().to_string();

    // A proposal is not durable: it is in the database and nowhere else.
    let proposals = call(dir.path(), json!({ "method": "proposals" }));
    assert_eq!(proposals["proposals"].as_array().unwrap().len(), 1);
    assert!(!dir.path().join("wiki").exists());

    let accepted = call(
        dir.path(),
        json!({ "method": "accept", "node": id, "caller": { "model": "user" } }),
    );
    let path = accepted["accepted"]["page"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(path.starts_with("wiki/gotchas/"), "{path}");
    assert!(dir.path().join(&path).exists());
    assert_eq!(
        call(dir.path(), json!({ "method": "proposals" }))["proposals"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Lose every index. The accepted knowledge is still there, with its id and its authority.
    delete_index(dir.path());
    ok(dir.path(), &["index-vault"]);
    let opened = call(dir.path(), json!({ "method": "open", "node": id }));
    assert_eq!(
        opened["detail"]["node"]["title"],
        "Migrations run twice in CI"
    );
    assert_eq!(
        opened["detail"]["node"]["authority"],
        "unverified_observation"
    );
    assert_eq!(opened["detail"]["page"]["path"], path.as_str());
}

#[test]
fn a_rejected_proposal_is_gone_for_good_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init"]);
    let created = call(
        dir.path(),
        json!({
            "method": "create",
            "input": { "type": "observation", "title": "A hunch" },
            "caller": { "model": "curator" }
        }),
    );
    let id = created["node"]["id"].as_str().unwrap();
    let rejected = call(
        dir.path(),
        json!({ "method": "reject", "node": id, "reason": "noise", "caller": { "model": "user" } }),
    );
    assert_eq!(rejected["node"]["status"], "archived");
    assert_eq!(
        call(dir.path(), json!({ "method": "proposals" }))["proposals"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(!dir.path().join("wiki").exists());
}

#[test]
fn export_then_import_recovers_the_knowledge_and_caps_what_arrived() {
    let source = tempfile::tempdir().unwrap();
    ok(source.path(), &["init"]);
    ok(source.path(), &["repo", "init", "--name", "payments"]);
    std::fs::create_dir_all(source.path().join("wiki/rules")).unwrap();
    std::fs::write(
        source.path().join("wiki/rules/deploy.md"),
        "---\nid: Rule_7\ntype: rule\ntitle: Deploy on Fridays\nauthority: user_instruction\n---\nAlways.\n",
    )
    .unwrap();
    ok(source.path(), &["index-vault"]);

    let parent = tempfile::tempdir().unwrap();
    let zip = parent.path().join("out.zip");
    ok(source.path(), &["export", zip.to_str().unwrap()]);

    // Writing the archive into the repository it exports would put it inside itself.
    let inside = source.path().join("inside.zip");
    assert!(!cli(source.path(), &["export", inside.to_str().unwrap()])
        .status
        .success());

    let destination = parent.path().join("restored");
    ok(
        parent.path(),
        &[
            "import",
            zip.to_str().unwrap(),
            destination.to_str().unwrap(),
        ],
    );

    let shown: Value =
        serde_json::from_str(&ok(&destination, &["--json", "show", "Rule_7"])).unwrap();
    assert_eq!(shown["title"], "Deploy on Fridays");
    // The text said user_instruction; the text came from outside; so it counts as imported.
    assert_eq!(shown["authority"], "imported_knowledge");

    let trusted = ok(&destination, &["trust", "list"]);
    assert!(trusted.contains("wiki/rules/deploy.md"), "{trusted}");
    ok(&destination, &["trust", "adopt", "wiki/rules/deploy.md"]);
    let shown: Value =
        serde_json::from_str(&ok(&destination, &["--json", "show", "Rule_7"])).unwrap();
    assert_eq!(shown["authority"], "user_instruction");

    // An import never overwrites.
    let again = cli(
        parent.path(),
        &[
            "import",
            zip.to_str().unwrap(),
            destination.to_str().unwrap(),
        ],
    );
    assert!(!again.status.success());
}

#[test]
fn materialize_is_a_dry_run_until_told_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init"]);
    // A node stated before this feature existed: in the database, in no file.
    {
        use athenaeum_core::prelude::*;
        let mut store = Workspace::discover(dir.path()).unwrap().open().unwrap();
        store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Old decision")
                    .with_provenance(Provenance::user()),
            )
            .unwrap();
    }
    let dry = ok(dir.path(), &["vault", "materialize"]);
    assert!(dry.contains("dry run"), "{dry}");
    assert!(!dir.path().join("wiki").exists());
    ok(dir.path(), &["vault", "materialize", "--apply"]);
    assert!(dir.path().join("wiki/decisions/old-decision.md").exists());
}

#[test]
fn syncing_without_a_link_says_so_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init"]);
    let output = cli(dir.path(), &["--json", "sync", "status"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "not_linked");
}
