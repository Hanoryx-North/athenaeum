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
}
