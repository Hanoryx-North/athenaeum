//! Stable integration boundary for an external coding-agent runtime.
//!
//! The agent asks this service for knowledge; it never opens SQLite or calls
//! store internals. The request and response types are serializable so the
//! same boundary works in-process, over Tauri IPC, or through the CLI's
//! newline-delimited JSON bridge. This crate still has no model, network, or
//! agent-runtime dependency.

use serde::{Deserialize, Serialize};

use crate::brief::KnowledgeBrief;
use crate::error::{Error, Result};
use crate::findings::{self, ReviewFindingView};
use crate::graph::{self, NavigationOptions, Reached, Step};
use crate::id::{EdgeId, NodeId, SessionId};
use crate::journal::NodeUtility;
use crate::model::{
    Applicability, Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode,
    Neighbor, NodeType, NodeVersion, Provenance, RecallSelection, Scope, SourceAnchor,
};
use crate::portable::memory::{self, Accepted};
use crate::status::{self, IndexOutcome, IndexScope, SourceSummary, WorkspaceStatus};
use crate::store::{EdgeFilter, KnowledgeStore, NodeFilter, SearchHit};
pub use crate::vault::PageRef as WikiPage;
use crate::workspace::Workspace;

/// Version of the cross-repository JSON contract.
///
/// Increment this only for a breaking wire-format or semantic change. Additive
/// fields remain compatible because consumers are expected to ignore fields
/// they do not understand.
pub const WIKI_PROTOCOL_VERSION: u32 = 1;

/// Optional behaviours advertised in `capabilities.features`.
///
/// * `page_pointers` — a node that is a page of the vault carries a `page` with its file path, in
///   `open` and in a brief. Absence of the feature means an older library; absence of `page` on a
///   node, with the feature present, means the node is not a page.
/// * `memory_lifecycle` — `proposals`, `accept` and `reject`: a model's proposal becomes durable
///   only when accepted, and accepting writes it to the vault as a page.
/// * `vault_sync` — the bridge compares the vault with the index before it answers a read and
///   reconciles when they differ, so a page an agent just edited is what the next question sees.
///   It keeps an index that was built; it does not build the first one.
const FEATURES: &[&str] = &[
    "page_pointers",
    "applicability",
    "vault_sync",
    "memory_lifecycle",
    // `status`, `sources` and `index` report on the engine's own state: how
    // fresh the derived index is, what it has read, and how to rebuild it.
    "engine_status",
    // `review` reports every kind of finding, not only the ones the older
    // review sweep knew about.
    "finding_report",
    // `path` answers "how are these connected?" from the graph itself.
    "graph_path",
];

const DEFAULT_SEARCH_LIMIT: u32 = 20;
const MAX_SEARCH_LIMIT: u32 = 100;
const DEFAULT_CONTEXT_SEEDS: u32 = 5;
const MAX_CONTEXT_SEEDS: usize = 10;
const MAX_CONTEXT_DEPTH: u8 = 4;
const MAX_CONTEXT_NODES: usize = 100;

/// A versioned request from an external Wiki provider adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiRequestEnvelope {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub request: WikiRequest,
}

