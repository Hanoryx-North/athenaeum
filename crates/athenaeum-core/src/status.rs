//! What the engine knows about itself: workspace status, index freshness, the
//! source inventory, and the index operations that keep them true.
//!
//! These are the four questions a client asks *about* the knowledge base rather
//! than of it — is there a vault here, how big is it, what has been read, and is
//! the derived index behind the files on disk. They are read-only apart from
//! [`index_scopes`], which is the explicit act of (re)building.
//!
//! Two rules run through everything here. First, an answer about staleness is
//! computed from metadata — a directory walk and a `stat` per file — never by
//! re-reading the sources, because these are called on a poll. Second, nothing
//! here is allowed to claim more than it checked: a file the scan did not read
//! is reported as `scanned: false` with a reason, and an index whose scan was
//! truncated says so instead of reporting a smaller, tidy-looking number.

use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::findings;
use crate::repository::{RepositoryIndexer, RepositoryScanOptions, RepositorySnapshot};
use crate::store::{KnowledgeStore, SourceRecord, SourceScope};
use crate::vault::{VaultIndexReport, VaultIndexer, VaultScanOptions};
use crate::wiki::WIKI_PROTOCOL_VERSION;
use crate::workspace::{Workspace, WORKSPACE_DIR};

/// The `meta` key holding when the vault index last ran.
pub(crate) const VAULT_INDEXED_AT: &str = "vault_indexed_at";
/// The `meta` key holding when the repository index last ran.
pub(crate) const REPOSITORY_INDEXED_AT: &str = "repository_indexed_at";
/// The `meta` key holding what the last vault scan did.
pub(crate) const VAULT_SCAN_STATE: &str = "vault_scan_state";
/// The `meta` key holding what the last repository scan did.
pub(crate) const REPOSITORY_SCAN_STATE: &str = "repository_scan_state";

/// A file a scan saw and did not read, and why.
///
/// Kept rather than folded into a warning string, because "how much of this
/// repository did you actually read?" is a question with a number for an
/// answer, and a warning log is not a number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    /// Path relative to the root of its scope, forward slashes.
    pub path: String,
    /// A stable machine-readable reason: `too_large`, `binary`, `budget`,
    /// `not_utf8`, `unreadable` or `not_indexable`.
    pub reason: String,
    /// The same thing said for a person.
    pub detail: String,
}

impl SkippedFile {
    pub fn new(path: impl Into<String>, reason: &str, detail: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.to_string(),
            detail: detail.into(),
        }
    }
}

/// What the last scan of a scope did, recorded so a later reader can tell an
/// incomplete scan from a complete one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    /// False when a budget stopped the walk before the tree ended.
    pub complete: bool,
    /// How many files were read into the index.
    pub scanned: usize,
    /// A bounded sample of what was skipped.
    pub skipped: Vec<SkippedFile>,
    /// How many files were skipped in total, which the sample may understate.
    pub skipped_total: usize,
    /// When the scan ran, RFC 3339.
    pub at: String,
}

impl Default for ScanState {
    fn default() -> Self {
        Self {
            complete: true,
            scanned: 0,
            skipped: Vec::new(),
            skipped_total: 0,
            at: String::new(),
        }
    }
}

/// Record what a scan of one scope did, and when.
pub fn record_scan_state(
    store: &KnowledgeStore,
    scope: SourceScope,
    state: &ScanState,
) -> Result<()> {
    let (state_key, at_key) = state_keys(scope);
    store.meta_set(state_key, &serde_json::to_string(state)?)?;
    store.meta_set(at_key, &state.at)?;
    Ok(())
}

/// What the last scan of one scope did, if one has ever run.
pub fn scan_state(store: &KnowledgeStore, scope: SourceScope) -> Result<Option<ScanState>> {
    let Some(raw) = store.meta_get(state_keys(scope).0)? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&raw)?))
}

fn state_keys(scope: SourceScope) -> (&'static str, &'static str) {
    match scope {
        SourceScope::Vault => (VAULT_SCAN_STATE, VAULT_INDEXED_AT),
        SourceScope::Repository => (REPOSITORY_SCAN_STATE, REPOSITORY_INDEXED_AT),
    }
}

