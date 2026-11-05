use athenaeum_runtime_config::developer_instructions;
use athenaeum_runtime_config::wiki_server;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn deployment_instructions_survive_product_defaults() {
    let instructions =
        developer_instructions(Some("Use the project's existing test runner.".into()));
    assert!(instructions.starts_with("You are Athenaeum"));
    assert!(instructions.ends_with("Use the project's existing test runner."));
    assert!(instructions.contains("reference material"));
    assert!(instructions.contains("Never accept your own proposals"));
    assert!(instructions.contains("wiki_context"));
    assert!(instructions.contains("wiki_create"));
}

#[test]
fn optional_wiki_has_argv_and_only_proposal_authority() {
    let executable = "C:\\Program Files\\Athenaeum\\wiki.exe";
    let server = wiki_server(executable).expect("valid executable path");
    assert_eq!(server["command"], executable);
    assert_eq!(server["required"], false);
    assert_eq!(
        server["args"],
        serde_json::json!([
            "mcp",
            "--",
            "--allow",
            "propose",
            "--caller",
            "athenaeum-agent"
        ])
    );
    assert!(server.get("env").is_none());
}

#[test]
fn invalid_wiki_executable_fails_configuration() {
    assert!(wiki_server("").is_err());
    assert!(wiki_server("  ").is_err());
    assert!(wiki_server("wiki\0--allow review").is_err());
}

fn probe() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_native-home-probe"));
    command.env_remove("ATHENAEUM_AGENT_HOME");
    command
}

#[test]
fn native_default_home_is_separate_from_codex() {
    let output = probe().output().expect("run home resolver");
    assert!(output.status.success(), "{:?}", output);
    let actual = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    assert_eq!(actual, dirs::home_dir().unwrap().join(".athenaeum-agent"));
}

#[test]
fn native_custom_home_is_canonicalized() {
    let directory = tempfile::tempdir().unwrap();
    let output = probe()
        .env("ATHENAEUM_AGENT_HOME", directory.path())
        .output()
        .expect("run home resolver");
    assert!(output.status.success(), "{:?}", output);
    let actual = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    assert_eq!(
        actual.canonicalize().unwrap(),
        directory.path().canonicalize().unwrap()
    );
}

#[test]
fn missing_or_file_home_is_an_error_not_a_codex_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let missing = probe()
        .env("ATHENAEUM_AGENT_HOME", directory.path().join("missing"))
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let file = directory.path().join("not-a-directory");
    std::fs::write(&file, b"test").unwrap();
    let output = probe().env("ATHENAEUM_AGENT_HOME", &file).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ATHENAEUM_AGENT_HOME"));
}
