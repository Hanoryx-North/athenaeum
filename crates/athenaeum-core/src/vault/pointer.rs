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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EvidenceKind, NodeType, Provenance, SourceAnchor};

    fn page_node() -> KnowledgeNode {
        KnowledgeNode::new(NodeType::Concept, "Attention").with_provenance(
            Provenance::deterministic(format!("{PAGE_PROVENANCE}:concept")),
        )
    }

    fn page_evidence(node: &KnowledgeNode, path: &str) -> Evidence {
        Evidence::new(node.id.clone(), EvidenceKind::Documentation)
            .with_id(format!("{PAGE_EVIDENCE_PREFIX}abc"))
            .with_anchor(SourceAnchor {
                content_hash: Some("hash-1".to_string()),
                ..SourceAnchor::file(path)
            })
    }

    #[test]
    fn a_page_node_points_at_its_file() {
        let node = page_node();
        let evidence = [page_evidence(&node, "wiki/concepts/attention.md")];

        let page = PageRef::of(&node, &evidence, Some(Path::new("/work/project"))).unwrap();

        assert_eq!(page.path, "wiki/concepts/attention.md");
        assert_eq!(page.content_hash, "hash-1");
        let absolute = PathBuf::from(page.absolute_path.as_deref().unwrap());
        assert_eq!(
            absolute,
            Path::new("/work/project")
                .join("wiki")
                .join("concepts")
                .join("attention.md")
        );
    }

    #[test]
    fn without_a_project_root_only_the_relative_path_is_known() {
        let node = page_node();
        let evidence = [page_evidence(&node, "wiki/a.md")];

        let page = PageRef::of(&node, &evidence, None).unwrap();

        assert_eq!(page.absolute_path, None);
        assert_eq!(page.best_path(), "wiki/a.md");
    }

    #[test]
    fn a_node_that_is_not_a_page_has_no_pointer() {
        // A node a model proposed over the bridge, even one whose evidence names a file.
        let node = KnowledgeNode::new(NodeType::Observation, "Maybe caches")
            .with_provenance(Provenance::model("some-model"));
        let evidence = [page_evidence(&node, "wiki/a.md")];

        assert_eq!(PageRef::of(&node, &evidence, Some(Path::new("/p"))), None);
    }

    #[test]
    fn a_raw_source_is_not_a_page() {
        let node = KnowledgeNode::new(NodeType::Source, "paper.pdf")
            .with_provenance(Provenance::deterministic("vault_index:source"));
        let evidence = [page_evidence(&node, "raw/sources/paper.pdf")];

        assert_eq!(PageRef::of(&node, &evidence, Some(Path::new("/p"))), None);
    }

    #[test]
    fn a_page_without_its_own_file_record_has_no_pointer() {
        let node = page_node();
        // Evidence that cites a source but is not the page's own record.
        let citation = Evidence::new(node.id.clone(), EvidenceKind::ExternalSource)
            .with_id("ev_vault_cite_1")
            .with_anchor(SourceAnchor::file("raw/sources/x.pdf"));

        assert_eq!(PageRef::of(&node, &[citation], Some(Path::new("/p"))), None);
    }

    #[test]
    fn a_pointer_serializes_in_camel_case_and_omits_what_is_unknown() {
        let page = PageRef {
            path: "wiki/a.md".to_string(),
            absolute_path: None,
            content_hash: "h".to_string(),
        };

        let json = serde_json::to_value(&page).unwrap();

        assert_eq!(
            json,
            serde_json::json!({ "path": "wiki/a.md", "contentHash": "h" })
        );
    }
}
