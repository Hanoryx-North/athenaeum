//! The sync engine: push, pull and conflict handling over a [`Remote`].
//!
//! The engine owns the *policy* — what may be overwritten and what may not — and none of the
//! transport. A [`Remote`] is a handful of calls a server answers; the CLI implements it over HTTP
//! and the tests implement it in memory, so every rule below is exercised without a network.
//!
//! The rules, in the order they matter:
//!
//! 1. **Nothing local is overwritten by a pull unless it is unchanged since the last sync.** A file
//!    edited on both sides is merged line by line if the edits do not overlap, and otherwise left
//!    exactly as it is and reported as a conflict.
//! 2. **A conflict blocks push.** Until each conflicted path is resolved, nothing is sent, so a
//!    divergent edit cannot be silently replaced by uploading the local one.
//! 3. **A pull is all or nothing.** Files are staged and verified against the hashes the manifest
//!    names, a journal records the decision, and only then are they moved into place. A crash
//!    between those steps is completed on the next run. The last-synced revision advances last.
//! 4. **The server is not trusted.** Every path it names is validated before anything is written,
//!    every body is hashed against the manifest, and the tree hash of every revision is recomputed.
//! 5. **Offline is a state, not a failure of the repository.** Every call that needs the server
//!    reports `offline` and changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::ledger::{sync_dir, Origin, TrustLedger};
use super::lock::RepoLock;
use super::manifest::{RepositoryManifest, FORMAT_VERSION, MANIFEST_FILE};
use super::merge::{self, Merged};
use super::syncset::{self, sha256_hex, ManifestEntry, Skipped, SyncLimits};
use super::{portable_error, write_atomic};
use crate::error::Result;
use crate::workspace::WORKSPACE_DIR;

/// The sync protocol this engine speaks.
pub const SYNC_PROTOCOL: u32 = 1;

// ---------------------------------------------------------------------------
// The remote, as the engine sees it.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub service: String,
    pub api_version: u32,
    pub sync_protocol: u32,
    #[serde(default)]
    pub limits: Option<SyncLimits>,
}

/// Who owns a remote repository. The server sends a whole profile; only the name is needed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOwner {
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepository {
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
pub struct RemoteRevision {
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
    /// The id the caller would like the repository to have; the server keeps it if it is free.
    pub id: Option<String>,
    pub name: String,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// What the engine needs from a Library server.
///
/// An implementation reports an unreachable server as a [`crate::Error::Portable`] with code
/// `offline`, a rejected push on a stale base as `stale_base`, and passes the server's own error
/// codes through unchanged.
pub trait Remote {
    fn info(&self) -> Result<ServerInfo>;
    fn create_repository(&self, request: &CreateRepository) -> Result<RemoteRepository>;
    fn repository(&self, id: &str) -> Result<RemoteRepository>;
    fn revision(&self, id: &str, revision: &str) -> Result<RemoteRevision>;
    fn blob(&self, id: &str, sha256: &str) -> Result<Vec<u8>>;
    fn push(&self, id: &str, request: &PushRequest) -> Result<RemoteRevision>;
}

// ---------------------------------------------------------------------------
// Local sync state.
// ---------------------------------------------------------------------------

/// Where a repository is in its relationship with one remote. Local only; never synced.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub server: String,
    pub remote_id: String,
    #[serde(default)]
    pub remote_name: Option<String>,
    /// The remote revision this repository last agreed with.
    #[serde(default)]
    pub last_synced: Option<String>,
    /// The manifest of `last_synced`: what both sides held when they last agreed.
    #[serde(default)]
    pub base: Vec<ManifestEntry>,
    /// Paths whose divergent edits are waiting for a person to decide.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// The revision the conflicting remote versions came from.
    #[serde(default)]
    pub conflict_revision: Option<String>,
    /// A remote revision a pull merged, to record on the next push.
    #[serde(default)]
    pub merged_from: Option<String>,
    #[serde(default)]
    pub last_sync_at: Option<String>,
}

impl SyncState {
    fn path(root: &Path) -> Result<PathBuf> {
        Ok(sync_dir(root).join("state.json"))
    }

    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = Self::path(root)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(&Self::path(root)?, text.as_bytes())
    }

    fn require(root: &Path) -> Result<Self> {
        Self::load(root)?.ok_or_else(|| {
            portable_error(
                "not_linked",
                "this repository is not linked to a Library; run `athenaeum sync link` first",
            )
        })
    }
}

