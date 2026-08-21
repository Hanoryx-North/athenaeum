//! The knowledge model.
//!
//! Deliberately free of storage concerns: everything here is a plain value
//! type that can be constructed, tested and serialized without a database.
//! Persistence lives in [`crate::store`].

pub mod anchor;
pub mod edge;
pub mod enums;
pub mod evidence;
pub mod node;
pub mod version;

pub use anchor::{hash, AnchorResolution, SourceAnchor};
pub use edge::{KnowledgeEdge, Neighbor};
pub use enums::{Authority, EdgeType, EvidenceKind, NodeStatus, NodeType, ProvenanceSource, Scope};
pub use evidence::{Evidence, INLINE_EXCERPT_LIMIT};
pub use node::{KnowledgeNode, Provenance};
pub use version::NodeVersion;
