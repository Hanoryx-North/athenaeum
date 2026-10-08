//! The append-only event journal (buildplan §42).
//!
//! Usage statistics and history are kept *here*, not on the node. A node's
//! row is canonical knowledge; how often it was retrieved is not knowledge
//! and must not be allowed to rewrite the node every time someone reads it
//! (§41: "keep mutable usage statistics outside canonical node content").

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::SessionId;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    NodeCreated,
    NodeUpdated,
    NodeDeleted,
    /// Returned by a search or traversal.
    NodeRetrieved,
    /// Actually used by an agent to do something.
    NodeUsed,
    /// Cited in a knowledge brief or an answer.
    NodeCited,
    NodeVerified,
    NodeRejected,
    /// A proposal became a page of the vault, or a user-stated claim was written to one.
    NodeAccepted,
    EdgeCreated,
    /// A proposed edge was accepted and became traversable.
    EdgePromoted,
    EdgeRemoved,
    EvidenceAttached,
    /// A source anchor was moved to a newly identified location.
    EvidenceRelocated,
    KnowledgeSuperseded,
    KnowledgeContradicted,
    KnowledgeStale,
}

impl EventKind {
    /// Every kind, in declaration order. Also what [`EventKind::from_str`]
    /// searches, so each on-disk spelling exists in exactly one place — the
    /// same guarantee `str_enum!` gives the model vocabularies. A rename that
    /// reached only one of two tables would make written events unreadable.
    pub const ALL: &'static [EventKind] = &[
        EventKind::NodeCreated,
        EventKind::NodeUpdated,
        EventKind::NodeDeleted,
        EventKind::NodeRetrieved,
        EventKind::NodeUsed,
        EventKind::NodeCited,
        EventKind::NodeVerified,
        EventKind::NodeRejected,
        EventKind::NodeAccepted,
        EventKind::EdgeCreated,
        EventKind::EdgePromoted,
        EventKind::EdgeRemoved,
        EventKind::EvidenceAttached,
        EventKind::EvidenceRelocated,
        EventKind::KnowledgeSuperseded,
        EventKind::KnowledgeContradicted,
        EventKind::KnowledgeStale,
    ];

    pub fn as_str(&self) -> &'static str {
        use EventKind::*;
        match self {
            NodeCreated => "node_created",
            NodeUpdated => "node_updated",
            NodeDeleted => "node_deleted",
            NodeRetrieved => "node_retrieved",
            NodeUsed => "node_used",
            NodeCited => "node_cited",
            NodeVerified => "node_verified",
            NodeRejected => "node_rejected",
            NodeAccepted => "node_accepted",
            EdgeCreated => "edge_created",
            EdgePromoted => "edge_promoted",
            EdgeRemoved => "edge_removed",
            EvidenceAttached => "evidence_attached",
            EvidenceRelocated => "evidence_relocated",
            KnowledgeSuperseded => "knowledge_superseded",
            KnowledgeContradicted => "knowledge_contradicted",
            KnowledgeStale => "knowledge_stale",
        }
    }

    /// True for events that describe *use* rather than *change*.
    ///
    /// Utility scoring (§41) reads these; a graph rebuild can safely ignore
    /// them, because they say nothing about what is true.
    pub fn is_usage(&self) -> bool {
        matches!(
            self,
            EventKind::NodeRetrieved | EventKind::NodeUsed | EventKind::NodeCited
        )
    }
}

impl std::str::FromStr for EventKind {
    type Err = crate::model::enums::UnknownValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EventKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| crate::model::enums::UnknownValue {
                vocabulary: "EventKind",
                value: s.to_string(),
            })
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// One journal entry. Written once, never modified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Monotonic sequence number assigned by the database.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub kind: EventKind,
    /// The node or edge id this concerns, when it concerns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    /// Free-form structured payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

/// An event on its way to being written, before the database assigns a seq.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub kind: EventKind,
    pub subject: Option<String>,
    pub session: Option<SessionId>,
    pub detail: Option<Value>,
}

impl NewEvent {
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            subject: None,
            session: None,
            detail: None,
        }
    }

    pub fn about(kind: EventKind, subject: impl std::fmt::Display) -> Self {
        Self::new(kind).with_subject(subject)
    }

    pub fn with_subject(mut self, subject: impl std::fmt::Display) -> Self {
        self.subject = Some(subject.to_string());
        self
    }

    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// How useful a node has actually been, derived entirely from the journal.
///
/// Buildplan §41: measure whether nodes earn their place, rather than
/// assuming that more nodes is better.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeUtility {
    pub retrievals: u32,
    pub uses: u32,
    pub citations: u32,
    pub verifications: u32,
    pub rejections: u32,
}

impl NodeUtility {
    /// Fraction of retrievals that led to the node actually being used.
    ///
    /// A node retrieved constantly and used never is noise: it is matching
    /// searches without answering them.
    pub fn use_rate(&self) -> Option<f32> {
        if self.retrievals == 0 {
            return None;
        }
        Some(self.uses as f32 / self.retrievals as f32)
    }

    /// True when this node keeps surfacing without ever helping.
    pub fn looks_like_noise(&self) -> bool {
        self.retrievals >= 5 && self.uses == 0
    }
}
