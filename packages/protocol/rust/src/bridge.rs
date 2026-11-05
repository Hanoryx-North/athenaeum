use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BRIDGE_PROTOCOL_VERSION;

/// Every method of bridge v1, in the order `capabilities` reports them.
pub const BRIDGE_METHODS: &[&str] = &[
    "capabilities",
    "search",
    "open",
    "navigate",
    "related",
    "evidence",
    "history",
    "context",
    "create",
    "update",
    "link",
    "attach_evidence",
    "record_use",
    "proposals",
    "accept",
    "reject",
    "status",
    "review",
    "sources",
    "index",
    "path",
];

/// `review.findings[].kind` values of bridge v1.
pub const BRIDGE_REVIEW_KINDS: &[&str] = &[
    "contradiction",
    "dispute",
    "stale_anchor",
    "unsupported_rule",
    "unreviewed_proposal",
    "duplicate",
    "near_duplicate",
    "superseded",
    "incomplete_scan",
    "orphan",
];

/// `review.findings[].severity` values of bridge v1.
pub const BRIDGE_REVIEW_SEVERITIES: &[&str] = &["blocking", "warning", "info"];

/// `index` scopes of bridge v1.
pub const BRIDGE_INDEX_SCOPES: &[&str] = &["vault", "repository", "all"];

/// `sources.sources[].scope` values of bridge v1: which scan read the file.
pub const BRIDGE_SOURCE_SCOPES: &[&str] = &["vault", "repository"];

/// `error.code` values of a bridge error response.
pub const BRIDGE_ERROR_CODES: &[&str] = &[
    "invalid_request",
    "not_found",
    "incompatible_store",
    "knowledge_error",
    "request_too_large",
    "unsupported_protocol",
    "workspace_not_found",
];

/// One JSON object on stdin, followed by exactly one response on stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequestEnvelope {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub request: BridgeRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BridgeRequest {
    Capabilities,
    Search {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
    },
    Open {
        node: String,
    },
    Navigate {
        node: String,
        #[serde(default)]
        depth: Option<u8>,
        #[serde(default)]
        max_nodes: Option<usize>,
    },
    Related {
        node: String,
        #[serde(default)]
        include_proposed: bool,
    },
    Evidence {
        node: String,
    },
    History {
        node: String,
    },
    Context {
        task: String,
        #[serde(default)]
        seeds: Vec<String>,
        #[serde(default)]
        depth: Option<u8>,
        #[serde(default)]
        max_nodes: Option<usize>,
    },
    Create {
        input: NodeCreate,
        caller: Caller,
    },
    Update {
        input: NodeUpdate,
        caller: Caller,
    },
    Link {
        input: LinkCreate,
        caller: Caller,
    },
    AttachEvidence {
        input: EvidenceCreate,
        caller: Caller,
    },
    RecordUse {
        nodes: Vec<String>,
        #[serde(default)]
        session: Option<String>,
    },
    Proposals {
        #[serde(default)]
        limit: Option<u32>,
    },
    Accept {
        node: String,
        caller: Caller,
    },
    Reject {
        node: String,
        #[serde(default)]
        reason: Option<String>,
        caller: Caller,
    },
    Status,
    Review {
        #[serde(default)]
        limit: Option<u32>,
    },
    Sources {
        #[serde(default)]
        limit: Option<u32>,
    },
    Index {
        scope: BridgeIndexScope,
        #[serde(default)]
        paths: Vec<String>,
    },
    Path {
        from: String,
        to: String,
        #[serde(default)]
        depth: Option<u8>,
        #[serde(default)]
        max_nodes: Option<usize>,
    },
}

/// What `index` rescans: the vault, the repository, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeIndexScope {
    Vault,
    Repository,
    All,
}

impl BridgeRequest {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Search { .. } => "search",
            Self::Open { .. } => "open",
            Self::Navigate { .. } => "navigate",
            Self::Related { .. } => "related",
            Self::Evidence { .. } => "evidence",
            Self::History { .. } => "history",
            Self::Context { .. } => "context",
            Self::Create { .. } => "create",
            Self::Update { .. } => "update",
            Self::Link { .. } => "link",
            Self::AttachEvidence { .. } => "attach_evidence",
            Self::RecordUse { .. } => "record_use",
            Self::Proposals { .. } => "proposals",
            Self::Accept { .. } => "accept",
            Self::Reject { .. } => "reject",
            Self::Status => "status",
            Self::Review { .. } => "review",
            Self::Sources { .. } => "sources",
            Self::Index { .. } => "index",
            Self::Path { .. } => "path",
        }
    }

    pub fn reads_knowledge(&self) -> bool {
        matches!(
            self,
            Self::Search { .. }
                | Self::Open { .. }
                | Self::Navigate { .. }
                | Self::Related { .. }
                | Self::Evidence { .. }
                | Self::History { .. }
                | Self::Context { .. }
                | Self::Proposals { .. }
                | Self::Status
                | Self::Review { .. }
                | Self::Sources { .. }
                | Self::Path { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caller {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCreate {
    #[serde(rename = "type")]
    pub node_type: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeUpdate {
    pub node: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkCreate {
    pub from: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub to: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAnchor {
    pub file: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub line_start: Option<u32>,
    #[serde(default)]
    pub line_end: Option<u32>,
    #[serde(default)]
    pub region_hash: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceCreate {
    pub node: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub anchor: Option<SourceAnchor>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Response payloads are additive; the method tag and envelope are the stable part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub method: String,
    #[serde(flatten)]
    pub data: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeResponseEnvelope {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub outcome: BridgeOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BridgeOutcome {
    Ok { response: BridgeResponse },
    Error { error: ProtocolError },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl BridgeResponseEnvelope {
    pub fn ok(request_id: Option<String>, response: BridgeResponse) -> Self {
        Self {
            protocol_version: BRIDGE_PROTOCOL_VERSION,
            request_id,
            outcome: BridgeOutcome::Ok { response },
        }
    }

    pub fn error(
        request_id: Option<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            protocol_version: BRIDGE_PROTOCOL_VERSION,
            request_id,
            outcome: BridgeOutcome::Error {
                error: ProtocolError {
                    code: code.into(),
                    message: message.into(),
                    details,
                },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub protocol_version: u32,
    pub operations: Vec<String>,
    pub limits: BridgeLimits,
    pub node_types: Vec<String>,
    pub edge_types: Vec<String>,
    pub authorities: Vec<String>,
    pub scopes: Vec<String>,
    pub evidence_kinds: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLimits {
    pub max_search_results: u32,
    pub max_context_seeds: usize,
    pub max_context_depth: u8,
    pub max_context_nodes: usize,
}
