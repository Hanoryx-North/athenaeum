//! Knowledge nodes — the semantic objects the whole system is built around.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{Authority, NodeStatus, NodeType, ProvenanceSource, Scope};
use crate::id::{NodeId, SessionId};

/// Where a claim came from, carried alongside the claim itself.
///
/// Buildplan §20: an edge a model guessed and an edge derived from an
/// `import` statement must never become indistinguishable once they are in
/// the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: ProvenanceSource,
    /// Free text identifying the specific origin — a model name, a pack
    /// identifier, "user typed it in the node editor".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The agent session during which this was learned, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
}

impl Provenance {
    pub fn new(source: ProvenanceSource) -> Self {
        Self {
            source,
            detail: None,
            session: None,
        }
    }

    /// Stated by the user directly. The most trustworthy origin there is.
    pub fn user() -> Self {
        Self::new(ProvenanceSource::User)
    }

    /// Extracted mechanically — imports, AST walks, git log. No model.
    pub fn deterministic(detail: impl Into<String>) -> Self {
        Self::new(ProvenanceSource::Deterministic).with_detail(detail)
    }

    /// Proposed by a model. Claims carrying this can still be useful, but
    /// they start life as proposals, not facts.
    pub fn model(model_name: impl Into<String>) -> Self {
        Self::new(ProvenanceSource::Model).with_detail(model_name)
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }
}

impl Default for Provenance {
    fn default() -> Self {
        Self::new(ProvenanceSource::default())
    }
}

/// A single meaningful piece of knowledge.
///
/// Not a chunk of text (buildplan §9). `AuthService` is a node; "bytes
/// 4000–4500 of AuthService.ts" is not. The source file stays the source of
/// truth for implementation detail — this stores the *meaning*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeNode {
    pub id: NodeId,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Short, unique-ish, human-facing. This is what the wiki agent matches
    /// against when it looks for seed nodes.
    pub title: String,
    /// One or two sentences. The retrieval layer shows summaries at L1 and
    /// only pulls full content when the agent asks for it (§28).
    pub summary: String,
    /// The full body. Markdown by convention; the store does not parse it.
    pub content: String,
    pub scope: Scope,
    pub authority: Authority,
    pub status: NodeStatus,
    pub provenance: Provenance,
    /// When this claim was last confirmed against its evidence. Distinct
    /// from `updated_at`: editing a node's wording is not verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Monotonic, starting at 1. Every committed edit archives the previous
    /// body into `node_versions` and bumps this (§36).
    pub version: u32,
}

impl KnowledgeNode {
    /// A new node with sane, deliberately cautious defaults: project scope,
    /// unverified authority, valid status, model provenance.
    pub fn new(node_type: NodeType, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: NodeId::new(),
            node_type,
            title: title.into(),
            summary: String::new(),
            content: String::new(),
            scope: Scope::default(),
            authority: Authority::default(),
            status: NodeStatus::default(),
            provenance: Provenance::default(),
            last_verified_at: None,
            created_at: now,
            updated_at: now,
            version: 1,
        }
    }

    pub fn with_id(mut self, id: impl Into<NodeId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = authority;
        self
    }

    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// True when this node should still be offered to an agent as current
    /// knowledge. Superseded and archived nodes stay in the database for
    /// history (§17 — "do not simply delete old knowledge") but must not be
    /// handed to the primary agent as if they were live.
    pub fn is_live(&self) -> bool {
        matches!(
            self.status,
            NodeStatus::Valid | NodeStatus::Stale | NodeStatus::StaleRequiresReview
        )
    }

    /// True when the claim needs a human or a verifier model to look at it.
    pub fn needs_review(&self) -> bool {
        matches!(
            self.status,
            NodeStatus::StaleRequiresReview | NodeStatus::Contradicted
        )
    }

    /// The text the full-text index should see. Title is repeated because a
    /// match on the title is a far stronger signal than one in the body.
    pub(crate) fn search_text(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}",
            self.title, self.title, self.summary, self.content
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_nodes_are_unverified_by_default() {
        let n = KnowledgeNode::new(NodeType::Fact, "Sessions expire after 24h");
        assert_eq!(n.authority, Authority::UnverifiedObservation);
        assert_eq!(n.version, 1);
        assert!(n.is_live());
        assert!(!n.needs_review());
    }

    #[test]
    fn superseded_nodes_are_retained_but_not_live() {
        let n = KnowledgeNode::new(NodeType::Decision, "Use sessions")
            .with_status(NodeStatus::Superseded);
        assert!(!n.is_live());
    }

    #[test]
    fn stale_nodes_stay_live_because_staleness_is_not_falsehood() {
        // §17: a node can be high-confidence and stale at the same time.
        let n = KnowledgeNode::new(NodeType::Fact, "OAuth validates state")
            .with_status(NodeStatus::Stale)
            .with_authority(Authority::ProjectDecision);
        assert!(n.is_live());
    }

    #[test]
    fn search_text_weights_the_title() {
        let n = KnowledgeNode::new(NodeType::Concept, "OAuth").with_content("body");
        assert_eq!(n.search_text().matches("OAuth").count(), 2);
    }
}
