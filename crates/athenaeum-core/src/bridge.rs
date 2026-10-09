//! The Wiki bridge, as a function: one versioned request in, one envelope out.
//!
//! `athenaeum bridge` reads a request from stdin and writes the envelope to stdout; the desktop app
//! hands the same bytes to this function in-process. Both therefore run exactly one implementation
//! of the contract, including how a vault edited since the last read is brought up to date and how
//! an engine error becomes a protocol error code. This module does no I/O of its own, so the
//! crate still has no network or process dependency.

use std::path::Path;

use crate::error::{Error, Result};
use crate::store::KnowledgeStore;
use crate::vault::{VaultIndexer, VaultScanOptions};
use crate::wiki::{
    WikiRequest, WikiRequestEnvelope, WikiResponse, WikiResponseEnvelope, WikiService,
    WIKI_PROTOCOL_VERSION,
};
use crate::workspace::Workspace;

/// The largest request the bridge will read. Bounds memory on every front end alike.
pub const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

/// Execute one serialized request envelope.
///
/// `workspace` is the knowledge base found for `requested_path`, or `None` when there is none, in
/// which case every request except `capabilities` answers `workspace_not_found`: a missing
/// knowledge base is reported, never created. The boolean is whether the request succeeded, which
/// the CLI maps to its exit status.
pub fn execute(
    bytes: &[u8],
    workspace: Option<&Workspace>,
    requested_path: &Path,
) -> (WikiResponseEnvelope, bool) {
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return (
            WikiResponseEnvelope::error(
                None,
                "request_too_large",
                format!("bridge requests are limited to {MAX_REQUEST_BYTES} bytes"),
            ),
            false,
        );
    }
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
                Ok(mut store) => {
                    if request.reads_knowledge() {
                        refresh_vault_index(&mut store, workspace);
                    }
                    WikiService::new(&mut store).handle(request)
                }
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
            let response = match error_details(&error) {
                Some(details) => WikiResponseEnvelope::error_with_details(
                    request_id,
                    error_code(&error),
                    error.to_string(),
                    details,
                ),
                None => WikiResponseEnvelope::error(request_id, error_code(&error), error.to_string()),
            };
            (response, false)
        }
    }
}

/// Run a request against the knowledge base that contains `path`, if there is one.
pub fn execute_at(bytes: &[u8], path: &Path) -> (WikiResponseEnvelope, bool) {
    let workspace = Workspace::discover(path);
    execute(bytes, workspace.as_ref(), path)
}

/// Bring the vault index up to date before a read is answered from it.
///
/// An agent changes the vault with ordinary file tools, and a headless session has nothing
/// watching the vault the way the desktop app does, so without this a question asked just after an
/// edit is answered from the index as it was before it, with nothing to say so. A metadata-only
/// comparison decides whether anything moved; an unchanged vault costs one directory walk.
///
/// A failure here must never fail the read. The index it falls back on is exactly what the answer
/// would have been without this step, so the failure is reported on stderr, where a transport's
/// JSON on stdout is untouched, and the request proceeds.
fn refresh_vault_index(store: &mut KnowledgeStore, workspace: &Workspace) {
    let Some(project) = workspace.project_root() else {
        return;
    };
    if let Err(error) = VaultIndexer::sync_if_changed(store, project, VaultScanOptions::default()) {
        eprintln!(
            "athenaeum: could not refresh the vault index, answering from the last one: {error}"
        );
    }
}

fn error_details(error: &Error) -> Option<serde_json::Value> {
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

/// The stable protocol code for an engine error. Codes are part of the contract.
pub fn error_code(error: &Error) -> String {
    if let Error::Portable { code, .. } = error {
        return code.clone();
    }
    let code: &'static str = match error {
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
        Error::Portable { .. } => unreachable!("handled above"),
    };
    code.to_string()
}

/// Convenience for callers that only need the result of a request they built themselves.
pub fn execute_value(
    request: serde_json::Value,
    path: &Path,
    request_id: Option<String>,
) -> Result<(WikiResponseEnvelope, bool)> {
    let envelope = serde_json::json!({
        "protocolVersion": WIKI_PROTOCOL_VERSION,
        "requestId": request_id,
        "request": request,
    });
    let bytes = serde_json::to_vec(&envelope)?;
    Ok(execute_at(&bytes, path))
}
