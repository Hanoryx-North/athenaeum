//! Typed relationships between nodes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{EdgeType, NodeStatus, Scope};
use super::node::Provenance;
use crate::id::{EdgeId, NodeId};

/// A relationship, stored once and traversable in both directions.
///
/// Buildplan §21 is emphatic that edges are first-class objects rather than
/// loose associations: every one carries a type, a provenance, a state and a
/// scope, so that "connect everything to everything" degenerate graphs can be
/// filtered rather than merely regretted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEdge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub provenance: Provenance,
    /// Proposed edges (typically model-suggested) are stored but excluded
    /// from default traversal until something promotes them.
    pub status: NodeStatus,
    pub scope: Scope,
    /// 0.0–1.0 where the producer can express one.
    ///
    /// The rule, in full: a structural relationship read from a syntax tree
    /// carries a confidence reflecting what it is — a file containing a
    /// declaration is 1.0, a call site resolved by name is lower, because a
    /// name match can be a different `open`. The same relationship read by the
    /// line-oriented fallback carries a lower number still, and says so in its
    /// note. A model's proposal carries the model's own number. `None` means the
    /// producer measured nothing: a snapshot that did not record how it was read,
    /// or a producer with no basis for a number. Absence is not zero, and it is
    /// not certainty either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Why this relationship exists, in one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeEdge {
    pub fn new(from: NodeId, edge_type: EdgeType, to: NodeId) -> Self {
        Self {
            id: EdgeId::new(),
            from,
            to,
            edge_type,
            provenance: Provenance::default(),
            // Model-proposed by default; `deterministic()` promotes it.
            status: NodeStatus::Proposed,
            scope: Scope::default(),
            confidence: None,
            note: None,
            created_at: Utc::now(),
        }
    }

    /// An edge read straight out of the code — an import, an `implements`,
    /// a file containing a symbol. These are valid on arrival because no
    /// model judgement was involved (§19).
    pub fn deterministic(
        from: NodeId,
        edge_type: EdgeType,
        to: NodeId,
        detail: impl Into<String>,
    ) -> Self {
        let mut edge = Self::new(from, edge_type, to);
        edge.provenance = Provenance::deterministic(detail);
        edge.status = NodeStatus::Valid;
        edge
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// True when traversal should follow this edge by default.
    pub fn is_traversable(&self) -> bool {
        matches!(self.status, NodeStatus::Valid | NodeStatus::Stale)
    }
}

/// One step out of a node, already oriented in the direction of travel.
///
/// When the walker arrives at a node along an incoming edge, `edge_type` here
/// is the *inverse* of the stored type, so callers never have to remember
/// which end of the row they came in on (§35).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neighbor {
    /// The node on the far end of the relationship.
    pub node: NodeId,
    /// The relationship as seen *from the node you started at*.
    pub edge_type: EdgeType,
    /// The underlying edge, exactly as stored.
    pub edge: KnowledgeEdge,
    /// False when the stored edge points at the starting node rather than
    /// away from it, i.e. this step was taken against the arrow.
    pub outbound: bool,
}
