//! The sync set: the files of a repository that travel, and the manifest that names them.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::manifest::{RepositoryManifest, FORMAT_VERSION, MANIFEST_FILE};
use super::portable_error;
use crate::error::Result;

/// Per-file ceiling the server advertises by default.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Per-repository ceiling the server advertises by default.
pub const DEFAULT_MAX_REPOSITORY_BYTES: u64 = 50 * 1024 * 1024;
const MAX_SEGMENT_BYTES: usize = 255;
const MAX_PATH_BYTES: usize = 1024;

/// One file of the sync set. Fields are declared in alphabetical order because the canonical
/// serialisation of a manifest is "keys sorted", and serde writes them in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// Why a file stayed local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

/// What the server will accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncLimits {
    pub max_file_bytes: u64,
    pub max_repository_bytes: u64,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_repository_bytes: DEFAULT_MAX_REPOSITORY_BYTES,
        }
    }
}

/// The files that sync, sorted by path, and the ones that did not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSet {
    pub entries: Vec<ManifestEntry>,
    pub skipped: Vec<Skipped>,
}

impl SyncSet {
    pub fn find(&self, path: &str) -> Option<&ManifestEntry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    pub fn tree_hash(&self) -> String {
        tree_hash(&self.entries)
    }
}

/// Check a repository-relative path against the format's rules.
///
/// `/` separators, no `.` or `..` segment, no leading `/`, no drive letter, no control characters,
/// no hidden segment, at most 255 bytes per segment and 1,024 in total. Returns the reason on
/// failure so a caller can name the first offending path.
pub fn validate_path(path: &str) -> std::result::Result<(), String> {
    if path.is_empty() {
        return Err("the path is empty".into());
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(format!("the path is longer than {MAX_PATH_BYTES} bytes"));
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("the path must be relative and use `/` separators".into());
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("the path must not start with a drive letter".into());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("the path contains a control character".into());
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err("the path contains an empty segment".into()),
            "." | ".." => return Err("the path contains a `.` or `..` segment".into()),
            _ if segment.starts_with('.') => {
                return Err("the path contains a hidden segment".into())
            }
            _ if segment.len() > MAX_SEGMENT_BYTES => {
                return Err(format!(
                    "a path segment is longer than {MAX_SEGMENT_BYTES} bytes"
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether a path is an accepted wiki page or the repository identity.
pub fn has_sync_extension(path: &str) -> bool {
    path == "athenaeum.json"
        || path
            .strip_prefix("wiki/")
            .is_some_and(|page| !page.is_empty() && page.ends_with(".md"))
}

/// Check a file body against the format's rules: UTF-8, no NUL, within the per-file limit.
pub fn validate_content(bytes: &[u8], limits: &SyncLimits) -> std::result::Result<(), String> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(format!(
            "the file is {} bytes, over the {} byte limit",
            bytes.len(),
            limits.max_file_bytes
        ));
    }
    if bytes.contains(&0) {
        return Err("the file contains a NUL byte".into());
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err("the file is not valid UTF-8".into());
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The canonical serialisation of a manifest: sorted by path byte order, keys sorted, no
/// insignificant whitespace, UTF-8.
pub fn canonical_manifest(entries: &[ManifestEntry]) -> String {
    let mut sorted: Vec<&ManifestEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    serde_json::to_string(&sorted).expect("a manifest always serialises")
}

pub fn tree_hash(entries: &[ManifestEntry]) -> String {
    sha256_hex(canonical_manifest(entries).as_bytes())
}

/// Walk the repository at `root` and decide, file by file, what syncs.
///
/// Reads every candidate to hash it. A file that cannot sync is reported with its reason and is
/// never an error.
pub fn scan(root: &Path, limits: &SyncLimits) -> Result<SyncSet> {
    let mut set = SyncSet::default();
    let mut total = 0_u64;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
        });

    for entry in walker {
        let Ok(entry) = entry else { continue };
        let file_type = entry.file_type();
        let Some(relative) = relative(root, entry.path()) else {
            if file_type.is_file() {
                set.skipped.push(Skipped {
                    path: entry.path().display().to_string(),
                    reason: "the path is not valid UTF-8".into(),
                });
            }
            continue;
        };
        if file_type.is_symlink() {
            set.skipped.push(Skipped {
                path: relative,
                reason: "symbolic links do not sync".into(),
            });
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if !has_sync_extension(&relative) {
            set.skipped.push(Skipped {
                path: relative,
                reason: "only accepted wiki Markdown and athenaeum.json sync".into(),
            });
            continue;
        }
        if let Err(reason) = validate_path(&relative) {
            set.skipped.push(Skipped {
                path: relative,
                reason,
            });
            continue;
        }
        let bytes = match std::fs::read(entry.path()) {
            Ok(bytes) => bytes,
            Err(error) => {
                set.skipped.push(Skipped {
                    path: relative,
                    reason: format!("it could not be read: {error}"),
                });
                continue;
            }
        };
        if let Err(reason) = validate_content(&bytes, limits) {
            set.skipped.push(Skipped {
                path: relative,
                reason,
            });
            continue;
        }
        if relative == MANIFEST_FILE {
            let manifest: RepositoryManifest = serde_json::from_slice(&bytes).map_err(|error| {
                portable_error(
                    "invalid_content",
                    format!("{MANIFEST_FILE} cannot sync: {error}"),
                )
            })?;
            if manifest.format_version != FORMAT_VERSION || !manifest.extra.is_empty() {
                return Err(portable_error(
                    "invalid_content",
                    format!("{MANIFEST_FILE} cannot sync with an unsupported format or extra fields; only repository identity metadata may leave this machine"),
                ));
            }
        }
        if total + bytes.len() as u64 > limits.max_repository_bytes {
            set.skipped.push(Skipped {
                path: relative,
                reason: "the repository size limit was reached".into(),
            });
            continue;
        }
        total += bytes.len() as u64;
        set.entries.push(ManifestEntry {
            sha256: sha256_hex(&bytes),
            size: bytes.len() as u64,
            path: relative,
        });
    }

    set.entries
        .sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(set)
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut out = String::new();
    for component in relative.components() {
        let part = component.as_os_str().to_str()?;
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_could_escape_the_repository_are_refused() {
        for bad in [
            "",
            "/etc/passwd",
            "../x.md",
            "a/../b.md",
            "a//b.md",
            "./a.md",
            "C:/x.md",
            "a\\b.md",
            "a/\u{7}b.md",
            ".git/config.toml",
            "wiki/.hidden.md",
        ] {
            assert!(validate_path(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(validate_path("wiki/concepts/retry-policy.md").is_ok());
    }

    #[test]
    fn only_text_files_outside_hidden_directories_sync() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::create_dir_all(root.join(".athenaeum")).unwrap();
        std::fs::create_dir_all(root.join("raw/sources")).unwrap();
        std::fs::write(root.join("wiki/a.md"), "# A\n").unwrap();
        std::fs::write(root.join(".athenaeum/knowledge.db.json"), "{}").unwrap();
        std::fs::write(root.join("raw/sources/paper.pdf"), b"%PDF").unwrap();
        std::fs::write(root.join("raw/sources/blob.md"), b"a\0b").unwrap();
        std::fs::write(root.join("raw/sources/latin.txt"), [0xff, 0xfe]).unwrap();

        let set = scan(root, &SyncLimits::default()).unwrap();
        let paths: Vec<_> = set.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["wiki/a.md"]);
        let skipped: Vec<_> = set.skipped.iter().map(|s| s.path.as_str()).collect();
        assert!(skipped.contains(&"raw/sources/paper.pdf"));
        assert!(skipped.contains(&"raw/sources/blob.md"));
        assert!(skipped.contains(&"raw/sources/latin.txt"));
        assert!(!skipped.iter().any(|p| p.starts_with(".athenaeum")));
    }

    #[test]
    fn the_tree_hash_does_not_depend_on_input_order() {
        let a = ManifestEntry {
            path: "a.md".into(),
            sha256: "1".into(),
            size: 1,
        };
        let b = ManifestEntry {
            path: "b.md".into(),
            sha256: "2".into(),
            size: 2,
        };
        assert_eq!(
            tree_hash(&[a.clone(), b.clone()]),
            tree_hash(&[b.clone(), a.clone()])
        );
        assert_eq!(
            canonical_manifest(&[b, a]),
            r#"[{"path":"a.md","sha256":"1","size":1},{"path":"b.md","sha256":"2","size":2}]"#
        );
    }
}
