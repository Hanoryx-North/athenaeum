use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{API_VERSION, PORTABLE_FORMAT_VERSION, SYNC_PROTOCOL_VERSION};

pub const API_PREFIX: &str = "/api/v1";
pub const MANIFEST_FILE: &str = "athenaeum.json";
pub const SYNC_EXTENSIONS: &[&str] = &["md"];
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_MAX_REPOSITORY_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryManifest {
    pub format_version: u32,
    pub repository_id: String,
    pub name: String,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl RepositoryManifest {
    pub fn format_supported(&self) -> bool {
        self.format_version <= PORTABLE_FORMAT_VERSION
    }
}

/// Key declaration order is the canonical JSON key order for a tree hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub service: String,
    pub api_version: u32,
    pub sync_protocol: u32,
    #[serde(default)]
    pub limits: Option<SyncLimits>,
}

impl ServerInfo {
    pub fn compatible(&self) -> bool {
        self.api_version == API_VERSION && self.sync_protocol == SYNC_PROTOCOL_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOwner {
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub owner: Option<RemoteOwner>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub tree_hash: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<String>,
    #[serde(default)]
    pub manifest: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub base: Option<String>,
    pub message: String,
    pub changes: Vec<Change>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRepository {
    /// Requests this exact identity. An occupied id is a 409 conflict. Omitted from the wire
    /// when absent; the server rejects `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// HTTP errors reuse the bridge's error body but omit its protocol/status wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub error: crate::bridge::ProtocolError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// `error.code` values the sync routes can answer.
pub const SYNC_ERROR_CODES: &[&str] = &[
    "invalid_request",
    "not_found",
    "request_too_large",
    "unsupported_protocol",
    "unauthorized",
    "forbidden",
    "stale_base",
    "conflict",
    "invalid_path",
    "invalid_content",
    "limit_exceeded",
    "rate_limited",
];

pub fn repositories_path() -> &'static str {
    "/api/v1/repositories"
}
pub fn repository_path(id: &str) -> String {
    format!("{API_PREFIX}/repositories/{id}")
}
pub fn revision_path(id: &str, revision: &str) -> String {
    format!("{API_PREFIX}/repositories/{id}/revisions/{revision}")
}
pub fn blob_path(id: &str, sha256: &str) -> String {
    format!("{API_PREFIX}/repositories/{id}/blobs/{sha256}")
}
pub fn push_path(id: &str) -> String {
    format!("{API_PREFIX}/repositories/{id}/push")
}

/// A repository id as `athenaeum.json` carries it and the hosted API accepts it: `repo_` followed by
/// 1 to 60 ASCII letters, digits, `_` or `-`. Local and hosted repositories share this one value.
pub fn validate_repository_id(id: &str) -> Result<(), &'static str> {
    let Some(suffix) = id.strip_prefix("repo_") else {
        return Err("repository ID must start with repo_");
    };
    if suffix.is_empty() || suffix.len() > 60 {
        return Err("repository ID needs 1 to 60 characters after repo_");
    }
    if !suffix
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("repository ID may contain only letters, digits, _ and -");
    }
    Ok(())
}

/// The repository sync set excludes hidden segments and paths that could escape a root.
pub fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("empty path");
    }
    if path.len() > 1024 {
        return Err("path exceeds 1024 UTF-8 bytes");
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("path must be relative and use forward slashes");
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("drive path");
    }
    if path.chars().any(char::is_control) {
        return Err("control character");
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err("empty or traversal segment");
        }
        if segment.starts_with('.') {
            return Err("hidden segment");
        }
        if segment.len() > 255 {
            return Err("segment exceeds 255 UTF-8 bytes");
        }
    }
    Ok(())
}

pub fn has_sync_extension(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| SYNC_EXTENSIONS.contains(&ext))
}

/// The hosted set contains wiki Markdown, root Markdown documents and repository identity.
pub fn is_sync_path(path: &str) -> bool {
    validate_path(path).is_ok()
        && (path == MANIFEST_FILE
            || ((path.starts_with("wiki/") || !path.contains('/')) && has_sync_extension(path)))
}

pub fn validate_content(bytes: &[u8], limits: SyncLimits) -> Result<(), &'static str> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err("file exceeds limit");
    }
    if bytes.contains(&0) {
        return Err("NUL byte");
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err("invalid UTF-8");
    }
    Ok(())
}

/// Canonical manifest bytes are compact JSON, entries sorted by UTF-8 path bytes, keys
/// in `path`, `sha256`, `size` order. SHA-256 of these bytes is `treeHash`.
pub fn canonical_manifest(entries: &[ManifestEntry]) -> String {
    let mut ordered: Vec<&ManifestEntry> = entries.iter().collect();
    ordered.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    serde_json::to_string(&ordered).expect("manifest entries serialize")
}

/// The bytes whose SHA-256 is a revision id (`rev_` plus 64 hex digits; the server assigns it).
/// Keys are alphabetical, `mergedFrom` is present only when set, `parent` is `null` for a first
/// revision. Field declaration order below is the key order, so no serde feature can reorder it.
pub fn canonical_revision(
    parent: Option<&str>,
    tree_hash: &str,
    author: &str,
    message: &str,
    created_at: &str,
    merged_from: Option<&str>,
) -> String {
    #[derive(Serialize)]
    struct Canonical<'a> {
        author: &'a str,
        #[serde(rename = "createdAt")]
        created_at: &'a str,
        #[serde(rename = "mergedFrom", skip_serializing_if = "Option::is_none")]
        merged_from: Option<&'a str>,
        message: &'a str,
        parent: Option<&'a str>,
        #[serde(rename = "treeHash")]
        tree_hash: &'a str,
    }
    serde_json::to_string(&Canonical {
        author,
        created_at,
        merged_from: merged_from.filter(|value| !value.is_empty()),
        message,
        parent,
        tree_hash,
    })
    .expect("revision fields serialize")
}
