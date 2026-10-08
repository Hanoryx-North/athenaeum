//! Where a page of the vault lives on disk, read back off the index.
//!
//! The vault is the source of truth and the agent edits it with ordinary file tools, so a node
//! that *is* a page has to say which file. Nothing new is stored for that: the indexer already
//! attaches one `Documentation` record to every page it indexes, anchored at the page file and
//! carrying the hash of the page as it read it. This reads that record back.
//!
//! What the pointer deliberately does not do is stand in for the node. The markdown holds the
//! prose; the relationships, evidence, history and utility are in the index and nowhere else. A
//! pointer is where to change what the project *says*, and the node is what it *knows*.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Evidence, KnowledgeNode, ProvenanceSource};

use super::index::{PAGE_EVIDENCE_PREFIX, PAGE_PROVENANCE};

/// The file behind a node that is a page of the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    /// Vault-relative path with forward slashes, e.g. `wiki/concepts/attention.md`.
    ///
    /// This is the page's identity — what `[[wikilinks]]` and the indexer key on — and it is the
    /// same on every platform.
    pub path: String,
    /// The same file as an absolute path on this host, which is what file tools can open.
    ///
    /// Absent when the library cannot say where the project is. The path is returned even when the
    /// vault sits outside what an agent's sandbox can reach: a pointer that silently disappears is
    /// harder to debug than a read that fails loudly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_path: Option<String>,
    /// SHA-256 of the page as the index last read it.
    ///
    /// A page edited since then has a different hash on disk, which is how a reader can tell the
    /// node it holds is behind the file it is about to change.
    pub content_hash: String,
}

impl PageRef {
    /// The page `node` was indexed from, or `None` when it is not a vault page.
    ///
    /// `evidence` is the node's own evidence, which the caller usually has in hand already.
    /// `project_root` is where the vault is rooted; without it only the relative path is known.
    pub fn of(
        node: &KnowledgeNode,
        evidence: &[Evidence],
        project_root: Option<&Path>,
    ) -> Option<Self> {
        if !is_vault_page(node) {
            return None;
        }
        let record = evidence
            .iter()
            .find(|record| record.id.as_str().starts_with(PAGE_EVIDENCE_PREFIX))?;
        let anchor = record.anchor.as_ref()?;
        let content_hash = anchor.content_hash.clone()?;
        Some(Self {
            absolute_path: project_root.map(|root| absolute(root, &anchor.file)),
            path: anchor.file.clone(),
            content_hash,
        })
    }

    /// The path an agent should be given: the absolute one when known, else the relative one.
    pub fn best_path(&self) -> &str {
        self.absolute_path.as_deref().unwrap_or(&self.path)
    }
}

/// A node the vault indexer wrote for a page, as opposed to a raw source or anything else.
fn is_vault_page(node: &KnowledgeNode) -> bool {
    node.provenance.source == ProvenanceSource::Deterministic
        && node
            .provenance
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with(PAGE_PROVENANCE))
}

/// Join a vault-relative path onto the project root one component at a time, so the result
/// uses this host's separator throughout rather than a mixture.
fn absolute(root: &Path, relative: &str) -> String {
    relative
        .split('/')
        .filter(|component| !component.is_empty())
        .fold(PathBuf::from(root), |path, component| path.join(component))
        .to_string_lossy()
        .into_owned()
}