/// The operations Repository 1 offers to an agent runtime.
///
/// Read operations are sufficient for a primary coding agent. Mutations are
/// deliberately model-attributed and conservative: model-created nodes keep
/// unverified authority, and model-created edges remain proposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WikiRequest {
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
        /// Include model proposals awaiting review.
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
        /// IDs or exact titles. Omit to derive seeds through search.
        #[serde(default)]
        seeds: Vec<String>,
        #[serde(default)]
        depth: Option<u8>,
        #[serde(default)]
        max_nodes: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection: Option<RecallSelection>,
    },
    Create {
        input: WikiNodeCreate,
        caller: WikiCaller,
    },
    Update {
        input: WikiNodeUpdate,
        caller: WikiCaller,
    },
    Link {
        input: WikiLinkCreate,
        caller: WikiCaller,
    },
    AttachEvidence {
        input: WikiEvidenceCreate,
        caller: WikiCaller,
    },
    RecordUse {
        nodes: Vec<String>,
        #[serde(default)]
        session: Option<SessionId>,
    },
    /// Model proposals nobody has accepted or rejected.
    Proposals {
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Make a proposal permanent by writing it to the vault as a page. The caller is whoever
    /// decided: the person, or the policy acting for them.
    Accept {
        node: String,
        caller: WikiCaller,
    },
    /// Decline a proposal so it is not offered again. Writes no file.
    Reject {
        node: String,
        #[serde(default)]
        reason: Option<String>,
        caller: WikiCaller,
    },
    /// What this workspace is, how much knowledge it holds, and whether the
    /// derived index is behind the files it describes.
    ///
    /// Answered even when there is no workspace at all: a directory with no
    /// vault is a state to render, not a transport error.
    Status,
    /// Everything in the knowledge base that needs a decision, in the shape a
    /// review queue renders. Reads only; decides nothing.
    Review {
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Every file the last index read, with how much of the graph points at it.
    Sources {
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Rebuild the derived index for one scope.
    ///
    /// An explicit act, not a read: `paths` narrows a repository pass, and a
    /// vault pass always covers the whole vault.
    Index {
        scope: IndexScope,
        #[serde(default)]
        paths: Vec<String>,
    },
    /// How two nodes are connected: the shortest chain of relationships
    /// between them, bounded like every other traversal here.
    Path {
        from: String,
        to: String,
        #[serde(default)]
        depth: Option<u8>,
        #[serde(default)]
        max_nodes: Option<usize>,
    },
}

impl WikiRequest {
    /// The fields a request of this method may carry.
    ///
    /// Serde cannot express this: an internally-tagged enum consumes the tag
    /// and ignores everything else it does not recognise, so an unknown field
    /// would be accepted in silence. A versioned contract is exactly where a
    /// misspelled key should be an error — the caller meant something by it, and
    /// answering as if they had not said it is how two clients drift apart.
    pub fn allowed_fields(method: &str) -> Option<&'static [&'static str]> {
        Some(match method {
            "capabilities" => &["method"],
            "search" => &["method", "query", "limit"],
            "open" | "evidence" | "history" => &["method", "node"],
            "navigate" => &["method", "node", "depth", "maxNodes"],
            "related" => &["method", "node", "includeProposed"],
            "context" => &["method", "task", "seeds", "depth", "maxNodes", "selection"],
            "create" | "update" | "link" | "attach_evidence" => &["method", "input", "caller"],
            "record_use" => &["method", "nodes", "session"],
            "proposals" | "review" | "sources" => &["method", "limit"],
            "accept" => &["method", "node", "caller"],
            "reject" => &["method", "node", "reason", "caller"],
            "status" => &["method"],
            "index" => &["method", "scope", "paths"],
            "path" => &["method", "from", "to", "depth", "maxNodes"],
            _ => return None,
        })
    }

    /// True for the requests that only look at knowledge: search, open, navigate, related,
    /// evidence, history, context and path. These are the ones a stale index would answer wrongly,
    /// and the ones a runtime can issue on every turn, which is why a transport refreshes the index
    /// for them and not for a mutation.
    ///
    /// `status`, `review` and `sources` are deliberately absent. They report how far behind the
    /// index is — `indexStale`, per-source `stale`, and an `incomplete_scan` finding — and
    /// reconciling the vault first would erase the very thing they were asked for.
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
                | Self::Path { .. }
        )
    }
}

/// Identity attached to knowledge proposed by a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiCaller {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiNodeCreate {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability: Option<Applicability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiNodeUpdate {
    /// ID or exact title.
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
pub struct WikiLinkCreate {
    /// IDs or exact titles.
    pub from: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub to: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiEvidenceCreate {
    /// ID or exact title.
    pub node: String,
    #[serde(default)]
    pub kind: EvidenceKind,
    #[serde(default)]
    pub anchor: Option<SourceAnchor>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// A response whose variant mirrors the request method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum WikiResponse {
    Capabilities { capabilities: WikiCapabilities },
    Search { hits: Vec<SearchHit> },
    Open { detail: WikiNodeDetail },
    Navigate { nodes: Vec<Reached> },
    Related { relationships: Vec<WikiRelatedNode> },
    Evidence { evidence: Vec<Evidence> },
    History { versions: Vec<NodeVersion> },
    Context { context: WikiContext },
    Create { node: KnowledgeNode },
    Update { node: KnowledgeNode },
    Link { edge: KnowledgeEdge },
    AttachEvidence { evidence: Evidence },
    RecordUse { nodes: Vec<NodeId> },
    Proposals { proposals: Vec<WikiProposal> },
    Accept { accepted: Box<Accepted> },
    Reject { node: KnowledgeNode },
    Status { status: WorkspaceStatus },
    Review { findings: Vec<ReviewFindingView> },
    Sources { sources: Vec<SourceSummary> },
    Index { index: IndexOutcome },
    Path { path: WikiPath },
}

/// The shortest chain of relationships connecting two nodes.
///
/// `steps` is the traversal as it happened — each step leaves a node along one
/// relationship, in the direction travelled — and `nodes` is the same path in
/// full, so a client can render titles without asking again. Both are bounded
/// by the same budget as [`WikiRequest::Navigate`]. When there is no path
/// within budget, `found` is false and both lists are empty: that is an answer,
/// not a failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiPath {
    /// The resolved node the walk started from.
    pub from: NodeId,
    /// The resolved node it was looking for.
    pub to: NodeId,
    pub found: bool,
    pub steps: Vec<Step>,
    pub nodes: Vec<KnowledgeNode>,
}

/// A proposal awaiting a decision, with what a reviewer needs to make it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiProposal {
    pub node: KnowledgeNode,
    pub relationships: Vec<WikiRelatedNode>,
    pub evidence: Vec<Evidence>,
}