/// The state of a workspace, as `status` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    /// The project directory the workspace belongs to: the one holding
    /// `.athenaeum/` and `wiki/`. Absolute, forward slashes.
    pub workspace_path: String,
    /// True when a knowledge base is open here. False is a state, not an error:
    /// a directory with no vault is a perfectly ordinary place to be standing.
    pub initialized: bool,
    /// True when the project holds a `wiki/` directory or indexed pages.
    pub has_vault: bool,
    pub vault_pages: u64,
    pub nodes: u64,
    pub edges: u64,
    pub proposals: u64,
    pub findings: u64,
    /// When the derived index last ran, RFC 3339, or null when it never has.
    pub indexed_at: Option<String>,
    /// True when something *has* been indexed and the sources have moved on
    /// since. Never-indexed is reported by `indexedAt: null`, not by this.
    pub index_stale: bool,
    pub protocol_version: u32,
    pub engine_version: String,
}

/// How fresh the derived index is, and what the last scans of each scope did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub vault_indexed_at: Option<String>,
    pub repository_indexed_at: Option<String>,
    /// The newer of the two, for a single "when was this built" line.
    pub indexed_at: Option<String>,
    pub vault_scan_incomplete: bool,
    pub repository_scan_incomplete: bool,
    /// True when an index exists and no longer matches the files it describes.
    pub stale: bool,
}

/// One file the engine has scanned, as `sources` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    /// Which scan produced this record. A path both scans read appears twice,
    /// once per scope, because the two records describe different things: a
    /// file of the code repository, and source material the wiki cites.
    pub scope: SourceScope,
    /// Path relative to the project root, forward slashes.
    pub path: String,
    pub bytes: u64,
    /// SHA-256 at scan time. Empty for a file that was deliberately not read.
    pub sha256: String,
    /// False when the scan saw this file and did not read it.
    pub scanned: bool,
    /// Graph nodes citing this file.
    pub nodes: u64,
    /// Evidence anchors pointing into this file.
    pub anchors: u64,
    pub indexed_at: Option<String>,
    /// True when the file is gone, or has changed since it was scanned. A
    /// `stat` comparison, deliberately: this is called on a poll and must not
    /// re-read the sources.
    pub stale: bool,
}

/// Which part of the project an index pass covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexScope {
    Vault,
    Repository,
    All,
}

impl IndexScope {
    pub const ALL: &'static [IndexScope] =
        &[IndexScope::Vault, IndexScope::Repository, IndexScope::All];

    pub fn as_str(&self) -> &'static str {
        match self {
            IndexScope::Vault => "vault",
            IndexScope::Repository => "repository",
            IndexScope::All => "all",
        }
    }
}

impl std::str::FromStr for IndexScope {
    type Err = crate::model::UnknownValue;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "vault" => Ok(IndexScope::Vault),
            "repository" => Ok(IndexScope::Repository),
            "all" => Ok(IndexScope::All),
            _ => Err(crate::model::UnknownValue {
                vocabulary: "IndexScope",
                value: value.to_string(),
            }),
        }
    }
}

/// What one index pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutcome {
    /// Pages the vault pass indexed. Zero when the scope excluded the vault.
    pub vault_pages: usize,
    /// Files the repository pass indexed. Zero when the scope excluded it.
    pub repository_files: usize,
    /// Declarations the repository pass found.
    pub symbols: usize,
    /// The graph's total relationship count after the pass.
    pub edges: u64,
    pub duration_ms: u64,
    /// True when any pass hit a budget before its tree ended.
    pub incomplete: bool,
    /// A bounded sample of the paths that were skipped, with duplicates across
    /// scopes removed. The reason for each is in the `incomplete_scan` review
    /// finding, which is where a person reads it.
    pub skipped: Vec<String>,
}

/// How many skipped paths an [`IndexOutcome`] carries. Enough to see the shape
/// of the problem, bounded so a pathological scan cannot return a book.
const MAX_REPORTED_SKIPPED: usize = 200;

impl WorkspaceStatus {
    /// The status of a directory that has no workspace at all.
    pub fn absent(requested_path: &Path) -> Self {
        Self {
            workspace_path: display_path(requested_path),
            initialized: false,
            has_vault: false,
            vault_pages: 0,
            nodes: 0,
            edges: 0,
            proposals: 0,
            findings: 0,
            indexed_at: None,
            index_stale: false,
            protocol_version: WIKI_PROTOCOL_VERSION,
            engine_version: crate::VERSION.to_string(),
        }
    }
}

