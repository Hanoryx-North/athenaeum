//! # Athenaeum core — the knowledge library
//!
//! A local, persistent, navigable network of project knowledge: typed nodes,
//! typed relationships, evidence, provenance and history, stored in one
//! SQLite file the user owns.
//!
//! This crate is the product. The desktop shell and the CLI in this workspace
//! are consumers of it, and it depends on neither — no UI, no HTTP client, no
//! model provider. That separation is deliberate:
//!
//! * **It is the same everywhere.** Windows x86_64, macOS arm64 and Linux run
//!   byte-identical logic over a statically linked SQLite, so a knowledge base
//!   created on one opens unchanged on another.
//! * **It outlives the model.** Switching from one provider to another does
//!   not touch anything in here (§62).
//! * **It is testable without a network.** Every behaviour below is exercised
//!   by tests that never make an API call.
//!
//! ## The shape of it
//!
//! ```text
//! KnowledgeNode  ──edges──▶  KnowledgeNode
//!      │                          │
//!   Evidence                   Evidence
//!      │                          │
//!  SourceAnchor               SourceAnchor
//!      │
//!  the actual file, which remains the source of truth
//! ```
//!
//! Nodes hold *meaning*; source files keep holding the implementation (§9).
//!
//! ## Example
//!
//! ```
//! use athenaeum_core::prelude::*;
//!
//! let mut store = KnowledgeStore::open_in_memory()?;
//!
//! let service = store.create_node(
//!     KnowledgeNode::new(NodeType::Concept, "AuthService")
//!         .with_summary("Validates tokens, then creates sessions.")
//!         .with_authority(Authority::VerifiedEvidence),
//! )?;
//!
//! let tests = store.create_node(
//!     KnowledgeNode::new(NodeType::Test, "auth.test.ts")
//!         .with_summary("Covers the token validation path."),
//! )?;
//!
//! store.connect(KnowledgeEdge::deterministic(
//!     service.id.clone(),
//!     EdgeType::TestedBy,
//!     tests.id.clone(),
//!     "test file references symbol",
//! ))?;
//!
//! // Arriving at the test, the relationship reads the other way round.
//! let back = store.neighbors(&tests.id, &EdgeFilter::traversable())?;
//! assert_eq!(back[0].edge_type, EdgeType::Tests);
//! assert_eq!(back[0].node, service.id);
//! # Ok::<(), athenaeum_core::Error>(())
//! ```

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

pub mod anchors;
pub mod authority;
pub mod blobs;
pub mod brief;
pub mod error;
pub mod graph;
pub mod id;
pub mod journal;
pub mod model;
pub mod repository;
pub mod store;
pub mod wiki;
pub mod workspace;

pub use error::{Error, Result};

/// Everything needed for ordinary use, in one import.
pub mod prelude {
    pub use crate::anchors::{
        AnchorRelocation, AnchorRelocationReport, AnchorRelocator, EvidenceRelocation,
    };
    pub use crate::blobs::BlobStore;
    pub use crate::brief::KnowledgeBrief;
    pub use crate::error::{Error, Result};
    pub use crate::graph::{navigate, shortest_path, NavigationOptions, Reached};
    pub use crate::id::{EdgeId, EvidenceId, NodeId, SessionId};
    pub use crate::journal::{EventKind, NewEvent, NodeUtility};
    pub use crate::model::{
        Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, Neighbor,
        NodeStatus, NodeType, Provenance, ProvenanceSource, Scope, SourceAnchor,
    };
    pub use crate::repository::{
        RepositoryIndexReport, RepositoryIndexer, RepositoryScanOptions, RepositorySnapshot,
    };
    pub use crate::store::{EdgeFilter, EventFilter, KnowledgeStore, NodeFilter, SearchHit, Stats};
    pub use crate::wiki::{WikiRequest, WikiResponse, WikiService, WIKI_PROTOCOL_VERSION};
    pub use crate::workspace::Workspace;
}

/// The version of this library, from Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