/// Successful or failed response envelope used by the process bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiResponseEnvelope {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub outcome: WikiOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WikiOutcome {
    Ok { response: Box<WikiResponse> },
    Error { error: WikiProtocolError },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiProtocolError {
    pub code: String,
    pub message: String,
    /// Additive, machine-readable context for errors that need recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl WikiResponseEnvelope {
    pub fn success(request_id: Option<String>, response: WikiResponse) -> Self {
        Self {
            protocol_version: WIKI_PROTOCOL_VERSION,
            request_id,
            outcome: WikiOutcome::Ok {
                response: Box::new(response),
            },
        }
    }

    pub fn error(
        request_id: Option<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: WIKI_PROTOCOL_VERSION,
            request_id,
            outcome: WikiOutcome::Error {
                error: WikiProtocolError {
                    code: code.into(),
                    message: message.into(),
                    details: None,
                },
            },
        }
    }

    pub fn error_with_details(
        request_id: Option<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        let mut response = Self::error(request_id, code, message);
        if let WikiOutcome::Error { error } = &mut response.outcome {
            error.details = Some(details);
        }
        response
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiCapabilities {
    pub protocol_version: u32,
    pub operations: Vec<String>,
    pub limits: WikiLimits,
    pub node_types: Vec<String>,
    pub edge_types: Vec<String>,
    pub authorities: Vec<String>,
    pub scopes: Vec<String>,
    pub evidence_kinds: Vec<String>,
    /// Optional behaviours this build has, beyond the operations. Additive: a build that predates
    /// the list reports none, which a consumer reads as "do not assume any of them".
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiLimits {
    pub max_search_results: u32,
    pub max_context_seeds: usize,
    pub max_context_depth: u8,
    pub max_context_nodes: usize,
}

/// Everything needed for a node-detail view or `wiki.open` tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiNodeDetail {
    pub node: KnowledgeNode,
    pub relationships: Vec<WikiRelatedNode>,
    pub evidence: Vec<Evidence>,
    pub history: Vec<NodeVersion>,
    pub utility: NodeUtility,
    /// The file behind the node when it is a page of the vault. Absent for every other node.
    ///
    /// It is an addition to what a node carries, never a substitute for it: the page holds the
    /// prose, and the relationships, evidence, history and utility above are not in the markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<WikiPage>,
}

/// A neighboring node with its title resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiRelatedNode {
    pub id: NodeId,
    pub title: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub edge_type: EdgeType,
    pub edge_id: EdgeId,
    pub outbound: bool,
    pub established: bool,
    /// Why this relationship exists, in one line, exactly as the edge records
    /// it. Absent when the producer wrote no reason, which is not the same as
    /// having written an empty one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How sure the producer was, when it said. A deterministic edge read
    /// straight from source carries no measured confidence, and says so by
    /// omitting it rather than by claiming zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// Context returned to an agent, both structured and already trust-wrapped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiContext {
    pub brief: KnowledgeBrief,
    pub prompt: String,
}

/// Storage-independent façade implementing the Wiki side of the integration.
#[derive(Debug)]
pub struct WikiService<'a> {
    store: &'a mut KnowledgeStore,
}

impl<'a> WikiService<'a> {
    pub fn new(store: &'a mut KnowledgeStore) -> Self {
        Self { store }
    }

    pub fn handle(&mut self, request: WikiRequest) -> Result<WikiResponse> {
        match request {
            WikiRequest::Capabilities => Ok(WikiResponse::Capabilities {
                capabilities: Self::capabilities(),
            }),
            WikiRequest::Search { query, limit } => Ok(WikiResponse::Search {
                hits: self.search(&query, limit)?,
            }),
            WikiRequest::Open { node } => Ok(WikiResponse::Open {
                detail: self.open(&node)?,
            }),
            WikiRequest::Navigate {
                node,
                depth,
                max_nodes,
            } => Ok(WikiResponse::Navigate {
                nodes: self.navigate(&node, depth, max_nodes)?,
            }),
            WikiRequest::Related {
                node,
                include_proposed,
            } => Ok(WikiResponse::Related {
                relationships: self.related(&node, include_proposed)?,
            }),
            WikiRequest::Evidence { node } => Ok(WikiResponse::Evidence {
                evidence: self.evidence(&node)?,
            }),
            WikiRequest::History { node } => Ok(WikiResponse::History {
                versions: self.history(&node)?,
            }),
            WikiRequest::Context {
                task,
                seeds,
                depth,
                max_nodes,
                selection,
            } => Ok(WikiResponse::Context {
                context: self.context_selected(
                    &task,
                    &seeds,
                    depth,
                    max_nodes,
                    selection.unwrap_or_default(),
                )?,
            }),
            WikiRequest::Create { input, caller } => Ok(WikiResponse::Create {
                node: self.create(input, caller)?,
            }),
            WikiRequest::Update { input, caller } => Ok(WikiResponse::Update {
                node: self.update(input, caller)?,
            }),
            WikiRequest::Link { input, caller } => Ok(WikiResponse::Link {
                edge: self.link(input, caller)?,
            }),
            WikiRequest::AttachEvidence { input, caller } => Ok(WikiResponse::AttachEvidence {
                evidence: self.attach_evidence(input, caller)?,
            }),
            WikiRequest::RecordUse { nodes, session } => Ok(WikiResponse::RecordUse {
                nodes: self.record_use(&nodes, session)?,
            }),
            WikiRequest::Proposals { limit } => Ok(WikiResponse::Proposals {
                proposals: self.proposals(limit)?,
            }),
            WikiRequest::Accept { node, caller } => Ok(WikiResponse::Accept {
                accepted: Box::new(self.accept(&node, caller)?),
            }),
            WikiRequest::Reject {
                node,
                reason,
                caller,
            } => Ok(WikiResponse::Reject {
                node: self.reject(&node, reason.as_deref(), caller)?,
            }),
            WikiRequest::Status => Ok(WikiResponse::Status {
                status: self.status()?,
            }),
            WikiRequest::Review { limit } => Ok(WikiResponse::Review {
                findings: self.review(limit)?,
            }),
            WikiRequest::Sources { limit } => Ok(WikiResponse::Sources {
                sources: self.sources(limit)?,
            }),
            WikiRequest::Index { scope, paths } => Ok(WikiResponse::Index {
                index: self.index(scope, &paths)?,
            }),
            WikiRequest::Path {
                from,
                to,
                depth,
                max_nodes,
            } => Ok(WikiResponse::Path {
                path: self.path(&from, &to, depth, max_nodes)?,
            }),
        }
    }

