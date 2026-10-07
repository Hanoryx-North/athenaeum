//! Persistence for the knowledge model.
//!
//! Everything here goes through one SQLite file plus a directory of
//! content-addressed blobs. Opening a knowledge base needs no server, no
//! index build and no network (buildplan §56).

mod edges;
mod events;
mod evidence;
mod nodes;
mod schema;
mod search;

pub use edges::EdgeFilter;
pub use events::EventFilter;
pub use nodes::NodeFilter;
pub use schema::SCHEMA_VERSION;
pub use search::SearchHit;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::blobs::BlobStore;
use crate::error::{Error, Result};

/// Filename of the knowledge database inside the workspace directory.
pub const DB_FILENAME: &str = "knowledge.db";

/// A local knowledge base: the graph, its evidence and its history.
///
/// Cloning is deliberately not offered — a store owns a SQLite connection.
/// Share one behind a `Mutex` (which is what the desktop shell does) rather
/// than opening the same file twice from one process.
#[derive(Debug)]
pub struct KnowledgeStore {
    conn: Connection,
    blobs: BlobStore,
    root: PathBuf,
}

impl KnowledgeStore {
    /// Open (creating if needed) the knowledge base rooted at `root`.
    ///
    /// `root` is the workspace directory — conventionally `.athenaeum/`
    /// inside the project. The layout it manages is:
    ///
    /// ```text
    /// .athenaeum/
    /// ├── knowledge.db     nodes, edges, evidence, history, journal
    /// ├── blobs/sha256/    content-addressed evidence material
    /// ├── exports/         human-readable views, written on demand
    /// └── cache/           derived, always safe to delete
    /// ```
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;

        let db_path = root.join(DB_FILENAME);
        let mut conn = Connection::open(&db_path).map_err(|source| Error::Storage {
            path: db_path.display().to_string(),
            source,
        })?;

        schema::apply_pragmas(&conn)?;
        schema::migrate(&mut conn)?;

        let mut store = Self {
            conn,
            blobs: BlobStore::new(root.join("blobs")),
            root,
        };
        store.ensure_search_index_current()?;
        Ok(store)
    }

    /// An ephemeral store held entirely in memory.
    ///
    /// Intended for tests and for previewing an import before committing it.
    /// Blobs go to a scratch directory that is *not* cleaned up on drop, so
    /// callers who care should point [`BlobStore`] somewhere they control.
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        schema::apply_pragmas(&conn)?;
        schema::migrate(&mut conn)?;

        let root = std::env::temp_dir().join(format!("athenaeum-{}", uuid::Uuid::new_v4()));
        let mut store = Self {
            conn,
            blobs: BlobStore::new(root.join("blobs")),
            root,
        };
        store.ensure_search_index_current()?;
        Ok(store)
    }

    /// The workspace directory this store manages.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where large evidence material is kept.
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Escape hatch for read-only queries the typed API does not cover.
    ///
    /// Writing through this bypasses versioning, the journal and the search
    /// index — do not.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Counts, for a status line or a health check.
    pub fn stats(&self) -> Result<Stats> {
        let count = |sql: &str| -> Result<u64> {
            Ok(self.conn.query_row(sql, [], |row| row.get::<_, i64>(0))? as u64)
        };
        Ok(Stats {
            nodes: count("SELECT COUNT(*) FROM nodes")?,
            live_nodes: count(
                "SELECT COUNT(*) FROM nodes
                 WHERE status IN ('valid', 'stale', 'stale_requires_review')",
            )?,
            edges: count("SELECT COUNT(*) FROM edges")?,
            evidence: count("SELECT COUNT(*) FROM evidence")?,
            events: count("SELECT COUNT(*) FROM journal")?,
            schema_version: SCHEMA_VERSION,
        })
    }
}

/// A summary of what a knowledge base contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub nodes: u64,
    /// Nodes an agent would currently be offered — excludes superseded and
    /// archived knowledge, which is retained but not live.
    pub live_nodes: u64,
    pub edges: u64,
    pub evidence: u64,
    pub events: u64,
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// Row-mapping helpers, shared by the submodules.
// ---------------------------------------------------------------------------

/// Timestamps are stored as RFC 3339 text so the database stays readable in
/// any SQLite browser, rather than as opaque integers.
pub(crate) fn to_sql_time(value: &DateTime<Utc>) -> String {
    value.to_rfc3339()
}

pub(crate) fn parse_time(raw: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

/// Turn a stored enum string back into its variant, failing loudly rather
/// than defaulting: an unrecognised value means the file was written by
/// something that knew more than we do, and guessing would corrupt meaning.
pub(crate) fn parse_enum<T: std::str::FromStr>(
    raw: &str,
    field: &'static str,
) -> rusqlite::Result<T> {
    raw.parse::<T>().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown {field}: {raw:?}"),
            )),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_creates_the_workspace_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".athenaeum");
        let store = KnowledgeStore::open(&root).unwrap();
        assert!(root.join(DB_FILENAME).exists());
        assert_eq!(store.root(), root);
    }

    #[test]
    fn reopening_preserves_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".athenaeum");
        KnowledgeStore::open(&root).unwrap();
        let reopened = KnowledgeStore::open(&root).unwrap();
        assert_eq!(reopened.stats().unwrap().nodes, 0);
    }

    #[test]
    fn a_fresh_store_is_empty_but_versioned() {
        let store = KnowledgeStore::open_in_memory().unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(stats.nodes, 0);
        assert_eq!(stats.edges, 0);
        assert_eq!(stats.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn timestamps_round_trip_through_text() {
        let now = Utc::now();
        let parsed = parse_time(&to_sql_time(&now)).unwrap();
        assert_eq!(parsed.timestamp_micros(), now.timestamp_micros());
    }

    #[test]
    fn unknown_enum_values_fail_instead_of_defaulting() {
        use crate::model::NodeType;
        let result = parse_enum::<NodeType>("wormhole", "node type");
        assert!(result.is_err());
    }
}
