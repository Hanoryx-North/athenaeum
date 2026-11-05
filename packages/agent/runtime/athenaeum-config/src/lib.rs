//! Athenaeum policy attached to the imported coding runtime, without knowledge storage.

use std::io;

use serde_json::Value;
use serde_json::json;

pub const INSTRUCTIONS: &str = include_str!("../instructions.md");

/// Keep deployment instructions alongside the product's knowledge contract.
pub fn developer_instructions(configured: Option<String>) -> String {
    match configured.filter(|text| !text.trim().is_empty()) {
        Some(text) => format!("{INSTRUCTIONS}\n\n{text}"),
        None => INSTRUCTIONS.to_string(),
    }
}

/// Stdio argv, never a shell command. The bridge supplies the knowledge and tool schemas.
/// The caller inserts this only when no `athenaeum` server was explicitly configured,
/// then applies upstream requirements/allowlists to the complete server map.
pub fn wiki_server(wiki_executable: &str) -> io::Result<Value> {
    if wiki_executable.trim().is_empty() || wiki_executable.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ATHENAEUM_BIN must name the wiki executable",
        ));
    }
    Ok(json!({
        "command": wiki_executable,
        "args": ["mcp", "--", "--allow", "propose", "--caller", "athenaeum-agent"],
        "required": false,
        "startup_timeout_sec": 20,
        "tool_timeout_sec": 120,
    }))
}
