//! Commands that ask about the knowledge base rather than of it.
//!
//! What is here, what has been read, and whether the derived index is behind the files on disk.
//! These are the questions a person asks before trusting an answer, and the ones a script asks
//! before deciding whether to re-index. The answers come from [`athenaeum_core::status`], the
//! same code the bridge's `status`, `sources` and `index` methods answer from, so the CLI and an
//! external client cannot disagree about them.

use std::path::Path;

use athenaeum_core::findings;
use athenaeum_core::prelude::*;
use athenaeum_core::status::{self, IndexScope};
use clap::ValueEnum;

use crate::render::{self, emit};
use crate::Outcome;

/// Which part of the project `athenaeum index --scope` covers.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ScopeArg {
    /// `wiki/` and `raw/sources/` only.
    Vault,
    /// Files, symbols, imports and Git history of the repository only.
    Repository,
    /// Both, repository first.
    All,
}

impl From<ScopeArg> for IndexScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Vault => IndexScope::Vault,
            ScopeArg::Repository => IndexScope::Repository,
            ScopeArg::All => IndexScope::All,
        }
    }
}

/// The workspace and index state.
///
/// A directory with no `.athenaeum/` is an answer, not a failure: the command reports
/// `initialized: false` and exits zero, and every other command reports `workspace_not_found`.
pub fn status(json: bool, cwd: &Path) -> Result<Outcome> {
    let report = status::workspace_status_at(cwd)?;
    emit(json, &report, || {
        render::workspace_status(&report);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

/// Every file the index has read — sources under `raw/sources/` and repository files — with its
/// hash, how much of the graph points at it, and whether it moved since.
///
/// The listing describes the last scan, so a stale index is named on stderr rather than quietly
/// refreshed: "what was read" and "what is there now" are different questions, and this answers
/// the first.
pub fn sources(json: bool, store: &KnowledgeStore, limit: u32) -> Result<Outcome> {
    crate::note_if_index_stale(store);
    let sources = status::source_summaries(store, Some(limit))?;
    emit(json, &sources, || {
        render::sources(&sources, limit);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

/// The review queue: what in the knowledge base needs a decision.
///
/// One value, two renderings, exactly as everywhere else: `--json` prints the pinned
/// `review` finding objects and the terminal prints the same facts. The command exits non-zero
/// only when a finding could hand an agent something wrong, unless `--strict` says every finding
/// counts.
pub fn review(json: bool, store: &KnowledgeStore, limit: u32, strict: bool) -> Result<Outcome> {
    let findings = findings::review_findings(store, Some(limit))?;
    let blocking = findings.iter().any(|finding| finding.blocking);
    let total = findings.len();
    emit(json, &findings, || {
        render::review_findings(&findings, limit);
        Ok(())
    })?;
    if blocking || (strict && total > 0) {
        return Ok(Outcome::ProblemsFound);
    }
    Ok(Outcome::Ok)
}

/// Index the vault, the repository, or both.
pub fn index(
    json: bool,
    store: &mut KnowledgeStore,
    workspace: &Workspace,
    scope: ScopeArg,
    paths: &[String],
) -> Result<Outcome> {
    let outcome = status::index_scopes(store, workspace, scope.into(), paths)?;
    emit(json, &outcome, || {
        render::index_outcome(&outcome);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}
