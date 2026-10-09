//! Versioned, one-shot client of the separate local core. Existing desktop
//! commands remain available while user data migrates between derived indexes.

use std::io::Write;
use std::process::{Command, Stdio};

use athenaeum_protocol::bridge::{
    BridgeOutcome, BridgeRequest, BridgeRequestEnvelope, BridgeResponseEnvelope,
};
use athenaeum_protocol::BRIDGE_PROTOCOL_VERSION;
use serde_json::Value;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[tauri::command]
pub async fn core_bridge(workspace: String, request: Value) -> Result<Value, String> {
    let parsed: BridgeRequest = serde_json::from_value(request)
        .map_err(|error| format!("invalid bridge request: {error}"))?;
    let method = parsed.method().to_string();
    let request_id = uuid::Uuid::new_v4().to_string();
    let envelope = BridgeRequestEnvelope {
        protocol_version: BRIDGE_PROTOCOL_VERSION,
        request_id: Some(request_id.clone()),
        request: parsed,
    };
    let payload = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    if payload.len() > MAX_REQUEST_BYTES {
        return Err("bridge request is too large".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let executable =
            std::env::var("ATHENAEUM_CLI_BIN").unwrap_or_else(|_| "athenaeum".to_string());
        let mut child = Command::new(&executable)
            .arg("-C")
            .arg(&workspace)
            .arg("bridge")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start {executable}: {error}"))?;
        child
            .stdin
            .take()
            .ok_or("bridge stdin was unavailable")?
            .write_all(&payload)
            .map_err(|error| format!("could not write bridge request: {error}"))?;
        let output = child
            .wait_with_output()
            .map_err(|error| format!("bridge failed: {error}"))?;
        if output.stdout.len() > MAX_RESPONSE_BYTES {
            return Err("bridge response is too large".to_string());
        }
        let reply: BridgeResponseEnvelope = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("bridge returned invalid JSON: {error}"))?;
        if reply.protocol_version != BRIDGE_PROTOCOL_VERSION
            || reply.request_id.as_deref() != Some(request_id.as_str())
        {
            return Err("bridge protocol version or request id mismatch".to_string());
        }
        match reply.outcome {
            BridgeOutcome::Ok { response } if response.method == method => {
                serde_json::to_value(response).map_err(|error| error.to_string())
            }
            BridgeOutcome::Ok { .. } => Err("bridge answered a different method".to_string()),
            BridgeOutcome::Error { error } => Err(format!("{}: {}", error.code, error.message)),
        }
    })
    .await
    .map_err(|error| format!("bridge task failed: {error}"))?
}
