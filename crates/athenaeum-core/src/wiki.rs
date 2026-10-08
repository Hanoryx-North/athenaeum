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
use crate::graph::{self, NavigationOptions, Reached};
use crate::id::{EdgeId, NodeId, SessionId};
use crate::journal::NodeUtility;
use crate::model::{
    Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, Neighbor, NodeType,
    NodeVersion, Provenance, Scope, SourceAnchor,
};
use crate::portable::memory::{self, Accepted};
use crate::store::{EdgeFilter, KnowledgeStore, SearchHit};
pub use crate::vault::PageRef as WikiPage;

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
const FEATURES: &[&str] = &["page_pointers", "vault_sync", "memory_lifecycle"];

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
}

impl WikiRequest {
    /// True for the requests that only look at knowledge: search, open, navigate, related,
    /// evidence, history and context. These are the ones a stale index would answer wrongly, and
    /// the ones a runtime can issue on every turn, which is why a transport refreshes the index
    /// for them and not for a mutation.
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
            } => Ok(WikiResponse::Context {
                context: self.context(&task, &seeds, depth, max_nodes)?,
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
        let seeds = if seed_references.is_empty() {
            self.store
                .search(task, DEFAULT_CONTEXT_SEEDS)?
                .into_iter()
                .map(|hit| hit.node.id)
                .collect()
        } else {
            seed_references
                .iter()
                .take(MAX_CONTEXT_SEEDS)
                .map(|reference| self.resolve(reference).map(|node| node.id))
                .collect::<Result<Vec<_>>>()?
        };
        let brief = KnowledgeBrief::assemble(
            self.store,
            task,
            &seeds,
            &bounded_navigation(depth, max_nodes),
        )?;
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
        let mut edge = KnowledgeEdge::new(from.id, input.edge_type, to.id)
            .with_provenance(model_provenance(caller)?);
        if let Some(note) = input.note {
            edge = edge.with_note(note);
        }
        if let Some(confidence) = input.confidence {
            edge = edge.with_confidence(confidence);
        }
        self.store.connect(edge)
    }

    pub fn attach_evidence(
        &mut self,
        input: WikiEvidenceCreate,
        caller: WikiCaller,
    ) -> Result<Evidence> {
        let node = self.resolve(&input.node)?;
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
        Ok(())
    }
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
        edge_id: neighbor.edge.id.clone(),
        outbound: neighbor.outbound,
        established: neighbor.edge.is_traversable(),
    })
}

fn spellings<T>(all: &[T], as_str: fn(&T) -> &'static str) -> Vec<String> {
    all.iter().map(|value| as_str(value).to_string()).collect()
}
