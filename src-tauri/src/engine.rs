//! Typed desktop requests and responses for the versioned knowledge bridge.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core_bridge;
use crate::panic_guard::{run_guarded, run_guarded_async};

/// Project automation and batch acceptance share the CLI's core implementation.
#[tauri::command]
pub async fn wiki_automation(
    workspace: String,
    settings: Option<athenaeum_core::portable::automation::Automation>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let workspace = require_workspace(&workspace)?;
        let root = std::path::Path::new(&workspace);
        let project = athenaeum_core::workspace::Workspace::discover(root)
            .ok_or("No wiki is open at this path")?;
        let mut store = project.open().map_err(|e| e.to_string())?;
        let root = store.project_root().ok_or("This wiki has no folder")?;
        if let Some(settings) = settings {
            settings.save(&root).map_err(|e| e.to_string())?;
        }
        let settings = athenaeum_core::portable::automation::Automation::load(&root)
            .map_err(|e| e.to_string())?;
        let batch = athenaeum_core::portable::automation::maintain(&mut store)
            .map_err(|e| e.to_string())?;
        let revision = store.stats().map_err(|e| e.to_string())?.events;
        Ok(serde_json::json!({ "settings": settings, "batch": batch, "revision": revision }))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Requests: the closed set of methods the knowledge UI may ask for, as the
// bridge envelope itself (tag = method), so a request is written once.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum EngineRequest {
    /// Workspace and index state.
    Status,
    /// What needs a decision.
    Review {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// Every source the engine knows about.
    Sources {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// Reconcile the derived index.
    Index {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<String>>,
    },
    /// Proposals nobody has decided on.
    Proposals {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// A bounded knowledge brief, with the trust wrapper in `prompt`.
    Context {
        task: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u8>,
        #[serde(rename = "maxNodes", default, skip_serializing_if = "Option::is_none")]
        max_nodes: Option<u32>,
    },
    /// The evidence records behind one node — its relationships, evidence,
    /// history, utility and page, in one answer.
    Open { node: String },
    /// A node's recorded versions.
    History { node: String },
    /// How two nodes connect in the graph the engine holds.
    Path {
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u8>,
        #[serde(rename = "maxNodes", default, skip_serializing_if = "Option::is_none")]
        max_nodes: Option<u32>,
    },
}

// ---------------------------------------------------------------------------
// Responses: the wire shapes, kept apart from the engine's domain models.
// ---------------------------------------------------------------------------

/// `status` — what the workspace and its derived index hold right now.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub workspace_path: String,
    /// True once `.athenaeum/` exists and holds a knowledge base.
    pub initialized: bool,
    /// True when the project directory carries a Markdown vault at all.
    pub has_vault: bool,
    pub vault_pages: u64,
    pub nodes: u64,
    pub edges: u64,
    /// Proposals waiting for a person.
    pub proposals: u64,
    /// Review findings waiting for a person.
    pub findings: u64,
    /// When the derived index last reconciled against the vault, if ever.
    #[serde(default)]
    pub indexed_at: Option<String>,
    /// True when files changed after `indexed_at` and have not been indexed.
    pub index_stale: bool,
    pub protocol_version: u32,
    pub engine_version: String,
}

/// One thing the engine says needs a decision.
///
/// The vocabulary is closed and pinned: `severity` is `blocking`, `warning`
/// or `info`, `confidence` is a number in 0..=1 that is always present, and
/// `blocking` is the gate the UI reads (severity is for display).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineFinding {
    /// Deterministic: the same state yields the same id across runs.
    pub id: String,
    /// `contradiction`, `disputed`, `stale`, `stale_anchor`, `unsupported`,
    /// `duplicate`, `superseded`, `unreviewed_proposal`, `incomplete_scan`, …
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    /// Node ids this finding is about.
    pub nodes: Vec<String>,
    /// Evidence ids, addressable through `engine_evidence`.
    pub evidence: Vec<String>,
    /// The one file the finding is about, when it is about one.
    #[serde(default)]
    pub source_path: Option<String>,
    pub confidence: f64,
    pub blocking: bool,
}

/// One file the engine has seen, with what it scanned of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSource {
    /// Which scan read the file: `vault` or `repository`. One record exists per
    /// (scope, path), so the pair is the identity and nothing is collapsed.
    pub scope: String,
    /// Vault-relative or repository-relative, forward slashes.
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    /// False when the engine has never scanned this file. Not a failure — an
    /// absence, and the UI says so rather than implying coverage.
    pub scanned: bool,
    pub nodes: u64,
    pub anchors: u64,
    #[serde(default)]
    pub indexed_at: Option<String>,
    /// True when the file changed after it was indexed.
    pub stale: bool,
}

/// `index` — what the last reconcile over the vault and repository saw.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineIndex {
    pub vault_pages: u64,
    pub repository_files: u64,
    pub symbols: u64,
    pub edges: u64,
    pub duration_ms: u64,
    /// True when a budget cut the pass short, so the numbers are a floor.
    pub incomplete: bool,
    /// Paths the pass could not cover, capped by the engine. Empty when
    /// `incomplete` is false.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProvenance {
    pub source: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
}

