//! Locating and creating the on-disk workspace (buildplan §57).

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::store::KnowledgeStore;

/// Directory name placed inside a project.
pub const WORKSPACE_DIR: &str = ".athenaeum";

/// The `.athenaeum/` directory belonging to a project.
///
/// ```text
/// .athenaeum/
/// ├── config.toml
/// ├── knowledge.db
/// ├── blobs/sha256/
/// ├── exports/
/// └── cache/
/// ```
///
/// Note what is absent: no `models/` directory. Athenaeum never downloads a
/// model — knowledge is local, intelligence is not (§4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Treat `dir` as an existing workspace directory.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { root: dir.into() }
    }

    /// Create the workspace layout inside `project_root`.
    ///
    /// Idempotent: running it on an initialized project leaves the existing
    /// knowledge untouched.
    pub fn init(project_root: impl AsRef<Path>) -> Result<Self> {
        let root = project_root.as_ref().join(WORKSPACE_DIR);
        for dir in ["blobs", "exports", "cache"] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root })
    }

    /// Walk up from `start` looking for an existing workspace, the way git
    /// finds `.git`. Returns `None` at the filesystem root.
    pub fn discover(start: impl AsRef<Path>) -> Option<Self> {
        let mut cursor = start.as_ref();
        loop {
            let candidate = cursor.join(WORKSPACE_DIR);
            if candidate.is_dir() {
                return Some(Self { root: candidate });
            }
            cursor = cursor.parent()?;
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project directory containing this workspace.
    pub fn project_root(&self) -> Option<&Path> {
        self.root.parent()
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn exports_dir(&self) -> PathBuf {
        self.root.join("exports")
    }

    /// Derived data. Deleting this directory must never lose knowledge (§60).
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Open the knowledge base held in this workspace.
    pub fn open(&self) -> Result<KnowledgeStore> {
        KnowledgeStore::open(&self.root)
    }

    /// The workspace an open store is rooted in.
    ///
    /// Lets a caller that already holds a store — the bridge, the desktop —
    /// reach the paths around it without a second `discover`, and without the
    /// risk of a caller-supplied workspace disagreeing with the store it is
    /// reading.
    pub fn of_store(store: &KnowledgeStore) -> Self {
        Self::at(store.root().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_the_documented_layout() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::init(dir.path()).unwrap();
        assert!(ws.root().join("blobs").is_dir());
        assert!(ws.root().join("exports").is_dir());
        assert!(ws.root().join("cache").is_dir());
        // No models directory: intelligence is not local (§4).
        assert!(!ws.root().join("models").exists());
    }

    #[test]
    fn init_twice_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let first = Workspace::init(dir.path()).unwrap();
        let second = Workspace::init(dir.path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn discovery_walks_up_from_a_nested_directory() {
        let dir = tempfile::tempdir().unwrap();
        Workspace::init(dir.path()).unwrap();
        let nested = dir.path().join("src").join("auth").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        let found = Workspace::discover(&nested).expect("should find the workspace above");
        assert_eq!(found.root(), dir.path().join(WORKSPACE_DIR));
    }

    #[test]
    fn discovery_returns_none_outside_any_project() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Workspace::discover(dir.path()).is_none());
    }
}
