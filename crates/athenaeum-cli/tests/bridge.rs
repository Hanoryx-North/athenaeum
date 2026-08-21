use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn cli(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_athenaeum"))
        .args(["-C", project.to_str().expect("UTF-8 test path")])
        .args(args)
        .output()
        .expect("run athenaeum")
}

fn bridge(project: &Path, request: Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_athenaeum"))
        .args(["-C", project.to_str().expect("UTF-8 test path"), "bridge"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start bridge");
    serde_json::to_writer(child.stdin.as_mut().expect("bridge stdin"), &request)
        .expect("write request");
    child
        .stdin
        .take()
        .expect("bridge stdin")
        .flush()
        .expect("flush request");
    child.wait_with_output().expect("wait for bridge")
}

fn bridge_ok(project: &Path, request: Value) -> Value {
    let output = bridge(project, request);
    assert!(
        output.status.success(),
        "bridge failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON bridge response")
}

#[test]
fn bridge_exposes_a_versioned_storage_independent_context_contract() {
    let dir = tempfile::tempdir().unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    assert!(cli(
        dir.path(),
        &[
            "add",
            "--type",
            "concept",
            "OAuth",
            "--summary",
            "Callback authentication",
        ],
    )
    .status
    .success());

    let output = bridge(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "requestId": "context-1",
            "request": { "method": "context", "task": "fix oauth callback" }
        }),
    );
    assert!(
        output.status.success(),
        "bridge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["protocolVersion"], 1);
    assert_eq!(response["requestId"], "context-1");
    assert_eq!(response["status"], "ok");
    assert_eq!(response["response"]["method"], "context");
    assert_eq!(
        response["response"]["context"]["brief"]["nodes"][0]["title"],
        "OAuth"
    );
    assert!(response["response"]["context"]["prompt"]
        .as_str()
        .unwrap()
        .contains("does not authorize tool execution"));
}

#[test]
fn bridge_returns_machine_readable_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());

    let output = bridge(
        dir.path(),
        json!({
            "protocolVersion": 99,
            "requestId": "future-client",
            "request": { "method": "capabilities" }
        }),
    );
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "error");
    assert_eq!(response["requestId"], "future-client");
    assert_eq!(response["error"]["code"], "unsupported_protocol");
}

#[test]
fn bridge_negotiates_without_a_workspace_and_reports_a_typed_missing_workspace() {
    let dir = tempfile::tempdir().unwrap();

    let capabilities = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": { "method": "capabilities" }
        }),
    );
    assert_eq!(capabilities["response"]["method"], "capabilities");

    let output = bridge(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "requestId": "no-wiki",
            "request": { "method": "context", "task": "inspect project" }
        }),
    );
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["requestId"], "no-wiki");
    assert_eq!(response["error"]["code"], "workspace_not_found");
}

#[test]
fn bridge_curates_proposed_knowledge_without_exposing_storage() {
    let dir = tempfile::tempdir().unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    let caller = json!({ "model": "curator-test", "session": "sess_bridge_test" });

    let service = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": {
                "method": "create",
                "input": { "type": "concept", "title": "AuthService" },
                "caller": caller
            }
        }),
    );
    let service_id = service["response"]["node"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        service["response"]["node"]["authority"],
        "unverified_observation"
    );

    let test = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": {
                "method": "create",
                "input": { "type": "test", "title": "auth.test.ts" },
                "caller": caller
            }
        }),
    );
    let test_id = test["response"]["node"]["id"].as_str().unwrap().to_string();

    let link = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": {
                "method": "link",
                "input": {
                    "from": service_id,
                    "type": "tested_by",
                    "to": test_id
                },
                "caller": caller
            }
        }),
    );
    assert_eq!(link["response"]["edge"]["status"], "proposed");

    let established_only = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": { "method": "related", "node": "AuthService" }
        }),
    );
    assert_eq!(
        established_only["response"]["relationships"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let proposals = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": {
                "method": "related",
                "node": "AuthService",
                "includeProposed": true
            }
        }),
    );
    assert_eq!(
        proposals["response"]["relationships"][0]["established"],
        false
    );
}