/// The status of a workspace, or of a directory that has none.
///
/// `store` is `None` when no knowledge base was found, which is a normal state
/// rather than a failure: the caller gets `initialized: false` and a path, and
/// can render "no vault here" without treating a transport error as a fact
/// about the filesystem.
pub fn workspace_status(
    store: Option<&KnowledgeStore>,
    requested_path: &Path,
) -> Result<WorkspaceStatus> {
    let Some(store) = store else {
        return Ok(WorkspaceStatus::absent(requested_path));
    };
    let stats = store.stats()?;
    let index = index_status(store)?;
    let project = store.project_root();
    Ok(WorkspaceStatus {
        workspace_path: project
            .as_deref()
            .map(display_path)
            .unwrap_or_else(|| display_path(store.root())),
        initialized: true,
        has_vault: has_vault(store, project.as_deref())?,
        vault_pages: store.count_nodes_by_provenance_prefix(crate::vault::PAGE_PROVENANCE)?,
        nodes: stats.nodes,
        edges: stats.edges,
        proposals: store.count_proposals()?,
        findings: findings::count(store, &findings::FindingOptions::status())?,
        indexed_at: index.indexed_at,
        index_stale: index.stale,
        protocol_version: WIKI_PROTOCOL_VERSION,
        engine_version: crate::VERSION.to_string(),
    })
}

/// Find the workspace containing `requested_path`, open its knowledge base and
/// report its status. One call for a CLI command that only wants the answer.
pub fn workspace_status_at(requested_path: &Path) -> Result<WorkspaceStatus> {
    match Workspace::discover(requested_path) {
        Some(workspace) => {
            let store = workspace.open()?;
            workspace_status(Some(&store), requested_path)
        }
        None => Ok(WorkspaceStatus::absent(requested_path)),
    }
}

/// How fresh the derived index is.
pub fn index_status(store: &KnowledgeStore) -> Result<IndexStatus> {
    let vault_indexed_at = store.meta_get(VAULT_INDEXED_AT)?;
    let repository_indexed_at = store.meta_get(REPOSITORY_INDEXED_AT)?;
    let vault_state = scan_state(store, SourceScope::Vault)?;
    let repository_state = scan_state(store, SourceScope::Repository)?;
    let vault_scan_incomplete = vault_state.as_ref().is_some_and(|state| !state.complete);
    let repository_scan_incomplete = repository_state
        .as_ref()
        .is_some_and(|state| !state.complete);

    let vault_moved = match (store.project_root(), &vault_indexed_at) {
        (Some(project), Some(_)) => {
            match crate::vault::fingerprint(&project) {
                Ok(current) => {
                    store.meta_get(crate::vault::FINGERPRINT_KEY)?.as_deref()
                        != Some(current.as_str())
                }
                // A vault that cannot be walked cannot be compared. Reporting
                // "stale" would be a guess; the incomplete-scan finding is the
                // honest place for the failure, and it is raised separately.
                Err(_) => false,
            }
        }
        _ => false,
    };

    // The repository is compared through the inventory the last scan wrote,
    // one `stat` per file it read, rather than by walking the tree again: a
    // status call is cheap enough to make on every request, and a scan that
    // has to rediscover the tree is not. This catches a file that was edited,
    // deleted or replaced since it was read — the repairs and rewrites that
    // make a derived graph wrong. A file that did not exist when the scan ran
    // is caught by the scan itself, which notices an unread file as readily as
    // a changed one.
    let repository_moved = match (store.project_root(), &repository_indexed_at) {
        (Some(project), Some(_)) => store
            .list_sources(None)?
            .into_iter()
            .any(|(scope, record)| {
                scope == SourceScope::Repository && file_moved(&project, &record)
            }),
        _ => false,
    };

    Ok(IndexStatus {
        indexed_at: newest(vault_indexed_at.clone(), repository_indexed_at.clone()),
        vault_indexed_at,
        repository_indexed_at,
        vault_scan_incomplete,
        repository_scan_incomplete,
        stale: vault_moved
            || repository_moved
            || vault_scan_incomplete
            || repository_scan_incomplete,
    })
}

/// The recorded source inventory, with each file's freshness and how much of
/// the graph points at it.
pub fn source_summaries(store: &KnowledgeStore, limit: Option<u32>) -> Result<Vec<SourceSummary>> {
    let references = store.source_references()?;
    let project = store.project_root();
    let records = store.list_sources(limit)?;
    Ok(records
        .into_iter()
        .map(|(scope, record)| {
            let path = record.path.replace('\\', "/");
            let cited = references.get(&path).copied().unwrap_or_default();
            let stale = project
                .as_deref()
                .map(|project| file_moved(project, &record))
                .unwrap_or(false);
            SourceSummary {
                scope,
                path,
                bytes: record.bytes,
                sha256: record.sha256,
                scanned: record.scanned,
                nodes: cited.nodes,
                anchors: cited.anchors,
                indexed_at: Some(record.indexed_at),
                stale,
            }
        })
        .collect())
}

