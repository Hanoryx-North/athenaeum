//! The portable repository: what leaves a machine, what arrives, and how much of it is believed.
//!
//! The vault is the source of truth and `knowledge.db` is an index over it. This module is
//! everything that follows from taking that seriously once a repository has to move between
//! computers: the manifest that names a repository, the set of files that syncs, the ledger that
//! remembers what arrived from elsewhere, export and import, the proposal lifecycle that turns a
//! model's suggestion into durable text, and the sync engine itself.
//!
//! None of it makes a network call. The sync engine speaks to a [`sync::Remote`], and the
//! implementation of that trait that uses HTTP lives in the CLI, so this crate stays what
//! `lib.rs` promises it is.
//!
//! The contract these types implement is `docs/portable-format.md`.

pub mod archive;
pub mod ledger;
pub mod lock;
pub mod manifest;
pub mod memory;
pub mod merge;
pub mod sync;
pub mod syncset;

use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};

/// Write `bytes` to `path` so that a reader never sees half of them.
///
/// The bytes go to a sibling temporary file that is flushed and then renamed over the target. A
/// crash leaves either the old file or the new one, never a truncated page.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| Error::InvalidValue {
        field: "path",
        value: path.display().to_string(),
    })?;
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(Error::from)
}

/// An error about the portable format or a sync, in the terms the protocol reports.
pub fn portable_error(code: impl Into<String>, message: impl Into<String>) -> Error {
    Error::Portable {
        code: code.into(),
        message: message.into(),
    }
}