/// Link a repository to a remote one. Creates `athenaeum.json` when the vault has none.
pub fn link(root: &Path, server: &str, repository: &RemoteRepository) -> Result<SyncState> {
    let mut state = SyncState::load(root)?.unwrap_or_default();
    if !state.remote_id.is_empty() && state.remote_id != repository.id {
        return Err(portable_error(
            "conflict",
            format!(
                "this repository is already linked to {}; unlink it first",
                state.remote_id
            ),
        ));
    }
    match RepositoryManifest::read(root)? {
        Some(manifest) if manifest.repository_id != repository.id => {
            return Err(portable_error(
                "conflict",
                format!(
                    "this vault is {} but the Library repository is {}; clone it into a new directory instead",
                    manifest.repository_id, repository.id
                ),
            ));
        }
        None => {
            let mut manifest = RepositoryManifest::new(repository.name.as_str());
            manifest.repository_id = repository.id.clone();
            manifest.write(root)?;
        }
        Some(_) => {}
    }
    state.server = server.to_string();
    state.remote_id = repository.id.clone();
    state.remote_name = Some(repository.name.clone());
    state.save(root)?;
    Ok(state)
}

/// Forget the link. Files, history and the trust ledger are untouched.
pub fn unlink(root: &Path) -> Result<bool> {
    let path = SyncState::path(root)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

// ---------------------------------------------------------------------------
// Results.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
    pub fn len(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPosition {
    NotLinked,
    Offline,
    Conflicted,
    UpToDate,
    /// Local changes waiting to be pushed.
    Ahead,
    /// Remote changes waiting to be pulled.
    Behind,
    /// Both.
    Diverged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub position: SyncPosition,
    pub server: Option<String>,
    pub remote_id: Option<String>,
    pub last_synced: Option<String>,
    pub remote_head: Option<String>,
    pub pending: Pending,
    pub conflicts: Vec<String>,
    pub skipped: Vec<Skipped>,
    pub reachable: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullOutcome {
    pub up_to_date: bool,
    pub revision: Option<String>,
    /// Files written or replaced from the remote.
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    /// Files both sides edited that merged without overlap.
    pub merged: Vec<String>,
    pub conflicts: Vec<String>,
    /// True when files changed, so the caller should rebuild the local index.
    pub changed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushOutcome {
    pub nothing_to_push: bool,
    pub revision: Option<String>,
    pub pushed: Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Local,
    Remote,
    Both,
}

/// Local snapshots for human conflict review. A missing side means that side deleted the file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPreview {
    pub path: String,
    pub base: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
}

// ---------------------------------------------------------------------------
// The engine.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SyncEngine<'a, R: Remote + ?Sized> {
    root: &'a Path,
    remote: &'a R,
    limits: SyncLimits,
}

impl<'a, R: Remote + ?Sized> SyncEngine<'a, R> {
    pub fn new(root: &'a Path, remote: &'a R) -> Self {
        Self {
            root,
            remote,
            limits: SyncLimits::default(),
        }
    }

    /// Use the limits the server advertised instead of the defaults.
    pub fn with_limits(mut self, limits: SyncLimits) -> Self {
        self.limits = limits;
        self
    }

    fn workspace(&self) -> PathBuf {
        self.root.join(WORKSPACE_DIR)
    }

    /// Refuse a server that speaks a sync protocol this engine does not.
    pub fn check_compatibility(&self) -> Result<ServerInfo> {
        let info = self.remote.info()?;
        if info.sync_protocol != SYNC_PROTOCOL {
            return Err(portable_error(
                "unsupported_protocol",
                format!(
                    "the server speaks sync protocol {}; this client speaks {SYNC_PROTOCOL}",
                    info.sync_protocol
                ),
            ));
        }
        Ok(info)
    }

    /// Where this repository stands. Never writes; works offline.
    pub fn status(&self) -> Result<SyncStatus> {
        let Some(state) = SyncState::load(self.root)? else {
            return Ok(SyncStatus {
                position: SyncPosition::NotLinked,
                server: None,
                remote_id: None,
                last_synced: None,
                remote_head: None,
                pending: Pending::default(),
                conflicts: Vec::new(),
                skipped: Vec::new(),
                reachable: false,
                detail: Some("not linked to a Library".into()),
            });
        };
        let local = syncset::scan(self.root, &self.limits)?;
        let pending = diff_pending(&state.base, &local.entries);
        let (remote_head, reachable, detail) = match self.remote.repository(&state.remote_id) {
            Ok(repo) => (repo.head, true, None),
            Err(error) => (None, false, Some(error.to_string())),
        };
        let behind = reachable && remote_head != state.last_synced;
        let ahead = !pending.is_empty();
        let position = if !state.conflicts.is_empty() {
            SyncPosition::Conflicted
        } else if !reachable {
            SyncPosition::Offline
        } else {
            match (ahead, behind) {
                (false, false) => SyncPosition::UpToDate,
                (true, false) => SyncPosition::Ahead,
                (false, true) => SyncPosition::Behind,
                (true, true) => SyncPosition::Diverged,
            }
        };
        Ok(SyncStatus {
            position,
            server: Some(state.server),
            remote_id: Some(state.remote_id),
            last_synced: state.last_synced,
            remote_head,
            pending,
            conflicts: state.conflicts,
            skipped: local.skipped,
            reachable,
            detail,
        })
    }

    /// Bring remote changes into the working tree without losing local ones.
    pub fn pull(&self) -> Result<PullOutcome> {
        let _lock = RepoLock::acquire(&self.workspace(), "vault")?;
        self.recover()?;
        let mut state = SyncState::require(self.root)?;
        if !state.conflicts.is_empty() {
            return Err(portable_error(
                "conflict",
                format!(
                    "{} path(s) are waiting for a decision: {}. Resolve them with \
                     `athenaeum sync resolve <path> --keep local|remote|both`",
                    state.conflicts.len(),
                    state.conflicts.join(", ")
                ),
            ));
        }
        self.check_compatibility()?;

        let repository = self.remote.repository(&state.remote_id)?;
        let Some(head) = repository.head else {
            return Ok(PullOutcome {
                up_to_date: true,
                ..Default::default()
            });
        };
        if state.last_synced.as_deref() == Some(head.as_str()) {
            return Ok(PullOutcome {
                up_to_date: true,
                revision: Some(head),
                ..Default::default()
            });
        }

        let revision = self.remote.revision(&state.remote_id, &head)?;
        verify_revision(&revision)?;
        let remote_map = to_map(&revision.manifest);
        let base_map = to_map(&state.base);
        let local_set = syncset::scan(self.root, &self.limits)?;
        let local_map = to_map(&local_set.entries);

        let paths: BTreeSet<&str> = base_map
            .keys()
            .chain(local_map.keys())
            .chain(remote_map.keys())
            .copied()
            .collect();

        let mut plan = Plan::default();
        for path in paths {
            let b = base_map.get(path);
            let l = local_map.get(path);
            let r = remote_map.get(path);
            if l == r || r == b {
                // Converged already, or the remote did not touch it: nothing to bring in.
                continue;
            }
            if l == b {
                // Unchanged locally, changed remotely: take the remote.
                match r {
                    Some(sha) => plan.writes.push(PlannedWrite {
                        path: path.to_string(),
                        bytes: self.fetch(&state.remote_id, path, sha)?,
                        sha: (*sha).clone(),
                        merged: false,
                    }),
                    None => plan.deletes.push(path.to_string()),
                }
                continue;
            }
            // The manifest is the one file where "both sides changed it" is not a dispute: it names
            // the repository, and the repository is the remote's. Taking the remote's keeps every
            // device that syncs with it agreeing on who it is.
            if path == MANIFEST_FILE {
                if let Some(sha) = r {
                    plan.writes.push(PlannedWrite {
                        path: path.to_string(),
                        bytes: self.fetch(&state.remote_id, path, sha)?,
                        sha: (*sha).clone(),
                        merged: false,
                    });
                }
                continue;
            }

            // Changed on both sides, differently.
            let remote_bytes = match r {
                Some(sha) => Some(self.fetch(&state.remote_id, path, sha)?),
                None => None,
            };
            let local_bytes = match l {
                Some(_) => Some(std::fs::read(self.root.join(path))?),
                None => None,
            };
            let base_text = match b {
                Some(sha) => Some(self.fetch(&state.remote_id, path, sha)?),
                None => None,
            };
            let (Some(local_bytes), Some(remote_bytes)) = (&local_bytes, &remote_bytes) else {
                // Edited on one side, deleted on the other. Never resolved by software.
                plan.conflicts.push(PlannedConflict {
                    path: path.to_string(),
                    remote: remote_bytes,
                    base: base_text,
                });
                continue;
            };
            let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
            let base_string = base_text.as_deref().map(text).unwrap_or_default();
            match merge::merge3(&base_string, &text(local_bytes), &text(remote_bytes)) {
                Merged::Clean(merged) => {
                    let bytes = merged.into_bytes();
                    plan.writes.push(PlannedWrite {
                        path: path.to_string(),
                        sha: sha256_hex(&bytes),
                        bytes,
                        merged: true,
                    });
                }
                Merged::Conflicted => plan.conflicts.push(PlannedConflict {
                    path: path.to_string(),
                    remote: Some(remote_bytes.clone()),
                    base: base_text,
                }),
            }
        }

        // Conflicting remote versions are set aside before anything else changes.
        for conflict in &plan.conflicts {
            let dir = self.conflict_dir(&head);
            if let Some(bytes) = &conflict.remote {
                write_atomic(&dir.join("remote").join(&conflict.path), bytes)?;
            }
            if let Some(bytes) = &conflict.base {
                write_atomic(&dir.join("base").join(&conflict.path), bytes)?;
            }
        }

        let outcome = PullOutcome {
            up_to_date: false,
            revision: Some(head.clone()),
            updated: plan
                .writes
                .iter()
                .filter(|w| !w.merged)
                .map(|w| w.path.clone())
                .collect(),
            merged: plan
                .writes
                .iter()
                .filter(|w| w.merged)
                .map(|w| w.path.clone())
                .collect(),
            deleted: plan.deletes.clone(),
            conflicts: plan.conflicts.iter().map(|c| c.path.clone()).collect(),
            changed: !plan.writes.is_empty() || !plan.deletes.is_empty(),
        };

        // Recorded on the next push only when this pull actually joined two lines of history.
        let had_local_changes = !diff_pending(&state.base, &local_set.entries).is_empty();
        state.last_synced = Some(head.clone());
        state.base = revision.manifest.clone();
        state.conflicts = outcome.conflicts.clone();
        state.conflict_revision = (!outcome.conflicts.is_empty()).then(|| head.clone());
        state.merged_from = had_local_changes.then(|| head.clone());
        state.last_sync_at = Some(now());

        self.commit(plan, state)?;
        Ok(outcome)
    }

    /// Send local changes. Refuses while a conflict is open or the remote has moved.
    pub fn push(&self, message: &str) -> Result<PushOutcome> {
        let _lock = RepoLock::acquire(&self.workspace(), "vault")?;
        self.recover()?;
        let mut state = SyncState::require(self.root)?;
        if !state.conflicts.is_empty() {
            return Err(portable_error(
                "conflict",
                format!(
                    "refusing to push while {} conflict(s) are unresolved: {}",
                    state.conflicts.len(),
                    state.conflicts.join(", ")
                ),
            ));
        }
        self.check_compatibility()?;

        let repository = self.remote.repository(&state.remote_id)?;
        if repository.head != state.last_synced {
            return Err(portable_error(
                "stale_base",
                "the remote has revisions this repository has not pulled; run `athenaeum sync pull` first",
            ));
        }

        let local = syncset::scan(self.root, &self.limits)?;
        let pending = diff_pending(&state.base, &local.entries);
        if pending.is_empty() {
            return Ok(PushOutcome {
                nothing_to_push: true,
                revision: state.last_synced,
                pushed: pending,
            });
        }

        let mut changes = Vec::with_capacity(pending.len());
        for path in pending.added.iter().chain(&pending.modified) {
            let bytes = std::fs::read(self.root.join(path))?;
            let content = String::from_utf8(bytes)
                .map_err(|_| portable_error("invalid_content", format!("{path} is not UTF-8")))?;
            changes.push(Change {
                path: path.clone(),
                content: Some(content),
                delete: false,
            });
        }
        for path in &pending.deleted {
            changes.push(Change {
                path: path.clone(),
                content: None,
                delete: true,
            });
        }

        let request = PushRequest {
            base: state.last_synced.clone(),
            message: message.to_string(),
            changes,
            merged_from: state.merged_from.clone(),
        };
        let revision = self.remote.push(&state.remote_id, &request)?;

        // The server computed a tree from the base and these changes. It must be the tree we hold.
        if revision.tree_hash != local.tree_hash() {
            return Err(portable_error(
                "integrity",
                "the server's tree does not match the files that were sent; nothing was recorded \
                 locally, run `athenaeum sync status` and pull",
            ));
        }

        let mut ledger = TrustLedger::load(self.root)?;
        for path in pending
            .added
            .iter()
            .chain(&pending.modified)
            .chain(&pending.deleted)
        {
            ledger.forget(path);
        }
        ledger.save(self.root)?;

        state.last_synced = Some(revision.id.clone());
        state.base = local.entries;
        state.merged_from = None;
        state.last_sync_at = Some(now());
        state.save(self.root)?;

        Ok(PushOutcome {
            nothing_to_push: false,
            revision: Some(revision.id),
            pushed: pending,
        })
    }

    /// Inspect a conflict without contacting the Library or changing any files.
    pub fn conflict(&self, path: &str) -> Result<ConflictPreview> {
        let _lock = RepoLock::acquire(&self.workspace(), "vault")?;
        let state = SyncState::require(self.root)?;
        if !state.conflicts.iter().any(|p| p == path) {
            return Err(portable_error(
                "not_found",
                format!("{path} is not in conflict"),
            ));
        }
        syncset::validate_path(path).map_err(|r| portable_error("invalid_path", r))?;
        let dir = self.conflict_dir(state.conflict_revision.as_deref().unwrap_or_default());
        let read = |target: PathBuf| -> Result<Option<String>> {
            use std::io::Read;
            ensure_inside(self.root, &target)?;
            let file = match std::fs::File::open(&target) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let mut bytes = Vec::new();
            file.take(self.limits.max_file_bytes + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > self.limits.max_file_bytes {
                return Err(portable_error(
                    "invalid_content",
                    "conflict page exceeds the per-file limit",
                ));
            }
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| portable_error("invalid_content", "conflict page is not UTF-8"))
        };
        Ok(ConflictPreview {
            path: path.to_string(),
            local: read(self.root.join(path))?,
            base: read(dir.join("base").join(path))?,
            remote: read(dir.join("remote").join(path))?,
        })
    }

    /// Settle one conflicted path.
    pub fn resolve(&self, path: &str, keep: Keep) -> Result<()> {
        let _lock = RepoLock::acquire(&self.workspace(), "vault")?;
        let mut state = SyncState::require(self.root)?;
        if !state.conflicts.iter().any(|p| p == path) {
            return Err(portable_error(
                "not_found",
                format!("{path} is not in conflict"),
            ));
        }
        syncset::validate_path(path).map_err(|r| portable_error("invalid_path", r))?;
        let revision = state.conflict_revision.clone().unwrap_or_default();
        let dir = self.conflict_dir(&revision);
        let target = self.root.join(path);
        let remote_copy = std::fs::read(dir.join("remote").join(path)).ok();
        let base_copy = std::fs::read(dir.join("base").join(path)).ok();
        let local_copy = std::fs::read(&target).ok();

        match keep {
            Keep::Local => {
                if let Some(bytes) = &local_copy {
                    if merge::has_conflict_markers(&String::from_utf8_lossy(bytes)) {
                        return Err(portable_error(
                            "conflict",
                            format!(
                                "{path} still contains conflict markers; edit them out, then keep local"
                            ),
                        ));
                    }
                }
                state.conflicts.retain(|p| p != path);
            }
            Keep::Remote => {
                ensure_inside(self.root, &target)?;
                let mut ledger = TrustLedger::load(self.root)?;
                match &remote_copy {
                    Some(bytes) => {
                        write_atomic(&target, bytes)?;
                        ledger.record(path, &sha256_hex(bytes), Origin::Pulled);
                    }
                    None => {
                        let _ = std::fs::remove_file(&target);
                        ledger.forget(path);
                    }
                }
                ledger.save(self.root)?;
                state.conflicts.retain(|p| p != path);
            }
            Keep::Both => {
                let (Some(local), Some(remote)) = (&local_copy, &remote_copy) else {
                    return Err(portable_error(
                        "invalid_request",
                        "keeping both needs a local and a remote version; the other side deleted this file",
                    ));
                };
                ensure_inside(self.root, &target)?;
                let text = |bytes: &Vec<u8>| String::from_utf8_lossy(bytes).into_owned();
                let marked = merge::merge3_with_markers(
                    &base_copy.as_ref().map(text).unwrap_or_default(),
                    &text(local),
                    &text(remote),
                    ("local", "remote"),
                );
                write_atomic(&target, marked.as_bytes())?;
                // Stays in conflict: the markers have to be edited out, then `--keep local`.
            }
        }
        state.save(self.root)?;
        if state.conflicts.is_empty() {
            let _ = std::fs::remove_dir_all(self.workspace().join("conflicts"));
        }
        Ok(())
    }

    /// Pull the whole of a remote repository into an empty directory and link it.
    pub fn clone_into(&self, repository: &RemoteRepository, server: &str) -> Result<PullOutcome> {
        self.check_compatibility()?;
        let Some(head) = repository.head.clone() else {
            link(self.root, server, repository)?;
            return Ok(PullOutcome {
                up_to_date: true,
                ..Default::default()
            });
        };
        let revision = self.remote.revision(&repository.id, &head)?;
        verify_revision(&revision)?;
        let mut plan = Plan::default();
        for entry in &revision.manifest {
            plan.writes.push(PlannedWrite {
                path: entry.path.clone(),
                bytes: self.fetch(&repository.id, &entry.path, &entry.sha256)?,
                sha: entry.sha256.clone(),
                merged: false,
            });
        }
        let updated = plan
            .writes
            .iter()
            .map(|w| w.path.clone())
            .collect::<Vec<_>>();

        // Files first, then the identity they carry, so the state lands under the right id.
        std::fs::create_dir_all(self.workspace())?;
        let _lock = RepoLock::acquire(&self.workspace(), "vault")?;
        let staged_state = SyncState {
            server: server.to_string(),
            remote_id: repository.id.clone(),
            remote_name: Some(repository.name.clone()),
            last_synced: Some(head.clone()),
            base: revision.manifest.clone(),
            last_sync_at: Some(now()),
            ..Default::default()
        };
        let had_manifest = plan.writes.iter().any(|w| w.path == MANIFEST_FILE);
        if !had_manifest {
            let mut manifest = RepositoryManifest::new(repository.name.as_str());
            manifest.repository_id = repository.id.clone();
            manifest.write(self.root)?;
        }
        self.commit_with_manifest(plan, staged_state)?;
        Ok(PullOutcome {
            up_to_date: false,
            revision: Some(head),
            changed: !updated.is_empty(),
            updated,
            ..Default::default()
        })
    }

    // -----------------------------------------------------------------------

    fn fetch(&self, remote_id: &str, path: &str, sha: &str) -> Result<Vec<u8>> {
        syncset::validate_path(path).map_err(|reason| {
            portable_error(
                "invalid_path",
                format!("the server named an unsafe path {path:?}: {reason}"),
            )
        })?;
        let bytes = self.remote.blob(remote_id, sha)?;
        if sha256_hex(&bytes) != sha {
            return Err(portable_error(
                "integrity",
                format!("{path} does not match the hash its revision names; nothing was written"),
            ));
        }
        syncset::validate_content(&bytes, &self.limits)
            .map_err(|reason| portable_error("invalid_content", format!("{path}: {reason}")))?;
        if path == MANIFEST_FILE {
            let manifest: RepositoryManifest = serde_json::from_slice(&bytes).map_err(|error| {
                portable_error("invalid_content", format!("{MANIFEST_FILE}: {error}"))
            })?;
            if manifest.repository_id != remote_id
                || manifest.format_version != FORMAT_VERSION
                || !manifest.extra.is_empty()
            {
                return Err(portable_error(
                    "invalid_content",
                    format!(
                        "{MANIFEST_FILE} contains an unexpected identity, format, or extra fields"
                    ),
                ));
            }
        }
        Ok(bytes)
    }

    fn conflict_dir(&self, revision: &str) -> PathBuf {
        let safe: String = revision
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        self.workspace().join("conflicts").join(safe)
    }

    fn journal_path(&self) -> PathBuf {
        self.workspace().join("sync").join("journal.json")
    }

    fn staging_dir(&self) -> PathBuf {
        self.workspace().join("sync").join("staging")
    }

    /// Stage, journal, apply. The journal write is the commit point.
    fn commit(&self, plan: Plan, state: SyncState) -> Result<()> {
        self.commit_with_manifest(plan, state)
    }

    fn commit_with_manifest(&self, plan: Plan, state: SyncState) -> Result<()> {
        let staging = self.staging_dir();
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;

        let mut ops = Vec::new();
        for (index, write) in plan.writes.iter().enumerate() {
            ensure_inside(self.root, &self.root.join(&write.path))?;
            let name = index.to_string();
            write_atomic(&staging.join(&name), &write.bytes)?;
            ops.push(JournalOp {
                path: write.path.clone(),
                staged: Some(name),
                sha256: Some(write.sha.clone()),
            });
        }
        for path in &plan.deletes {
            ensure_inside(self.root, &self.root.join(path))?;
            ops.push(JournalOp {
                path: path.clone(),
                staged: None,
                sha256: None,
            });
        }
        let journal = Journal { ops, state };
        write_atomic(
            &self.journal_path(),
            serde_json::to_string(&journal)?.as_bytes(),
        )?;
        self.apply(&journal)
    }

    fn apply(&self, journal: &Journal) -> Result<()> {
        let staging = self.staging_dir();
        for op in &journal.ops {
            let target = self.root.join(&op.path);
            ensure_inside(self.root, &target)?;
            match &op.staged {
                Some(name) => {
                    let staged = staging.join(name);
                    if !staged.exists() {
                        continue; // already moved by a run that was interrupted
                    }
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if std::fs::rename(&staged, &target).is_err() {
                        std::fs::copy(&staged, &target)?;
                        std::fs::remove_file(&staged)?;
                    }
                }
                None => match std::fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
        }

        let mut ledger = TrustLedger::load(self.root)?;
        for op in &journal.ops {
            match (&op.staged, &op.sha256) {
                (Some(_), Some(sha)) => ledger.record(&op.path, sha, Origin::Pulled),
                _ => {
                    ledger.forget(&op.path);
                }
            }
        }
        ledger.save(self.root)?;

        // Advancing the last-synced revision is the last step: the state never claims a revision
        // the working tree does not hold.
        journal.state.save(self.root)?;
        let _ = std::fs::remove_file(self.journal_path());
        let _ = std::fs::remove_dir_all(&staging);
        Ok(())
    }

    /// Finish a pull that was interrupted after its journal was written.
    fn recover(&self) -> Result<()> {
        let path = self.journal_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            let _ = std::fs::remove_dir_all(self.staging_dir());
            return Ok(());
        };
        match serde_json::from_str::<Journal>(&text) {
            Ok(journal) => self.apply(&journal),
            Err(_) => {
                // A journal that does not parse was never committed; drop it and its staging.
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir_all(self.staging_dir());
                Ok(())
            }
        }
    }
}

#[derive(Debug, Default)]
struct Plan {
    writes: Vec<PlannedWrite>,
    deletes: Vec<String>,
    conflicts: Vec<PlannedConflict>,
}

#[derive(Debug)]
struct PlannedWrite {
    path: String,
    bytes: Vec<u8>,
    sha: String,
    merged: bool,
}

#[derive(Debug)]
struct PlannedConflict {
    path: String,
    remote: Option<Vec<u8>>,
    base: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    ops: Vec<JournalOp>,
    state: SyncState,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalOp {
    path: String,
    staged: Option<String>,
    sha256: Option<String>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn to_map(entries: &[ManifestEntry]) -> BTreeMap<&str, &String> {
    entries
        .iter()
        .map(|entry| (entry.path.as_str(), &entry.sha256))
        .collect()
}

fn diff_pending(base: &[ManifestEntry], local: &[ManifestEntry]) -> Pending {
    let base = to_map(base);
    let local_map = to_map(local);
    let mut pending = Pending::default();
    for (path, sha) in &local_map {
        match base.get(path) {
            None => pending.added.push((*path).to_string()),
            Some(old) if old != sha => pending.modified.push((*path).to_string()),
            Some(_) => {}
        }
    }
    for path in base.keys() {
        if !local_map.contains_key(path) {
            pending.deleted.push((*path).to_string());
        }
    }
    pending
}

/// A revision the server describes must hash to the tree it names, and name only safe paths.
fn verify_revision(revision: &RemoteRevision) -> Result<()> {
    for entry in &revision.manifest {
        syncset::validate_path(&entry.path).map_err(|reason| {
            portable_error(
                "invalid_path",
                format!(
                    "revision {} names an unsafe path {:?}: {reason}",
                    revision.id, entry.path
                ),
            )
        })?;
        if !syncset::has_sync_extension(&entry.path) {
            return Err(portable_error(
                "invalid_path",
                format!(
                    "revision {} names {:?}, which is not accepted wiki Markdown or repository identity",
                    revision.id, entry.path
                ),
            ));
        }
    }
    if syncset::tree_hash(&revision.manifest) != revision.tree_hash {
        return Err(portable_error(
            "integrity",
            format!(
                "revision {} does not hash to its own tree; the server's answer cannot be trusted",
                revision.id
            ),
        ));
    }
    Ok(())
}

/// Refuse to write through a symbolic link: a link inside the repository could point anywhere.
fn ensure_inside(root: &Path, target: &Path) -> Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| {
        portable_error(
            "invalid_path",
            format!("{} is outside the repository", target.display()),
        )
    })?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component);
        match std::fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(portable_error(
                    "invalid_path",
                    format!(
                        "{} is a symbolic link; refusing to write through it",
                        cursor.display()
                    ),
                ))
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
