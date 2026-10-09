//! The source inventory: which files the engine read, and how they looked then.
//!
//! Two callers need this and neither can afford to re-derive it. The `sources`
//! report answers "what has this engine actually seen, and is any of it behind
//! the disk?" — a question a full walk-and-hash cannot answer on every request.
//! An incremental scan asks "which files changed since last time?" — a question
//! that is exactly one comparison per file if the previous answer was kept.
//!
//! Everything here is derived. The rows describe files on disk; the files are
//! the truth, deleting the rows loses nothing, and the next index pass rebuilds
//! them. Nothing in the knowledge model reads this table.

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::KnowledgeStore;
use crate::error::Result;

/// Which part of a project a source file belongs to.
///
/// The two are scanned by different passes with different budgets, and a path
/// can appear in both — a repository checked out under a vault is not a case
/// worth forbidding, only one worth keeping apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceScope {
    /// A page or a raw source under the vault.
    Vault,
    /// A file of the code repository being understood.
    Repository,
}

impl SourceScope {
    pub const ALL: &'static [SourceScope] = &[SourceScope::Vault, SourceScope::Repository];

    pub fn as_str(&self) -> &'static str {
        match self {
            SourceScope::Vault => "vault",
            SourceScope::Repository => "repository",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "vault" => Some(SourceScope::Vault),
            "repository" => Some(SourceScope::Repository),
            _ => None,
        }
    }
}

impl std::fmt::Display for SourceScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// One file as the last scan of its scope left it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub path: String,
    /// Size in bytes at scan time.
    pub bytes: u64,
    /// SHA-256 of the content, empty when the scan did not read the file.
    pub sha256: String,
    /// Modification time in nanoseconds since the Unix epoch, for the
    /// incremental comparison. Zero when the filesystem would not say.
    pub mtime_ns: i64,
    /// False for a file the scan saw and deliberately did not read — too
    /// large, binary, or past a budget. It is still a source; it is simply
    /// not one anything here understands.
    pub scanned: bool,
    /// Why it was not read, when it was not.
    pub detail: Option<String>,
    /// When this row was written, RFC 3339.
    pub indexed_at: String,
}

/// How much of the graph points at one source file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceReferences {
    /// Evidence rows anchored into the file.
    pub anchors: u64,
    /// Distinct nodes those anchors belong to.
    pub nodes: u64,
}

impl KnowledgeStore {
    /// Replace the recorded inventory for one scope.
    ///
    /// Wholesale, in one transaction: the scan that produced these rows saw the
    /// whole scope, so anything absent from `records` is a file that is gone,
    /// and leaving its row behind would make a deleted file look indexed.
    pub fn replace_sources(&mut self, scope: SourceScope, records: &[SourceRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM source_files WHERE scope = ?1",
            [scope.as_str()],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO source_files
                     (scope, path, bytes, sha256, mtime_ns, scanned, detail, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for record in records {
                insert.execute(params![
                    scope.as_str(),
                    record.path,
                    record.bytes as i64,
                    record.sha256,
                    record.mtime_ns,
                    record.scanned as i64,
                    record.detail,
                    record.indexed_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The recorded inventory, ordered by path so two listings read alike.
    pub fn list_sources(&self, limit: Option<u32>) -> Result<Vec<(SourceScope, SourceRecord)>> {
        let mut sql = String::from(
            "SELECT scope, path, bytes, sha256, mtime_ns, scanned, detail, indexed_at
             FROM source_files ORDER BY scope ASC, path ASC",
        );
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {}", limit.min(100_000)));
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |row| {
                let scope: String = row.get(0)?;
                Ok((
                    scope,
                    SourceRecord {
                        path: row.get(1)?,
                        bytes: row.get::<_, i64>(2)? as u64,
                        sha256: row.get(3)?,
                        mtime_ns: row.get(4)?,
                        scanned: row.get::<_, i64>(5)? != 0,
                        detail: row.get(6)?,
                        indexed_at: row.get(7)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(scope, record)| SourceScope::parse(&scope).map(|scope| (scope, record)))
            .collect())
    }

    /// How many files one scope holds, without loading them.
    pub fn source_count(&self, scope: SourceScope) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM source_files WHERE scope = ?1",
            [scope.as_str()],
            |row| row.get::<_, i64>(0),
        )? as u64)
    }

    /// Evidence anchors per anchored file, for the `sources` report.
    ///
    /// One grouped query rather than a query per file: a project with twenty
    /// thousand files must not pay twenty thousand round trips to list them.
    pub fn source_references(&self) -> Result<HashMap<String, SourceReferences>> {
        let mut stmt = self.conn.prepare(
            "SELECT anchor_file, COUNT(*), COUNT(DISTINCT node)
             FROM evidence WHERE anchor_file IS NOT NULL GROUP BY anchor_file",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    SourceReferences {
                        anchors: row.get::<_, i64>(1)? as u64,
                        nodes: row.get::<_, i64>(2)? as u64,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().collect())
    }
}
