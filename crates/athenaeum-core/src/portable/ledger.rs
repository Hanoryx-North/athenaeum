//! The trust ledger: what arrived from somewhere else, and has not been reviewed since.
//!
//! A page's `authority:` is text, and text that came in with a pull, a fork or an import is a
//! claim made by whoever wrote it, not a fact about this machine. The ledger remembers, per path,
//! the hash of the content that arrived. While a page still hashes to what the ledger recorded,
//! the indexer caps its effective authority at `imported_knowledge`. Any local edit changes the
//! hash and lifts the cap: that is the moment a person took responsibility for the text.
//!
//! It lives at `.athenaeum/sync/ledger.json`, local only, never synced. It is deliberately not keyed
//! by repository id: a directory is one repository, and a ledger that went missing because an id
//! changed would silently lift every cap it was holding.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::write_atomic;
use crate::error::Result;
use crate::workspace::WORKSPACE_DIR;

/// Where a recorded hash came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Written by a pull.
    Pulled,
    /// Written by an import.
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub sha256: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrustLedger {
    files: BTreeMap<String, LedgerEntry>,
}

impl TrustLedger {
    /// The ledger for the repository at `root`. Empty when nothing has ever arrived.
    pub fn load(root: &Path) -> Result<Self> {
        let path = ledger_path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(&ledger_path(root), text.as_bytes())
    }

    pub fn record(&mut self, path: &str, sha256: &str, origin: Origin) {
        self.files.insert(
            path.to_string(),
            LedgerEntry {
                sha256: sha256.to_string(),
                origin,
            },
        );
    }

    pub fn forget(&mut self, path: &str) -> bool {
        self.files.remove(path).is_some()
    }

    pub fn entry(&self, path: &str) -> Option<&LedgerEntry> {
        self.files.get(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// True while `path` still holds exactly what arrived from elsewhere.
    pub fn caps(&self, path: &str, content_hash: &str) -> bool {
        self.files
            .get(path)
            .is_some_and(|entry| entry.sha256 == content_hash)
    }
}

/// The directory holding sync state for a repository, inside its workspace.
pub fn sync_dir(root: &Path) -> PathBuf {
    root.join(WORKSPACE_DIR).join("sync")
}

/// Where the ledger for the repository at `root` is kept.
pub fn ledger_path(root: &Path) -> PathBuf {
    sync_dir(root).join("ledger.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_capped_only_while_it_still_holds_what_arrived() {
        let mut ledger = TrustLedger::default();
        ledger.record("wiki/a.md", "abc", Origin::Pulled);
        assert!(ledger.caps("wiki/a.md", "abc"));
        assert!(!ledger.caps("wiki/a.md", "edited"));
        assert!(!ledger.caps("wiki/b.md", "abc"));
    }

    #[test]
    fn a_repository_nothing_arrived_in_has_an_empty_ledger() {
        let dir = tempfile::tempdir().unwrap();
        assert!(TrustLedger::load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn the_ledger_round_trips_in_the_documented_shape() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = TrustLedger::default();
        ledger.record("wiki/a.md", "abc", Origin::Pulled);
        ledger.save(dir.path()).unwrap();
        let loaded = TrustLedger::load(dir.path()).unwrap();
        assert_eq!(loaded, ledger);
        let text = std::fs::read_to_string(ledger_path(dir.path())).unwrap();
        assert!(text.contains("\"wiki/a.md\""), "{text}");
        assert!(text.contains("\"origin\": \"pulled\""), "{text}");
    }
}