    pub fn capabilities() -> WikiCapabilities {
        WikiCapabilities {
            protocol_version: WIKI_PROTOCOL_VERSION,
            operations: [
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
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            limits: WikiLimits {
                max_search_results: MAX_SEARCH_LIMIT,
                max_context_seeds: MAX_CONTEXT_SEEDS,
                max_context_depth: MAX_CONTEXT_DEPTH,
                max_context_nodes: MAX_CONTEXT_NODES,
            },
            node_types: spellings(NodeType::ALL, NodeType::as_str),
            edge_types: spellings(EdgeType::ALL, EdgeType::as_str),
            authorities: spellings(Authority::ALL, Authority::as_str),
            scopes: spellings(Scope::ALL, Scope::as_str),
            evidence_kinds: spellings(EvidenceKind::ALL, EvidenceKind::as_str),
            features: FEATURES.iter().map(|feature| feature.to_string()).collect(),
        }
    }

    pub fn search(&self, query: &str, limit: Option<u32>) -> Result<Vec<SearchHit>> {
        self.store.search(
            query,
            limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT),
        )
    }

    pub fn open(&self, reference: &str) -> Result<WikiNodeDetail> {
        let node = self.resolve(reference)?;
        let evidence = self.store.evidence_for(&node.id)?;
        let page = WikiPage::of(&node, &evidence, self.store.project_root().as_deref());
        Ok(WikiNodeDetail {
            relationships: related_nodes(self.store, &node.id, &EdgeFilter::all())?,
            history: self.store.history(&node.id)?,
            utility: self.store.utility(&node.id)?,
            evidence,
            page,
            node,
        })
    }

    pub fn navigate(
        &self,
        reference: &str,
        depth: Option<u8>,
        max_nodes: Option<usize>,
    ) -> Result<Vec<Reached>> {
        let node = self.resolve(reference)?;
        let options = bounded_navigation(depth, max_nodes);
        graph::navigate(self.store, std::slice::from_ref(&node.id), &options)
    }

    pub fn related(&self, reference: &str, include_proposed: bool) -> Result<Vec<WikiRelatedNode>> {
        let node = self.resolve(reference)?;
        let filter = if include_proposed {
            EdgeFilter::all()
        } else {
            EdgeFilter::traversable()
        };
        related_nodes(self.store, &node.id, &filter)
    }

    pub fn evidence(&self, reference: &str) -> Result<Vec<Evidence>> {
        let node = self.resolve(reference)?;
        self.store.evidence_for(&node.id)
    }

    pub fn history(&self, reference: &str) -> Result<Vec<NodeVersion>> {
        let node = self.resolve(reference)?;
        self.store.history(&node.id)
    }

    pub fn context(
        &mut self,
        task: &str,
        seed_references: &[String],
        depth: Option<u8>,
        max_nodes: Option<usize>,
    ) -> Result<WikiContext> {
        self.context_selected(
            task,
            seed_references,
            depth,
            max_nodes,
            RecallSelection::default(),
        )
    }

    pub fn context_selected(
        &mut self,
        task: &str,
        seed_references: &[String],
        depth: Option<u8>,
        max_nodes: Option<usize>,
        selection: RecallSelection,
    ) -> Result<WikiContext> {
        selection.validate()?;
        let mut seeds = if seed_references.is_empty() {
            let matched = self
                .store
                .search_context_selected(task, DEFAULT_CONTEXT_SEEDS, &selection)?
                .into_iter()
                .map(|hit| hit.node.id)
                .collect::<Vec<_>>();
            let core = self
                .store
                .core_context_seeds(&selection, DEFAULT_CONTEXT_SEEDS)?;
            let mut seeds = Vec::new();
            for index in 0..core.len().max(matched.len()) {
                if let Some(id) = core.get(index) {
                    seeds.push(id.clone());
                }
                if let Some(id) = matched.get(index) {
                    seeds.push(id.clone());
                }
            }
            seeds
        } else {
            seed_references
                .iter()
                .take(MAX_CONTEXT_SEEDS)
                .map(|reference| self.resolve(reference).map(|node| node.id))
                .collect::<Result<Vec<_>>>()?
        };
        let mut seen = std::collections::HashSet::new();
        seeds.retain(|id| seen.insert(id.clone()));
        let mut options = bounded_navigation(depth, max_nodes);
        options.selection = Some(selection);
        options.claims_only = seed_references.is_empty();
        let brief = KnowledgeBrief::assemble(self.store, task, &seeds, &options)?;
        let prompt = brief.to_prompt();
        Ok(WikiContext { brief, prompt })
    }

