//! One-shot, language-neutral transport for the Wiki integration contract.

use std::io::{Read, Write};
use std::path::Path;

use athenaeum_core::wiki::{
    WikiRequest, WikiRequestEnvelope, WikiResponse, WikiResponseEnvelope, WikiService,
    WIKI_PROTOCOL_VERSION,
};
use athenaeum_core::workspace::Workspace;
use athenaeum_core::{Error, Result};

/// Execute one storage-independent Wiki request over stdin/stdout.
///
/// The bridge is intentionally one-shot for the first protocol version. It is
/// easy for any runtime language to integrate, holds no server open, and can
/// become a long-lived JSONL transport later without changing request or
/// response payloads.
pub fn run(workspace: Option<&Workspace>, requested_path: &Path) -> Result<bool> {
    const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;

    let (response, success) = if bytes.len() as u64 > MAX_REQUEST_BYTES {
        (
            WikiResponseEnvelope::error(
                None,
                "request_too_large",
                format!("bridge requests are limited to {MAX_REQUEST_BYTES} bytes"),
            ),
            false,
        )
    } else {
        execute(&bytes, workspace, requested_path)
    };

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &response)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(success)
}

fn execute(
    bytes: &[u8],
    workspace: Option<&Workspace>,
    requested_path: &Path,
) -> (WikiResponseEnvelope, bool) {
    let envelope = match serde_json::from_slice::<WikiRequestEnvelope>(bytes) {
        Ok(envelope) => envelope,
        Err(error) => {
            return (
                WikiResponseEnvelope::error(None, "invalid_request", error.to_string()),
                false,
            )
        }
    };

    let request_id = envelope.request_id;
    if envelope.protocol_version != WIKI_PROTOCOL_VERSION {
        return (
            WikiResponseEnvelope::error(
                request_id,
                "unsupported_protocol",
                format!(
                    "protocol v{} is not supported; this build supports v{}",
                    envelope.protocol_version, WIKI_PROTOCOL_VERSION
                ),
            ),
            false,
        );
    }

    let result = match envelope.request {
        WikiRequest::Capabilities => Ok(WikiResponse::Capabilities {
            capabilities: WikiService::capabilities(),
        }),
        request => match workspace {
            Some(workspace) => match workspace.open() {
                Ok(mut store) => WikiService::new(&mut store).handle(request),
                Err(error) => Err(error),
            },
            None => Err(Error::NoWorkspace {
                path: requested_path.display().to_string(),
            }),
        },
    };

    match result {
        Ok(value) => (WikiResponseEnvelope::success(request_id, value), true),
        Err(error) => {
            let response = match wiki_error_details(&error) {
                Some(details) => WikiResponseEnvelope::error_with_details(
                    request_id,
                    wiki_error_code(&error),
                    error.to_string(),
                    details,
                ),
                None => WikiResponseEnvelope::error(
                    request_id,
                    wiki_error_code(&error),
                    error.to_string(),
                ),
            };
            (response, false)
        }
    }
}

fn wiki_error_details(error: &Error) -> Option<serde_json::Value> {
    match error {
        Error::DuplicateTitle { title, existing } => Some(serde_json::json!({
            "title": title,
            "existingNodes": [existing],
        })),
        Error::NearDuplicateTitle { title, existing } => Some(serde_json::json!({
            "title": title,
            "existingNodes": existing,
        })),
        _ => None,
    }
}

fn wiki_error_code(error: &Error) -> &'static str {
    match error {
        Error::NodeNotFound(_) | Error::EdgeNotFound(_) | Error::EvidenceNotFound(_) => "not_found",
        Error::DuplicateId(_)
        | Error::DuplicateTitle { .. }
        | Error::NearDuplicateTitle { .. }
        | Error::DanglingEdge(_)
        | Error::SelfEdge(_)
        | Error::InvalidValue { .. }
        | Error::Invalid { .. } => "invalid_request",
        Error::SchemaTooNew { .. } => "incompatible_store",
        Error::Storage { .. }
        | Error::Sqlite(_)
        | Error::Io(_)
        | Error::Serde(_)
        | Error::RepositoryParser { .. }
        | Error::IncompleteRepositoryScan { .. } => "knowledge_error",
        Error::NoWorkspace { .. } => "workspace_not_found",
    }
}