/// A node as it crosses the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability: Option<athenaeum_protocol::bridge::Applicability>,
    pub authority: String,
    pub status: String,
    #[serde(default)]
    pub provenance: Option<EngineProvenance>,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineAnchor {
    pub file: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub line_start: Option<u64>,
    #[serde(default)]
    pub line_end: Option<u64>,
    #[serde(default)]
    pub region_hash: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineEvidence {
    pub id: String,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub anchor: Option<EngineAnchor>,
    #[serde(default)]
    pub blob_hash: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

/// A recorded version of a node. Accepting a proposal writes one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineVersion {
    pub node: String,
    pub version: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability: Option<athenaeum_protocol::bridge::Applicability>,
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRelationship {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub node_type: String,
    #[serde(default)]
    pub edge_type: String,
    #[serde(default)]
    pub edge_id: String,
    #[serde(default)]
    pub outbound: bool,
    /// False for a relationship nobody has accepted yet.
    #[serde(default)]
    pub established: bool,
    /// Additive in v1 and often absent: absent means the engine stated
    /// neither, never that the relationship is certain.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// A model's proposal, exactly as it sits in the derived index: not a page,
/// not knowledge, and impossible to make durable without a decision here.
///
/// v1 is exactly `{node, relationships, evidence}`. There is no proposal-level
/// confidence or reason on the wire; the UI reads those from the engine's
/// `unreviewed_proposal` finding for the same node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProposal {
    pub node: EngineNode,
    #[serde(default)]
    pub relationships: Vec<EngineRelationship>,
    #[serde(default)]
    pub evidence: Vec<EngineEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePage {
    /// Vault-relative path of the Markdown page acceptance wrote.
    pub path: String,
    #[serde(default)]
    pub absolute_path: Option<String>,
    #[serde(default)]
    pub content_hash: String,
}

/// A node with everything the engine holds about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineNodeDetail {
    pub node: EngineNode,
    #[serde(default)]
    pub relationships: Vec<EngineRelationship>,
    #[serde(default)]
    pub evidence: Vec<EngineEvidence>,
    #[serde(default)]
    pub history: Vec<EngineVersion>,
    pub utility: EngineUtility,
    #[serde(default)]
    pub page: Option<EnginePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUtility {
    pub retrievals: u64,
    pub uses: u64,
    pub citations: u64,
    pub verifications: u64,
    pub rejections: u64,
}

/// One hop of a connection: the node it starts from and the edge type crossed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePathStep {
    pub from: String,
    pub edge_type: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// How `from` connects to `to`. `found: false` is a normal answer, not an error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineGraphPath {
    pub from: String,
    pub to: String,
    pub found: bool,
    #[serde(default)]
    pub steps: Vec<EnginePathStep>,
    #[serde(default)]
    pub nodes: Vec<EngineNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineBriefNode {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub summary: String,
    pub authority: String,
    pub status: String,
    pub depth: u8,
    #[serde(default)]
    pub page: Option<EnginePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineBriefRelationship {
    pub from: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineBrief {
    pub task: String,
    #[serde(default)]
    pub nodes: Vec<EngineBriefNode>,
    #[serde(default)]
    pub relationships: Vec<EngineBriefRelationship>,
    #[serde(default)]
    pub anchors: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// The bounded retrieval the engine composes. `prompt` carries the trust
/// wrapper; the desktop shows it verbatim and never treats it as instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineContext {
    pub brief: EngineBrief,
    pub prompt: String,
}

/// The answer, tagged by the method the engine echoed back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum EngineResponse {
    Status { status: EngineStatus },
    Review { findings: Vec<EngineFinding> },
    Sources { sources: Vec<EngineSource> },
    Index { index: EngineIndex },
    Proposals { proposals: Vec<EngineProposal> },
    Context { context: EngineContext },
    Open { detail: Box<EngineNodeDetail> },
    History { versions: Vec<EngineVersion> },
    Path { path: EngineGraphPath },
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn require_workspace(workspace: &str) -> Result<String, String> {
    if workspace.trim().is_empty() {
        return Err("invalid_request: no workspace was given".to_string());
    }
    Ok(workspace.to_string())
}

/// Send one bridge request and return the response object.
async fn call(workspace: String, request: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || core_bridge::execute(&workspace, request))
        .await
        .map_err(|error| format!("engine task failed: {error}"))?
}

/// Check the method the engine echoed back before trusting the body.
fn expect_method<'a>(response: &'a Value, method: &str) -> Result<&'a Value, String> {
    match response.get("method").and_then(Value::as_str) {
        Some(answered) if answered == method => Ok(response),
        Some(answered) => Err(format!(
            "engine_protocol_error: asked for {method}, the engine answered {answered}"
        )),
        None => Err(format!(
            "engine_protocol_error: the {method} response has no method"
        )),
    }
}

fn parse_response<R: serde::de::DeserializeOwned>(
    method: &str,
    response: &Value,
) -> Result<R, String> {
    let response = expect_method(response, method)?;
    serde_json::from_value(response.clone()).map_err(|error| {
        format!(
            "engine_protocol_error: the {method} response did not match this app's protocol: {error}"
        )
    })
}

/// One typed knowledge request.
///
/// The request arrives in the bridge's own envelope, is validated against
/// [`EngineRequest`], and is what gets sent; the answer is validated against
/// [`EngineResponse`] before it is returned. Both halves of the contract are
/// checked at this boundary, and the interface never sees unchecked JSON.
#[tauri::command]
pub async fn engine_request(
    workspace: String,
    request: EngineRequest,
) -> Result<EngineResponse, String> {
    run_guarded_async("engine_request", async move {
        let workspace = require_workspace(&workspace)?;
        let method = method_of(&request);
        let outbound =
            serde_json::to_value(&request).map_err(|error| format!("invalid_request: {error}"))?;
        let response = call(workspace, outbound).await?;
        parse_response(method, &response)
    })
    .await
}

fn method_of(request: &EngineRequest) -> &'static str {
    match request {
        EngineRequest::Status => "status",
        EngineRequest::Review { .. } => "review",
        EngineRequest::Sources { .. } => "sources",
        EngineRequest::Index { .. } => "index",
        EngineRequest::Proposals { .. } => "proposals",
        EngineRequest::Context { .. } => "context",
        EngineRequest::Open { .. } => "open",
        EngineRequest::History { .. } => "history",
        EngineRequest::Path { .. } => "path",
    }
}

/// Which Library repository this project is linked to, if any.
///
/// Read from `athenaeum.json` and the sync state on disk. No network call is
/// made: the Library client lives in the CLI, and a window that quietly
/// contacted a hosted service would be exactly the background sync the product
/// refuses. Reachability is therefore reported as unknown, not guessed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLink {
    pub workspace_path: String,
    /// True when `athenaeum.json` exists at the project root.
    pub manifest_present: bool,
    #[serde(default)]
    pub repository_id: Option<String>,
    #[serde(default)]
    pub repository_name: Option<String>,
    /// True when the project holds a sync link to a Library repository.
    pub linked: bool,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub remote_id: Option<String>,
    #[serde(default)]
    pub remote_name: Option<String>,
    #[serde(default)]
    pub last_synced: Option<String>,
    #[serde(default)]
    pub last_sync_at: Option<String>,
    /// Paths whose divergent edits wait for a person.
    #[serde(default)]
    pub conflicts: Vec<String>,
}

