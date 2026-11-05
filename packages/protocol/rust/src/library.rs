//! The Library API v1 shapes that a Rust client of the hosted service needs. This is a deliberately
//! small mirror of `src/library.ts`: it covers what the shared fixtures in `fixtures/library/v1`
//! exercise, and `rust/tests/fixtures.rs` round-trips every one of them so the two languages cannot
//! quietly disagree. Vocabularies are `&str` slices, not enums, so a value added later by the
//! server (the contract is additive inside v1) never makes an older client fail to parse.

use serde::{Deserialize, Serialize};

use crate::sync::{ManifestEntry, SyncLimits};

/// What `GET /api/v1` reports as `service`. The retired name `athenaeum-library` is not accepted.
pub const API_SERVICE_NAME: &str = "athenaeum-api";
pub const TOKEN_SCOPES: &[&str] = &["repo:read", "repo:write", "repo:admin"];
pub const TOKEN_SECRET_PREFIX: &str = "atn_";
pub const REPOSITORY_VISIBILITIES: &[&str] = &["private", "public"];
pub const REPOSITORY_ROLES: &[&str] = &["none", "read", "write", "admin", "owner"];
pub const COLLABORATOR_ROLES: &[&str] = &["read", "write", "admin"];
pub const REPOSITORY_SORTS: &[&str] = &["updated", "created", "name", "forks"];
pub const DEVICE_POLL_ERROR_CODES: &[&str] = &[
    "authorization_pending",
    "slow_down",
    "expired_token",
    "access_denied",
];

/// Every `error.code` a Library API response can carry, with the HTTP status that goes with it.
pub const LIBRARY_ERROR_STATUS: &[(&str, u16)] = &[
    ("invalid_request", 400),
    ("not_found", 404),
    ("request_too_large", 413),
    ("unsupported_protocol", 400),
    ("unauthorized", 401),
    ("forbidden", 403),
    ("stale_base", 409),
    ("conflict", 409),
    ("invalid_path", 422),
    ("invalid_content", 422),
    ("limit_exceeded", 422),
    ("rate_limited", 429),
    ("authorization_pending", 400),
    ("slow_down", 400),
    ("expired_token", 400),
    ("access_denied", 400),
    ("internal_error", 500),
];

pub fn error_status(code: &str) -> Option<u16> {
    LIBRARY_ERROR_STATUS
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, status)| *status)
}

/// `rev_` followed by 64 lowercase hexadecimal digits. The server assigns revision ids.
pub fn validate_revision_id(id: &str) -> Result<(), &'static str> {
    let Some(hex) = id.strip_prefix("rev_") else {
        return Err("revision ID must start with rev_");
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("revision ID needs 64 lowercase hex digits after rev_");
    }
    Ok(())
}

// --- service metadata ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiLimits {
    #[serde(flatten)]
    pub sync: SyncLimits,
    pub max_push_bytes: u64,
    pub max_changes_per_push: u64,
}

/// `GET /api/v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInfo {
    pub service: String,
    pub api_version: u32,
    pub sync_protocol: u32,
    pub limits: ApiLimits,
    pub registration: String,
}

impl ApiInfo {
    pub fn compatible(&self) -> bool {
        self.service == API_SERVICE_NAME
            && self.api_version == crate::API_VERSION
            && self.sync_protocol == crate::SYNC_PROTOCOL_VERSION
    }
}

// --- users and repositories ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUser {
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkOrigin {
    pub id: String,
    #[serde(rename = "fullName")]
    pub full_name: String,
}

/// The repository as every browsing view sees it. `role` is what the caller may do to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryView {
    pub id: String,
    pub owner: PublicUser,
    pub name: String,
    pub full_name: String,
    pub description: String,
    pub visibility: String,
    pub head: Option<String>,
    pub forked_from: Option<ForkOrigin>,
    pub forks: u64,
    pub file_count: u64,
    pub size_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
    pub role: String,
}

/// The pagination envelope used by every listing that can be long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

// --- reading a repository ------------------------------------------------------------------------

/// A revision as the browsing routes return it. The list route omits `manifest`; the single and push
/// routes include it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    pub id: String,
    pub parent: Option<String>,
    pub tree_hash: String,
    pub author: String,
    pub message: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Vec<ManifestEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionList {
    pub revisions: Vec<RevisionView>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeListing {
    pub revision: Option<String>,
    pub path: String,
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContent {
    pub revision: String,
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String,
    pub text: String,
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub hunks: Vec<DiffHunk>,
    pub coarse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub from: Option<String>,
    pub to: String,
    pub files: Vec<FileDiff>,
    pub additions: u64,
    pub deletions: u64,
    pub truncated: bool,
}

// --- access tokens and the device flow -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenView {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub repositories: Option<Vec<String>>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
}

/// `POST /user/tokens` (201). `token` is the secret and is shown exactly once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedToken {
    pub token: String,
    #[serde(flatten)]
    pub view: TokenView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTokenGrant {
    pub access_token: String,
    pub token_type: String,
    pub username: String,
    pub scopes: Vec<String>,
}

// --- operator surface ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStats {
    pub users: u64,
    pub repositories: u64,
    pub public_repositories: u64,
    pub private_repositories: u64,
    pub revisions: u64,
    pub blobs: u64,
    pub stored_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub display_name: String,
    pub created_at: String,
    pub repository_count: u64,
}

/// Metadata only. An operator route never returns file contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminRepository {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub visibility: String,
    pub head: Option<String>,
    pub file_count: u64,
    pub size_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
}
