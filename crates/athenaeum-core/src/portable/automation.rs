//! Per-wiki automation choices. Local settings never enter the sync set.
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{memory, write_atomic};
use crate::error::Result;
use crate::store::KnowledgeStore;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Automation {
    pub accept_proposals: bool,
    pub auto_sync: bool,
}

impl Automation {
    pub fn load(project: &Path) -> Result<Self> {
        match std::fs::read(project.join(".athenaeum/automation.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, project: &Path) -> Result<()> {
        write_atomic(
            &project.join(".athenaeum/automation.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceBatch {
    pub accepted: Vec<memory::Accepted>,
    pub errors: Vec<String>,
}

/// Keep the queue intact on individual failures, and reconcile just once per batch.
pub fn accept_pending(store: &mut KnowledgeStore, settled_only: bool) -> Result<AcceptanceBatch> {
    let mut batch = AcceptanceBatch::default();
    for node in memory::proposals(store, None)? {
        // A background extraction can fail between creating its proposal and
        // attaching checked evidence. Such drafts need review, not automatic saving.
        if settled_only
            && node.provenance.detail.as_deref() == Some("athenaeum-background-memory")
            && store.evidence_for(&node.id)?.is_empty()
        {
            continue;
        }
        // Leave time for a capturing agent to attach evidence and relationships.
        if settled_only && (chrono::Utc::now() - node.updated_at).num_seconds() < 15 {
            continue;
        }
        match memory::accept_deferred(store, &node.id, "user-automation") {
            Ok(accepted) => batch.accepted.push(accepted),
            Err(error) => batch.errors.push(format!("{}: {error}", node.title)),
        }
    }
    if !batch.accepted.is_empty() {
        if let Some(project) = store.project_root() {
            crate::vault::VaultIndexer::scan_and_index(store, &project, Default::default())?;
        }
    }
    Ok(batch)
}

pub fn maintain(store: &mut KnowledgeStore) -> Result<AcceptanceBatch> {
    match store.project_root() {
        Some(project) if Automation::load(&project)?.accept_proposals => {
            accept_pending(store, true)
        }
        _ => Ok(AcceptanceBatch::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Evidence, EvidenceKind, KnowledgeNode, NodeType, Provenance};

    #[test]
    fn background_drafts_are_not_automatically_saved_before_evidence_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = KnowledgeStore::open(dir.path().join(".athenaeum")).unwrap();
        let draft = KnowledgeNode::new(NodeType::Gotcha, "Checked background finding")
            .with_provenance(Provenance::model("athenaeum-background-memory"));
        let draft = store.create_node(draft).unwrap();
        let settled = (chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339();
        let settle = |store: &KnowledgeStore| {
            store
                .connection()
                .execute(
                    "UPDATE nodes SET updated_at = ?2 WHERE id = ?1",
                    rusqlite::params![draft.id.as_str(), settled],
                )
                .unwrap();
        };
        settle(&store);
        assert!(accept_pending(&mut store, true)
            .unwrap()
            .accepted
            .is_empty());
        assert_eq!(memory::proposals(&store, None).unwrap().len(), 1);
        store
            .attach_evidence(
                Evidence::new(draft.id.clone(), EvidenceKind::ExternalSource)
                    .with_excerpt("An exact source excerpt checked by the worker.")
                    .with_note("Source workspace and path are outside this wiki."),
            )
            .unwrap();
        settle(&store);
        let batch = accept_pending(&mut store, true).unwrap();
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);
        assert_eq!(batch.accepted.len(), 1);
    }
}