#[tauri::command]
pub fn library_link(workspace: String) -> Result<LibraryLink, String> {
    run_guarded("library_link", || {
        let workspace = require_workspace(&workspace)?;
        let root = std::path::Path::new(&workspace);

        let manifest = athenaeum_core::portable::manifest::RepositoryManifest::read(root).map_err(
            |error| format!("library_state_unreadable: could not read athenaeum.json: {error}"),
        )?;
        let state = athenaeum_core::portable::sync::SyncState::load(root).map_err(|error| {
            format!("library_state_unreadable: could not read the sync state: {error}")
        })?;

        Ok(LibraryLink {
            workspace_path: workspace,
            manifest_present: manifest.is_some(),
            repository_id: manifest.as_ref().map(|m| m.repository_id.clone()),
            repository_name: manifest.as_ref().map(|m| m.name.clone()),
            linked: state.is_some(),
            server: state.as_ref().map(|s| s.server.clone()),
            remote_id: state.as_ref().map(|s| s.remote_id.clone()),
            remote_name: state.as_ref().and_then(|s| s.remote_name.clone()),
            last_synced: state.as_ref().and_then(|s| s.last_synced.clone()),
            last_sync_at: state.as_ref().and_then(|s| s.last_sync_at.clone()),
            conflicts: state.map(|s| s.conflicts).unwrap_or_default(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reviewed_scope_and_history_survive_desktop_response_validation() {
        let applicability = json!({"core": true, "projects": ["app"], "technologies": ["react"], "stages": ["initialize"]});
        let raw = json!({"method": "proposals", "proposals": [{"node": {
            "id": "example", "type": "procedure", "title": "Start small", "authority": "unverified_observation", "status": "valid", "applicability": applicability
        }, "relationships": [], "evidence": []}]});
        let parsed: EngineResponse = parse_response("proposals", &raw).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["proposals"][0]["node"]["applicability"],
            applicability
        );
        let history = json!({"method": "history", "versions": [{"node": "example", "version": 1, "authority": "unverified_observation", "status": "valid", "applicability": applicability}]});
        let parsed: EngineResponse = parse_response("history", &history).unwrap();
        let shown = serde_json::to_value(parsed).unwrap();
        assert_eq!(shown["versions"][0]["applicability"], applicability);
        assert_eq!(shown["versions"][0]["authority"], "unverified_observation");
    }
}
