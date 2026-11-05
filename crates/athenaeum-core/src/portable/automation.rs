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
