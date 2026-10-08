//! Content-addressed storage for evidence material (buildplan §59).
//!
//! Ten nodes citing the same file should not mean ten copies of that file in
//! the database. Material is written once under the hash of its content, and
//! evidence rows reference it by that hash.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::model::hash;

/// A directory of immutable, hash-named files.
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// `root` is the `blobs/` directory; it is created on demand.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store `content` and return its hash.
    ///
    /// Writing the same content twice is free and idempotent: the second call
    /// sees the file already exists and returns the same hash.
    pub fn put(&self, content: &str) -> Result<String> {
        let digest = hash(content);
        let path = self.path_for(&digest);

        if path.exists() {
            return Ok(digest);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write to a temporary name and rename into place, so a process that
        // dies mid-write cannot leave a truncated file sitting at a hash that
        // claims to describe complete content.
        //
        // The temporary name must be unique per writer, not per digest: the
        // desktop app and the CLI can hold the same workspace open at once,
        // and two of them storing identical content would otherwise write the
        // same temporary path — interleaving their bytes, and leaving the
        // loser's rename to fail on a file the winner already moved.
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        fs::write(&temp, content)?;
        if let Err(e) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(e.into());
        }

        Ok(digest)
    }

    /// Read content back, or `None` if this store has never seen that hash.
    pub fn get(&self, digest: &str) -> Result<Option<String>> {
        let path = self.path_for(digest);
        match fs::read_to_string(&path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn has(&self, digest: &str) -> bool {
        self.path_for(digest).exists()
    }

    /// `blobs/sha256/ab/cdef…`.
    ///
    /// The two-character fan-out keeps directory listings usable; a flat
    /// directory with 100k entries is slow to enumerate on every platform and
    /// genuinely painful on Windows.
    pub fn path_for(&self, digest: &str) -> PathBuf {
        let (prefix, rest) = digest.split_at(digest.len().min(2));
        self.root.join("sha256").join(prefix).join(rest)
    }
}
