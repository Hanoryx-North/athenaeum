//! The desktop's door to the one local knowledge engine.
//!
//! The UI sends the same versioned bridge request an agent sends over `athenaeum bridge`, and gets
//! the same answer, because both are served by [`athenaeum_core::bridge`]: this command calls it
//! in-process, and the CLI frames it over stdin and stdout. There is one implementation of the
//! contract, one index (`.athenaeum/knowledge.db`), and nothing for the desktop to install or find
//! on `PATH`. A request that cannot be answered fails with the protocol's own `code: message`.

use std::path::Path;

use athenaeum_core::bridge::{execute_value, MAX_REQUEST_BYTES};
use athenaeum_core::wiki::WikiOutcome;
use serde_json::Value;

/// Answer one bridge request for the knowledge base that contains `workspace`.
fn answer(workspace: &str, request: Value, request_id: String) -> Result<Value, String> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or("invalid bridge request: it has no method")?
        .to_string();
    if serde_json::to_vec(&request).map_or(true, |bytes| bytes.len() as u64 > MAX_REQUEST_BYTES) {
        return Err("request_too_large: the bridge request is too large".to_string());
    }
    let (envelope, _) = execute_value(request, Path::new(workspace), Some(request_id.clone()))
        .map_err(|error| error.to_string())?;
    if envelope.request_id.as_deref() != Some(request_id.as_str()) {
        return Err("bridge request id mismatch".to_string());
    }
    match envelope.outcome {
        WikiOutcome::Ok { response } => {
            let value = serde_json::to_value(&*response).map_err(|error| error.to_string())?;
            if value.get("method").and_then(Value::as_str) != Some(method.as_str()) {
                return Err("bridge answered a different method".to_string());
            }
            Ok(value)
        }
        WikiOutcome::Error { error } => Err(format!("{}: {}", error.code, error.message)),
    }
}

#[tauri::command]
pub async fn core_bridge(workspace: String, request: Value) -> Result<Value, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    // Retrieval walks the graph and may refresh the vault index: bounded work, but not work for the
    // UI thread.
    tauri::async_runtime::spawn_blocking(move || answer(&workspace, request, request_id))
        .await
        .map_err(|error| format!("bridge task failed: {error}"))?
}