    pub fn create(&mut self, input: WikiNodeCreate, caller: WikiCaller) -> Result<KnowledgeNode> {
        validate_nonempty(&input.title, "title")?;
        self.ensure_distinct_title(&input.title, None)?;
        let provenance = model_provenance(caller)?;
        self.store.create_node(
            KnowledgeNode::new(input.node_type, input.title)
                .with_summary(input.summary)
                .with_content(input.content)
                .with_scope(input.scope)
                .with_applicability(input.applicability)
                .with_provenance(provenance),
        )
    }

    pub fn update(&mut self, input: WikiNodeUpdate, caller: WikiCaller) -> Result<KnowledgeNode> {
        if input.title.is_none() && input.summary.is_none() && input.content.is_none() {
            return Err(Error::InvalidValue {
                field: "update",
                value: "no changed fields".to_string(),
            });
        }
        if let Some(title) = &input.title {
            validate_nonempty(title, "title")?;
        }
        let mut node = self.resolve(&input.node)?;
        // A page's text lives in the markdown, and the index follows the vault: an edit made here
        // would be discarded by the next reconcile, silently, which is worse than being refused.
        let evidence = self.store.evidence_for(&node.id)?;
        if let Some(page) = WikiPage::of(&node, &evidence, None) {
            return Err(Error::Invalid {
                id: node.id,
                reason: format!(
                    "it is a page of the vault, so its text is the file {}: change that file \
                     instead — the index follows the vault and would discard an edit made here",
                    page.path
                ),
            });
        }
        if let Some(title) = &input.title {
            self.ensure_distinct_title(title, Some(&node.id))?;
        }
        if let Some(title) = input.title {
            node.title = title;
        }
        if let Some(summary) = input.summary {
            node.summary = summary;
        }
        if let Some(content) = input.content {
            node.content = content;
        }
        self.store
            .update_node_attributed(node, input.reason, model_provenance(caller)?)
    }

    pub fn link(&mut self, input: WikiLinkCreate, caller: WikiCaller) -> Result<KnowledgeEdge> {
        let from = self.resolve(&input.from)?;
        let to = self.resolve(&input.to)?;
        let both_pages = self.is_page(&from)? && self.is_page(&to)?;
        let mut edge = KnowledgeEdge::new(from.id, input.edge_type, to.id)
            .with_provenance(model_provenance(caller)?);
        if let Some(note) = input.note {
            edge = edge.with_note(note);
        }
        if let Some(confidence) = input.confidence {
            edge = edge.with_confidence(confidence);
        }
        if both_pages {
            return memory::link_pages(self.store, edge);
        }
        self.store.connect(edge)
    }

    pub fn attach_evidence(
        &mut self,
        input: WikiEvidenceCreate,
        caller: WikiCaller,
    ) -> Result<Evidence> {
        let node = self.resolve(&input.node)?;
        // Same reasoning as `update`: an accepted page cites its sources in its own frontmatter,
        // and evidence attached here would be neither in the file nor kept by a rebuild.
        let existing = self.store.evidence_for(&node.id)?;
        if let Some(page) = WikiPage::of(&node, &existing, None) {
            return Err(Error::Invalid {
                id: node.id,
                reason: format!(
                    "it is a page of the vault, so its sources are cited in the canonical \
                     Markdown of {}: add them to its `sources:` or `evidence:` frontmatter \
                     instead — the index follows the vault and would not keep evidence attached \
                     here",
                    page.path
                ),
            });
        }
        let mut evidence = Evidence::new(node.id, input.kind);
        if let Some(anchor) = input.anchor {
            evidence = evidence.with_anchor(anchor);
        }
        if let Some(excerpt) = input.excerpt {
            evidence = evidence.with_excerpt(excerpt);
        }
        if let Some(note) = input.note {
            evidence = evidence.with_note(note);
        }
        self.store
            .attach_evidence_attributed(evidence, model_provenance(caller)?)
    }

    pub fn record_use(
        &mut self,
        references: &[String],
        session: Option<SessionId>,
    ) -> Result<Vec<NodeId>> {
        let nodes = references
            .iter()
            .map(|reference| self.resolve(reference).map(|node| node.id))
            .collect::<Result<Vec<_>>>()?;
        self.store.record_use(&nodes, session.as_ref())?;
        Ok(nodes)
    }

