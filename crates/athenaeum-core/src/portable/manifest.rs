//! `athenaeum.json`: the identity of a knowledge repository.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::{portable_error, write_atomic};
use crate::error::Result;

/// Filename of the manifest at the repository root.
pub const MANIFEST_FILE: &str = "athenaeum.json";
/// The format this build reads and writes. A higher one is refused, never downgraded.
pub const FORMAT_VERSION: u32 = 1;

/// The repository manifest.
///
/// Fields this build does not know are kept in [`Self::extra`] and written back, so a manifest
/// edited by a newer tool survives a round trip through an older one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryManifest {
    pub format_version: u32,
    /// Generated once and never changed, whatever the repository is renamed to.
    pub repository_id: String,
    pub name: String,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl RepositoryManifest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            repository_id: new_repository_id(),
            name: name.into(),
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            extra: BTreeMap::new(),
        }
    }

    /// Read the manifest at `root`, or `None` for a legacy vault that has none.
    ///
    /// A missing manifest is never an error. A malformed one, or one from a newer format, is: the
    /// alternative is guessing at an identity that other devices are relying on.
    pub fn read(root: &Path) -> Result<Option<Self>> {
        let path = root.join(MANIFEST_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let manifest: Self = serde_json::from_str(&text).map_err(|error| {
            portable_error(
                "invalid_request",
                format!("{MANIFEST_FILE} is not a valid manifest: {error}"),
            )
        })?;
        if manifest.format_version > FORMAT_VERSION {
            return Err(portable_error(
                "unsupported_format",
                format!(
                    "{MANIFEST_FILE} is format {}; this build supports up to {FORMAT_VERSION}. \
                     Upgrade Athenaeum to open this repository",
                    manifest.format_version
                ),
            ));
        }
        if !is_valid_repository_id(&manifest.repository_id) {
            return Err(portable_error(
                "invalid_request",
                format!(
                    "{MANIFEST_FILE} has an invalid repositoryId {:?}",
                    manifest.repository_id
                ),
            ));
        }
        Ok(Some(manifest))
    }

    pub fn write(&self, root: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(&root.join(MANIFEST_FILE), text.as_bytes())
    }

    /// Read the manifest, creating it when the vault has none. The flag says whether it was made.
    pub fn read_or_init(root: &Path, name: &str) -> Result<(Self, bool)> {
        match Self::read(root)? {
            Some(manifest) => Ok((manifest, false)),
            None => {
                let manifest = Self::new(name);
                manifest.write(root)?;
                Ok((manifest, true))
            }
        }
    }
}

pub fn new_repository_id() -> String {
    format!("repo_{}", uuid::Uuid::new_v4())
}

/// `repo_` followed by something that cannot escape a path or a URL segment: 1 to 60 letters,
/// digits, `-` or `_`, which is the rule the Library and `athenaeum_protocol` apply to the same
/// value (so the longest ID a Library accepts is one this build accepts too).
pub fn is_valid_repository_id(id: &str) -> bool {
    id.len() >= 6
        && id.len() <= 65
        && id.starts_with("repo_")
        && id[5..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A directory name made safe as a default repository name.
pub fn default_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "knowledge".to_string())
}
