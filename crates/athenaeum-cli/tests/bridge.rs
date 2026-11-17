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

#[test]
fn bridge_rejects_identifier_style_near_duplicates_with_existing_id() {
    let dir = tempfile::tempdir().unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    let caller = json!({ "model": "curator-test", "session": "sess_bridge_test" });
    let created = bridge_ok(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "request": {
                "method": "create",
                "input": { "type": "symbol", "title": "AuthService" },
                "caller": caller
            }
        }),
    );
    let existing = created["response"]["node"]["id"].as_str().unwrap();

    let output = bridge(
        dir.path(),
        json!({
            "protocolVersion": 1,
            "requestId": "near-duplicate",
            "request": {
                "method": "create",
                "input": { "type": "symbol", "title": "auth_service" },
                "caller": caller
            }
        }),
    );
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "invalid_request");
    assert_eq!(response["error"]["details"]["existingNodes"][0], existing);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains(existing));
}

#[test]
fn bridge_tells_an_agent_which_file_a_page_node_is() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki/decisions")).unwrap();
    std::fs::write(
        dir.path().join("wiki/decisions/validate-state.md"),
        "---\ntype: decision\ntitle: Validate state first\n---\n\n# Validate state first\n\nCreating the session first opens a login CSRF.\n",
    )
    .unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    assert!(cli(dir.path(), &["index-vault"]).status.success());

    let capabilities = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "capabilities"}}),
    );
    assert!(
        capabilities["response"]["capabilities"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "page_pointers"),
        "an agent decides whether to offer the affordance from this"
    );

    let opened = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "open", "node": "Validate state first"}}),
    );
    let page = &opened["response"]["detail"]["page"];
    assert_eq!(page["path"], "wiki/decisions/validate-state.md");
    let absolute = page["absolutePath"].as_str().unwrap();
    assert!(Path::new(absolute).is_absolute(), "{absolute}");
    assert!(
        std::fs::read_to_string(absolute)
            .unwrap()
            .contains("login CSRF"),
        "the pointer must open the page itself"
    );

    let context = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "context", "task": "fix the OAuth callback", "seeds": ["Validate state first"]}}),
    );
    let brief = &context["response"]["context"];
    assert_eq!(brief["brief"]["nodes"][0]["page"]["absolutePath"], absolute);
    assert!(!brief["prompt"]
        .as_str()
        .unwrap()
        .contains(&format!("Page: {absolute}")));
}

#[test]
fn bridge_omits_the_page_for_a_node_that_is_not_one() {
    let dir = tempfile::tempdir().unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    assert!(cli(
        dir.path(),
        &[
            "add",
            "--type",
            "gotcha",
            "WAL on network shares",
            "-s",
            "Use a local path."
        ],
    )
    .status
    .success());

    let opened = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "open", "node": "WAL on network shares"}}),
    );

    // Absent, not null: a consumer that ignores unknown fields never has to tell the two apart.
    assert!(opened["response"]["detail"].get("page").is_none());
}

#[test]
fn bridge_answers_from_the_vault_as_it_is_now_not_as_it_was_indexed() {
    // An agent edits a page and creates another with its file tools, then asks. A headless
    // session has nothing watching the vault, so the bridge has to notice for itself.
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("wiki/concepts/oauthcallback.md");
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    std::fs::write(
        &page,
        "---\ntype: concept\ntitle: OAuthCallback\n---\n\n# OAuthCallback\n\nExchanges the code.\n",
    )
    .unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());
    assert!(cli(dir.path(), &["index-vault"]).status.success());
    let open =
        json!({"protocolVersion": 1, "request": {"method": "open", "node": "OAuthCallback"}});
    let before = bridge_ok(dir.path(), open.clone());
    let hash_before = before["response"]["detail"]["page"]["contentHash"].clone();
    assert!(!before["response"]["detail"]["node"]["content"]
        .as_str()
        .unwrap()
        .contains("state parameter"));

    std::fs::write(
        &page,
        "---\ntype: concept\ntitle: OAuthCallback\n---\n\n# OAuthCallback\n\nExchanges the code. Also validates the state parameter.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("wiki/gotchas.md"),
        "---\ntype: gotcha\ntitle: Session fixation\n---\n\n# Session fixation\n\nRotate the session id on login.\n",
    )
    .unwrap();
    let after = bridge_ok(dir.path(), open);

    assert!(after["response"]["detail"]["node"]["content"]
        .as_str()
        .unwrap()
        .contains("state parameter"));
    assert_ne!(
        after["response"]["detail"]["page"]["contentHash"],
        hash_before
    );
    let search = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "search", "query": "session fixation"}}),
    );
    assert_eq!(search["response"]["hits"].as_array().unwrap().len(), 1);
}

#[test]
fn bridge_does_not_build_a_first_index_on_a_read() {
    // A first index of a large vault would outlast what a waiting caller should be made to wait
    // for, so it stays explicit. Here nothing has been indexed, and a read must not start.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    std::fs::write(
        dir.path().join("wiki/a.md"),
        "---\ntype: concept\ntitle: Alpha\n---\n\n# Alpha\n\nBody.\n",
    )
    .unwrap();
    assert!(cli(dir.path(), &["init"]).status.success());

    let search = bridge_ok(
        dir.path(),
        json!({"protocolVersion": 1, "request": {"method": "search", "query": "alpha"}}),
    );

    assert!(search["response"]["hits"].as_array().unwrap().is_empty());
    let stats = cli(dir.path(), &["--json", "stats"]);
    let stats: Value = serde_json::from_slice(&stats.stdout).unwrap();
    assert_eq!(stats["nodes"], 0, "a read must not have indexed the vault");
}