    pub fn proposals(&self, limit: Option<u32>) -> Result<Vec<WikiProposal>> {
        memory::proposals(
            self.store,
            Some(limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT)),
        )?
        .into_iter()
        .map(|node| {
            Ok(WikiProposal {
                relationships: related_nodes(self.store, &node.id, &EdgeFilter::all())?,
                evidence: self.store.evidence_for(&node.id)?,
                node,
            })
        })
        .collect()
    }

    pub fn accept(&mut self, reference: &str, caller: WikiCaller) -> Result<Accepted> {
        validate_nonempty(&caller.model, "model")?;
        let node = self.resolve(reference)?;
        memory::accept(self.store, &node.id, &caller.model)
    }

    pub fn reject(
        &mut self,
        reference: &str,
        reason: Option<&str>,
        caller: WikiCaller,
    ) -> Result<KnowledgeNode> {
        validate_nonempty(&caller.model, "model")?;
        let node = self.resolve(reference)?;
        memory::reject(self.store, &node.id, reason, &caller.model)
    }

    /// The workspace's state: what it holds, and how fresh the index is.
    pub fn status(&self) -> Result<WorkspaceStatus> {
        status::workspace_status(Some(self.store), self.store.root())
    }

    /// Everything that needs a decision, in the pinned review shape.
    pub fn review(&self, limit: Option<u32>) -> Result<Vec<ReviewFindingView>> {
        findings::review_findings(self.store, limit)
    }

    /// Every file the last index read, with what points at it.
    pub fn sources(&self, limit: Option<u32>) -> Result<Vec<SourceSummary>> {
        status::source_summaries(self.store, limit)
    }

    /// Rebuild the derived index for one scope.
    ///
    /// Deliberately not something a read does: a caller asking for an index is
    /// asking for work, and wants to be told how much of it happened.
    pub fn index(&mut self, scope: IndexScope, paths: &[String]) -> Result<IndexOutcome> {
        let workspace = Workspace::of_store(self.store);
        status::index_scopes(self.store, &workspace, scope, paths)
    }

    /// The shortest chain of relationships between two nodes.
    ///
    /// Reuses [`graph::shortest_path`] rather than walking again: there is one
    /// traversal implementation in this crate and this is a caller of it.
    pub fn path(
        &self,
        from: &str,
        to: &str,
        depth: Option<u8>,
        max_nodes: Option<usize>,
    ) -> Result<WikiPath> {
        let from_node = self.resolve(from)?;
        let to_node = self.resolve(to)?;
        let mut options = bounded_navigation(depth.or(Some(MAX_CONTEXT_DEPTH)), max_nodes);
        // A path of n steps visits n + 1 nodes, so the node budget also bounds
        // the steps: a walk may not return more of a path than it can show.
        options.max_depth = (options.max_depth as usize)
            .min(options.max_nodes.saturating_sub(1))
            .min(u8::MAX as usize) as u8;
        let steps = graph::shortest_path(self.store, &from_node.id, &to_node.id, &options)?
            .unwrap_or_default();
        let found = !steps.is_empty() || from_node.id == to_node.id;
        let mut nodes = Vec::new();
        if found {
            // The chain is `from`, the source of every step after the first,
            // and finally `to`.
            let mut ids = steps
                .iter()
                .map(|step| step.from.clone())
                .collect::<Vec<_>>();
            ids.push(to_node.id.clone());
            for id in ids.into_iter().take(options.max_nodes) {
                if let Some(node) = self.store.find_node(&id)? {
                    nodes.push(node);
                }
            }
        }
        Ok(WikiPath {
            found,
            from: from_node.id,
            to: to_node.id,
            steps,
            nodes,
        })
    }

    fn resolve(&self, reference: &str) -> Result<KnowledgeNode> {
        let id = NodeId::from_existing(reference);
        if let Some(node) = self.store.find_node(&id)? {
            return Ok(node);
        }
        self.store
            .find_node_by_title(reference)?
            .ok_or(Error::NodeNotFound(id))
    }

    fn ensure_distinct_title(&self, title: &str, except: Option<&NodeId>) -> Result<()> {
        if let Some(existing) = self.store.find_node_by_title(title)? {
            if except != Some(&existing.id) {
                return Err(Error::DuplicateTitle {
                    title: title.to_string(),
                    existing: existing.id,
                });
            }
        }

        let existing = self
            .store
            .find_nodes_by_title_key(title)?
            .into_iter()
            .filter(|node| except != Some(&node.id))
            .map(|node| node.id)
            .collect::<Vec<_>>();
        if !existing.is_empty() {
            return Err(Error::NearDuplicateTitle {
                title: title.to_string(),
                existing,
            });
        }

        // The stored title key ignores only spacing, `-` and `_`. A model rephrasing "OAuth
        // callback state validation" as "oauth-callback: state validation!" is still the same
        // knowledge, so compare once more with every non-alphanumeric character ignored.
        let loose = loose_title_key(title);
        if loose.chars().count() >= LOOSE_TITLE_MIN {
            let near = self
                .store
                .list_nodes(&NodeFilter::live())?
                .into_iter()
                .filter(|node| except != Some(&node.id) && loose_title_key(&node.title) == loose)
                .map(|node| node.id)
                .collect::<Vec<_>>();
            if !near.is_empty() {
                return Err(Error::NearDuplicateTitle {
                    title: title.to_string(),
                    existing: near,
                });
            }
        }
        Ok(())
    }

    /// True for a node that is a page of the vault, whose text and sources are its Markdown.
    fn is_page(&self, node: &KnowledgeNode) -> Result<bool> {
        let evidence = self.store.evidence_for(&node.id)?;
        Ok(WikiPage::of(node, &evidence, None).is_some())
    }
}

