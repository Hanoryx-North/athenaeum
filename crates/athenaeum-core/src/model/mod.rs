//! The knowledge model.
//!
//! Deliberately free of storage concerns: everything here is a plain value
//! type that can be constructed, tested and serialized without a database.
//! Persistence lives in [`crate::store`].

pub mod anchor;
pub mod applicability;
pub mod edge;
pub mod enums;
pub mod evidence;
pub mod node;
pub mod version;

pub use anchor::{hash, AnchorResolution, SourceAnchor};
pub use applicability::{Applicability, RecallSelection};
pub use edge::{KnowledgeEdge, Neighbor};
pub use enums::{
    Authority, EdgeType, EvidenceKind, NodeStatus, NodeType, ProvenanceSource, Scope, UnknownValue,
};
pub use evidence::{Evidence, INLINE_EXCERPT_LIMIT};
pub use node::{KnowledgeNode, Provenance};
pub use version::NodeVersion;

/// A stored confidence as the wire carries it.
///
/// Confidence is kept as `f32` in the model because it is a rough judgement, not
/// a measurement. Widening it to `f64` for JSON would publish the rounding error
/// instead — `0.8` becoming `0.800000011920929` — so the value is rounded to
/// three decimals, which is finer than any judgement here and stable across
/// runs and platforms.
pub fn wire_confidence(value: f32) -> f64 {
    (f64::from(value) * 1_000.0).round() / 1_000.0
}
