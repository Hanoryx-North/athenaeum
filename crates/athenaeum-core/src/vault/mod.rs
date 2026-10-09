//! Indexing the vault into the knowledge library.
//!
//! # Which layer holds what
//!
//! The vault — `wiki/*.md`, `raw/sources/`, `purpose.md`, `schema.md` — is
//! the **source of truth**. It is plain markdown on disk, it opens in
//! Obsidian, and the agent edits it with ordinary file tools.
//!
//! Everything this module writes into SQLite is a **derived index** over
//! that markdown. The acceptance test for the whole design is blunt:
//!
//! > Delete `.athenaeum/knowledge.db` and you lose nothing but time.
//!
//! It is enforced by [`tests::a_deleted_database_costs_only_time`]. If some
//! future change makes that test fail, the fix is never to weaken the test:
//! it is to move whatever was lost back into the vault, where it belongs.
//!
//! # What the index adds
//!
//! Markdown cannot answer quickly — or at all — the questions the knowledge
//! library exists for. The index supplies them:
//!
//! | From the vault | Becomes |
//! | --- | --- |
//! | a wiki page | a [`KnowledgeNode`], typed by frontmatter or by directory |
//! | a `[[wikilink]]` in the body | a deterministic [`EdgeType::References`] edge |
//! | a `related:` entry | a deterministic [`EdgeType::RelatedTo`] edge |
//! | a `sources:` entry | [`Evidence`] anchored at the raw file, plus a [`EdgeType::DerivedFrom`] edge |
//! | a file under `raw/sources/` | a [`NodeType::Source`] node |
//!
//! No model is involved in any of it. Every edge written here is read
//! straight out of the markdown, which is why they are stored as
//! [`ProvenanceSource::Deterministic`] and are traversable on arrival
//! rather than sitting in the proposal queue a model's suggestions land in.
//!
//! # Reconciling, not rebuilding
//!
//! Edits arrive from three directions at once — the app's editor, Obsidian,
//! and the agent — and all three are legitimate. So indexing is a diff:
//! [`VaultIndexer::index`] computes the state the vault implies, compares it
//! with what the store already holds, and writes only the difference.
//! Re-indexing an unchanged vault performs no writes at all.
//!
//! The indexer touches only what it created. Nodes, edges and evidence
//! whose provenance is not one of this module's own markers are left
//! strictly alone, so a node the user typed into the knowledge panel or one
//! the agent proposed over the bridge survives every re-index.
//!
//! [`KnowledgeNode`]: crate::model::KnowledgeNode
//! [`Evidence`]: crate::model::Evidence
//! [`EdgeType::References`]: crate::model::EdgeType::References
//! [`EdgeType::RelatedTo`]: crate::model::EdgeType::RelatedTo
//! [`EdgeType::DerivedFrom`]: crate::model::EdgeType::DerivedFrom
//! [`NodeType::Source`]: crate::model::NodeType::Source
//! [`ProvenanceSource::Deterministic`]: crate::model::ProvenanceSource::Deterministic

pub mod frontmatter;
pub mod graph;
mod index;
pub mod lint;
mod page;
mod pointer;
mod scan;
mod wikilink;

pub use frontmatter::{set_fields, FieldValue, Frontmatter, FrontmatterValue};
pub use graph::{GraphEdge, GraphNode, GraphOrigin, KnowledgeGraph};
pub use index::{
    UnresolvedKind, UnresolvedLink, VaultIndexReport, VaultIndexer, VaultSync, PAGE_PROVENANCE,
    SOURCE_PROVENANCE,
};
pub(crate) use index::is_vault_owned;
pub use lint::{LintFinding, LintFindingKind, LintOptions, LintSeverity, VaultLintReport};
pub use page::{is_valid_declared_id, DeclaredEvidence, VaultPage};
pub use pointer::PageRef;
pub use wikilink::WikiLink;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::status::SkippedFile;

/// The `meta` key holding the fingerprint of the vault as it was last indexed.
///
/// Exported so callers that report freshness — [`crate::status::index_status`]
/// — compare against the same value the indexer recorded, rather than a second
/// spelling of it that could drift.
pub const FINGERPRINT_KEY: &str = "vault_fingerprint";

/// The metadata-only fingerprint of a vault: which files exist, how long each
/// is, and when each was last written.
///
/// Answers "has the vault changed since it was indexed?" with a directory walk
/// instead of the read every page a reconcile costs. It is explicitly a
/// heuristic — see [`scan::fingerprint`] — and a missed edit is repaired by the
/// next explicit index.
pub fn fingerprint(root: impl AsRef<Path>) -> Result<String> {
    scan::fingerprint(root.as_ref())
}

/// Budgets that stop a pathological vault from stalling the app.
///
/// Every limit that is hit sets [`VaultSnapshot::complete`] to false, and an
/// incomplete snapshot is never allowed to delete anything: a truncated scan
/// must not be mistaken for "the user deleted half their wiki".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultScanOptions {
    pub max_pages: usize,
    pub max_sources: usize,
    pub max_page_bytes: u64,
    pub max_total_bytes: u64,
    /// Wall-clock ceiling for one pass, in milliseconds. `0` means no ceiling.
    /// Reaching it stops the walk and marks the snapshot incomplete, exactly as
    /// a size budget does — the point is that a scan reports what it did not
    /// finish rather than appearing to have read everything.
    pub max_duration_ms: u64,
}

impl Default for VaultScanOptions {
    fn default() -> Self {
        Self {
            max_pages: 50_000,
            max_sources: 50_000,
            max_page_bytes: 4_000_000,
            max_total_bytes: 268_435_456,
            max_duration_ms: 30_000,
        }
    }
}

/// A file under `raw/sources/`.
///
/// Only the metadata is recorded. What the file *says* is the ingest
/// pipeline's business, and the page it produced is already indexed; what
/// the index needs is that the source exists and can be pointed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSource {
    /// Path relative to `raw/sources/` — the identity a page's `sources:`
    /// frontmatter uses.
    pub identity: String,
    /// Vault-relative path, e.g. `raw/sources/papers/wei-2022.pdf`.
    pub path: String,
    pub bytes: u64,
    /// Modification time in nanoseconds since the Unix epoch, recorded so the
    /// source inventory can tell whether the file moved on without reading it.
    #[serde(default)]
    pub mtime_ns: i64,
}

/// Everything a vault contains, read but not yet interpreted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultSnapshot {
    pub pages: Vec<VaultPage>,
    pub sources: Vec<VaultSource>,
    pub warnings: Vec<String>,
    /// False when a budget stopped the walk before the vault ended.
    pub complete: bool,
    /// What the walk saw and did not read, with a reason. `complete: false`
    /// says a scan was truncated; this says which files it left behind and
    /// why, which is what a person can act on.
    pub skipped: Vec<SkippedFile>,
    /// Paths of pages that still hold exactly what arrived from a pull, a fork or an import. The
    /// indexer caps their authority: a claim of authority in text that came from elsewhere is a
    /// claim, not a fact. See [`crate::portable::ledger`].
    pub arrived: std::collections::BTreeSet<String>,
}

impl VaultSnapshot {
    /// Read the vault rooted at `root` — the project directory holding
    /// `wiki/`, not the `.athenaeum/` workspace inside it.
    pub fn scan(root: impl AsRef<Path>, options: VaultScanOptions) -> Result<Self> {
        scan::scan(root.as_ref(), options)
    }
}