/// Titles shorter than this (after normalising) are too short to call near-duplicates.
const LOOSE_TITLE_MIN: usize = 5;

fn loose_title_key(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn model_provenance(caller: WikiCaller) -> Result<Provenance> {
    validate_nonempty(&caller.model, "model")?;
    let mut provenance = Provenance::model(caller.model);
    if let Some(session) = caller.session {
        provenance = provenance.with_session(session);
    }
    Ok(provenance)
}

fn validate_nonempty(value: &str, field: &'static str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidValue {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn bounded_navigation(depth: Option<u8>, max_nodes: Option<usize>) -> NavigationOptions {
    NavigationOptions::default()
        .depth(depth.unwrap_or(2).min(MAX_CONTEXT_DEPTH))
        .max_nodes(
            max_nodes
                .unwrap_or(NavigationOptions::default().max_nodes)
                .min(MAX_CONTEXT_NODES),
        )
}

fn related_nodes(
    store: &KnowledgeStore,
    id: &NodeId,
    filter: &EdgeFilter,
) -> Result<Vec<WikiRelatedNode>> {
    store
        .neighbors(id, filter)?
        .into_iter()
        .map(|neighbor| related_node(store, neighbor))
        .collect()
}

fn related_node(store: &KnowledgeStore, neighbor: Neighbor) -> Result<WikiRelatedNode> {
    let node = store.get_node(&neighbor.node)?;
    Ok(WikiRelatedNode {
        id: node.id,
        title: node.title,
        node_type: node.node_type,
        edge_type: neighbor.edge_type,
        note: neighbor.edge.note.clone(),
        confidence: neighbor.edge.confidence.map(crate::model::wire_confidence),
        edge_id: neighbor.edge.id.clone(),
        outbound: neighbor.outbound,
        established: neighbor.edge.is_traversable(),
    })
}

fn spellings<T>(all: &[T], as_str: fn(&T) -> &'static str) -> Vec<String> {
    all.iter().map(|value| as_str(value).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeStatus, ProvenanceSource};

    fn caller() -> WikiCaller {
        WikiCaller {
            model: "test-model".into(),
            session: Some(SessionId::from_existing("sess_test")),
        }
    }

    #[test]
    fn capabilities_are_versioned_and_advertise_budgets() {
        let capabilities = WikiService::capabilities();
        assert_eq!(capabilities.protocol_version, WIKI_PROTOCOL_VERSION);
        assert!(capabilities.operations.contains(&"context".to_string()));
        assert_eq!(capabilities.limits.max_context_nodes, MAX_CONTEXT_NODES);
        assert_eq!(capabilities.node_types.len(), NodeType::ALL.len());
    }

    #[test]
    fn context_is_bounded_and_carries_the_trust_wrapper() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let oauth = store
            .create_node(
                KnowledgeNode::new(NodeType::Concept, "OAuth")
                    .with_summary("Callback authentication"),
            )
            .unwrap();
        let gotcha = store
            .create_node(
                KnowledgeNode::new(NodeType::Gotcha, "Validate state")
                    .with_summary("Validate before creating a session"),
            )
            .unwrap();
        store
            .connect(KnowledgeEdge::deterministic(
                oauth.id,
                EdgeType::HasGotcha,
                gotcha.id,
                "test",
            ))
            .unwrap();

        let context = WikiService::new(&mut store)
            .context("fix oauth callback", &[], Some(u8::MAX), Some(usize::MAX))
            .unwrap();
        assert_eq!(context.brief.nodes.len(), 2);
        assert!(context.prompt.contains("does not grant permissions"));
        assert!(!context.prompt.contains("usize"));
    }

    #[test]
    fn model_mutations_preserve_cautious_defaults_and_session_provenance() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let mut wiki = WikiService::new(&mut store);
        let a = wiki
            .create(
                WikiNodeCreate {
                    node_type: NodeType::Observation,
                    title: "AuthService".into(),
                    summary: "Validates tokens".into(),
                    content: String::new(),
                    scope: Scope::Project,
                    applicability: None,
                },
                caller(),
            )
            .unwrap();
        let b = wiki
            .create(
                WikiNodeCreate {
                    node_type: NodeType::Test,
                    title: "auth.test.ts".into(),
                    summary: String::new(),
                    content: String::new(),
                    scope: Scope::Project,
                    applicability: None,
                },
                caller(),
            )
            .unwrap();
        let edge = wiki
            .link(
                WikiLinkCreate {
                    from: a.id.to_string(),
                    edge_type: EdgeType::TestedBy,
                    to: b.id.to_string(),
                    note: None,
                    confidence: Some(0.8),
                },
                caller(),
            )
            .unwrap();

        assert_eq!(a.authority, Authority::UnverifiedObservation);
        assert_eq!(a.provenance.source, ProvenanceSource::Model);
        assert_eq!(a.provenance.session.unwrap().as_str(), "sess_test");
        assert_eq!(edge.status, NodeStatus::Proposed);
        assert!(!edge.is_traversable());
    }

    #[test]
    fn curator_create_refuses_an_exact_title_duplicate() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(KnowledgeNode::new(NodeType::Concept, "AuthService"))
            .unwrap();

        let error = WikiService::new(&mut store)
            .create(
                WikiNodeCreate {
                    node_type: NodeType::Observation,
                    title: "authservice".into(),
                    summary: String::new(),
                    content: String::new(),
                    scope: Scope::Project,
                    applicability: None,
                },
                caller(),
            )
            .unwrap_err();
        assert!(matches!(error, Error::DuplicateTitle { .. }));
        assert_eq!(store.stats().unwrap().nodes, 1);
    }

    #[test]
    fn curator_create_resolves_conservative_near_duplicates() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let existing = store
            .create_node(KnowledgeNode::new(NodeType::Symbol, "AuthService"))
            .unwrap();

        let error = WikiService::new(&mut store)
            .create(
                WikiNodeCreate {
                    node_type: NodeType::Symbol,
                    title: "auth_service".into(),
                    summary: "New evidence from this session".into(),
                    content: String::new(),
                    scope: Scope::Project,
                    applicability: None,
                },
                caller(),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            Error::NearDuplicateTitle { existing: ids, .. } if ids == vec![existing.id]
        ));
        assert_eq!(store.stats().unwrap().nodes, 1);
    }

    #[test]
    fn curator_update_cannot_rename_a_node_into_a_lookalike() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(KnowledgeNode::new(NodeType::Symbol, "AuthService"))
            .unwrap();
        let other = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "Sessions"))
            .unwrap();

        let error = WikiService::new(&mut store)
            .update(
                WikiNodeUpdate {
                    node: other.id.to_string(),
                    title: Some("auth-service".into()),
                    summary: None,
                    content: None,
                    reason: None,
                },
                caller(),
            )
            .unwrap_err();
        assert!(matches!(error, Error::NearDuplicateTitle { .. }));
    }

    #[test]
    fn open_and_record_use_accept_ids_or_titles() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Decision, "Use SQLite"))
            .unwrap();
        let mut wiki = WikiService::new(&mut store);

        assert_eq!(wiki.open("Use SQLite").unwrap().node.id, node.id);
        wiki.record_use(&[node.id.to_string()], None).unwrap();
        assert_eq!(wiki.open("Use SQLite").unwrap().utility.uses, 1);
    }

    #[test]
    fn model_edits_do_not_replace_the_claims_original_provenance() {
        use crate::store::EventFilter;

        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let original = store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Use SQLite")
                    .with_provenance(Provenance::user()),
            )
            .unwrap();
        let mut wiki = WikiService::new(&mut store);
        let updated = wiki
            .update(
                WikiNodeUpdate {
                    node: original.id.to_string(),
                    title: None,
                    summary: Some("The project database is SQLite".into()),
                    content: None,
                    reason: Some("clarified during implementation".into()),
                },
                caller(),
            )
            .unwrap();

        assert_eq!(updated.provenance.source, ProvenanceSource::User);
        let events = store
            .events(&EventFilter::default().of_kind(crate::journal::EventKind::NodeUpdated))
            .unwrap();
        assert_eq!(events[0].session.as_ref().unwrap().as_str(), "sess_test");
        assert_eq!(
            events[0].detail.as_ref().unwrap()["actorDetail"],
            "test-model"
        );
    }

    #[test]
    fn response_envelopes_have_stable_success_and_error_shapes() {
        let ok = WikiResponseEnvelope::success(
            Some("req-1".into()),
            WikiResponse::Capabilities {
                capabilities: WikiService::capabilities(),
            },
        );
        let json = serde_json::to_value(ok).unwrap();
        assert_eq!(json["protocolVersion"], WIKI_PROTOCOL_VERSION);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["response"]["method"], "capabilities");

        let error = WikiResponseEnvelope::error(None, "invalid_request", "bad JSON");
        let json = serde_json::to_value(error).unwrap();
        assert_eq!(json["status"], "error");
        assert_eq!(json["error"]["code"], "invalid_request");
        assert!(json["error"].get("details").is_none());
    }

    #[test]
    fn request_fields_use_cross_language_camel_case() {
        let request: WikiRequest = serde_json::from_value(serde_json::json!({
            "method": "related",
            "node": "OAuth",
            "includeProposed": true
        }))
        .unwrap();
        assert_eq!(
            request,
            WikiRequest::Related {
                node: "OAuth".into(),
                include_proposed: true,
            }
        );

        let serialized = serde_json::to_value(WikiRequest::Navigate {
            node: "OAuth".into(),
            depth: Some(3),
            max_nodes: Some(40),
        })
        .unwrap();
        assert_eq!(serialized["maxNodes"], 40);
        assert!(serialized.get("max_nodes").is_none());
    }
}