/// Rebuild the derived index for one scope, or for both.
///
/// `paths`, when given, restricts a **repository** pass to those
/// project-relative files. That is a targeted re-index, not a scan: it never
/// marks anything stale, because it did not see the whole tree and absence
/// would prove nothing. A vault pass always covers the whole vault — the vault
/// index reconciles what it did not see away, so a partial vault pass would be
/// a deletion, and the only honest answer to "index these two pages" is to
/// index the vault.
pub fn index_scopes(
    store: &mut KnowledgeStore,
    workspace: &Workspace,
    scope: IndexScope,
    paths: &[String],
) -> Result<IndexOutcome> {
    let started = Instant::now();
    let project = workspace
        .project_root()
        .ok_or_else(|| Error::InvalidValue {
            field: "workspace",
            value: format!(
                "{} is not a project workspace (its directory is not named {WORKSPACE_DIR})",
                workspace.root().display()
            ),
        })?;
    if !paths.is_empty() && scope == IndexScope::Vault {
        return Err(Error::InvalidValue {
            field: "paths",
            value: "a vault pass covers the whole vault; restrict a repository pass instead"
                .to_string(),
        });
    }

    let options = RepositoryScanOptions::default();
    let mut outcome = IndexOutcome::default();
    let mut skipped = Vec::new();

    if matches!(scope, IndexScope::Repository | IndexScope::All) {
        if paths.is_empty() {
            let report = RepositoryIndexer::scan_and_index(store, project, options)?;
            outcome.repository_files = report.files;
            outcome.symbols = report.symbols;
            outcome.incomplete |= !report.scan_complete;
            skipped.extend(report.skipped.iter().map(|file| file.path.clone()));
        } else {
            let mut scan = RepositorySnapshot::scan_detailed(project, options)?;
            // A path the scan cannot see is reported, not refused: the request
            // named it, and an answer that lists it in `skipped` tells the
            // caller what happened instead of failing the whole pass. A path
            // that escapes the project is still an error — that is a request to
            // index something outside the workspace, not a missing file.
            let unmatched = scan.restrict_to(paths)?;
            skipped.extend(unmatched.iter().cloned());
            let report = RepositoryIndexer::index_scan(store, project, scan)?;
            outcome.repository_files = report.files;
            outcome.symbols = report.symbols;
            outcome.incomplete |= !report.scan_complete;
            skipped.extend(report.skipped.iter().map(|file| file.path.clone()));
        }
    }

    if matches!(scope, IndexScope::Vault | IndexScope::All) {
        let report: VaultIndexReport =
            VaultIndexer::scan_and_index(store, project, VaultScanOptions::default())?;
        outcome.vault_pages = report.pages;
        outcome.incomplete |= !report.scan_complete;
        skipped.extend(report.skipped.iter().map(|file| file.path.clone()));
    }

    outcome.edges = store.stats()?.edges;
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    skipped.sort();
    skipped.dedup();
    skipped.truncate(MAX_REPORTED_SKIPPED);
    outcome.skipped = skipped;
    Ok(outcome)
}

/// True when a `wiki/` directory is present, or pages are already indexed.
///
/// Both halves matter: a vault whose pages were all deleted still has an index
/// until it is reconciled, and a vault that was just created has a directory
/// and no pages yet.
fn has_vault(store: &KnowledgeStore, project: Option<&Path>) -> Result<bool> {
    if project.is_some_and(|project| project.join("wiki").is_dir()) {
        return Ok(true);
    }
    Ok(store.count_nodes_by_provenance_prefix(crate::vault::PAGE_PROVENANCE)? > 0)
}

/// Whether a recorded file is gone or has changed since it was recorded.
fn file_moved(project: &Path, record: &SourceRecord) -> bool {
    let Ok(metadata) = std::fs::metadata(project.join(&record.path)) else {
        return true;
    };
    if metadata.len() != record.bytes {
        return true;
    }
    modification_time(&metadata) != record.mtime_ns
}

/// Modification time in nanoseconds since the Unix epoch, `0` when the
/// filesystem will not say — which compares equal to a record that also had
/// nothing to compare, rather than reporting every such file as changed.
pub(crate) fn modification_time(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos() as i64)
        .unwrap_or(0)
}

/// The later of two RFC 3339 timestamps, falling back to the first that parses.
fn newest(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let parsed_left = chrono::DateTime::parse_from_rfc3339(&left);
            let parsed_right = chrono::DateTime::parse_from_rfc3339(&right);
            match (parsed_left, parsed_right) {
                (Ok(a), Ok(b)) if b > a => Some(right),
                (Err(_), Ok(_)) => Some(right),
                _ => Some(left),
            }
        }
        (Some(left), None) => Some(left),
        (None, right) => right,
    }
}

/// A path as the wire carries it: absolute where it can be, forward slashes.
fn display_path(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}
