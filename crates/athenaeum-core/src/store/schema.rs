//! Schema definition and migration.
//!
//! One SQLite file holds the whole knowledge base (buildplan §56/§57): nodes,
//! edges, evidence, history and the event journal. No vector database is
//! required to open a project.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// The schema version this build writes and understands.
pub const SCHEMA_VERSION: u32 = 3;

/// Migrations, applied in order. Index `n` migrates from version `n` to
/// `n + 1`, so appending to this array is the only way to change the schema.
/// Never edit an entry that has shipped.
const MIGRATIONS: &[&str] = &[V1, V2, V3];

/// Connection-level settings applied every time the database is opened.
///
/// These are not persisted in the file, so they must be re-applied per
/// connection rather than set once at creation.
pub(crate) fn apply_pragmas(conn: &Connection) -> Result<()> {
    // Foreign keys are OFF by default in SQLite and must be enabled per
    // connection. Without this, `edges` could reference deleted nodes.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // WAL lets the UI read while the curator writes, instead of the reader
    // blocking behind an indexing pass. Works on all three target platforms.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Rather than failing instantly when the curator holds the write lock.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Bring a freshly-opened database up to [`SCHEMA_VERSION`].
///
/// Refuses to touch a file written by a newer build: silently dropping
/// columns this version does not know about would destroy the user's
/// knowledge, and the knowledge is the product.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let found: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if found > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found == SCHEMA_VERSION {
        return Ok(());
    }

    // Each step runs in its own transaction so a failure halfway through a
    // multi-version upgrade leaves the file at the last version that fully
    // applied, rather than in a half-migrated state.
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration)?;
        let next = index as u32 + 1;
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
    }

    Ok(())
}

const V1: &str = r#"
-- Nodes: semantic objects, never text chunks (§9).
CREATE TABLE nodes (
    id                  TEXT PRIMARY KEY,
    type                TEXT NOT NULL,
    title               TEXT NOT NULL,
    summary             TEXT NOT NULL DEFAULT '',
    content             TEXT NOT NULL DEFAULT '',
    scope               TEXT NOT NULL,
    authority           TEXT NOT NULL,
    status              TEXT NOT NULL,
    provenance_source   TEXT NOT NULL,
    provenance_detail   TEXT,
    provenance_session  TEXT,
    last_verified_at    TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_nodes_type   ON nodes(type);
CREATE INDEX idx_nodes_status ON nodes(status);
CREATE INDEX idx_nodes_scope  ON nodes(scope);
-- Case-insensitive so entity resolution can ask "do we already know about
-- 'authservice'?" without scanning the table (§39, duplicate detection).
CREATE INDEX idx_nodes_title  ON nodes(title COLLATE NOCASE);

-- Every edit archives the previous body here (§36). History is never lost.
CREATE TABLE node_versions (
    node        TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    title       TEXT NOT NULL,
    summary     TEXT NOT NULL,
    content     TEXT NOT NULL,
    authority   TEXT NOT NULL,
    status      TEXT NOT NULL,
    reason      TEXT,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (node, version)
);

-- Edges: first-class objects with their own type, provenance and state (§21).
CREATE TABLE edges (
    id                  TEXT PRIMARY KEY,
    from_node           TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    to_node             TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    type                TEXT NOT NULL,
    status              TEXT NOT NULL,
    scope               TEXT NOT NULL,
    confidence          REAL,
    note                TEXT,
    provenance_source   TEXT NOT NULL,
    provenance_detail   TEXT,
    provenance_session  TEXT,
    created_at          TEXT NOT NULL,
    -- The same relationship asserted twice is one relationship. This is the
    -- main defence against the "semantic hairball" of §21.
    UNIQUE (from_node, to_node, type)
);

CREATE INDEX idx_edges_from ON edges(from_node);
CREATE INDEX idx_edges_to   ON edges(to_node);
CREATE INDEX idx_edges_type ON edges(type);

-- Evidence: why we believe a node (§15).
CREATE TABLE evidence (
    id            TEXT PRIMARY KEY,
    node          TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,
    anchor_json   TEXT,
    -- Denormalized out of anchor_json so "which knowledge touches this file?"
    -- is an index lookup rather than a full scan with JSON parsing.
    anchor_file   TEXT,
    anchor_symbol TEXT,
    blob_hash     TEXT,
    excerpt       TEXT,
    note          TEXT,
    created_at    TEXT NOT NULL
);

CREATE INDEX idx_evidence_node ON evidence(node);
CREATE INDEX idx_evidence_file ON evidence(anchor_file);

-- Append-only event journal (§42). Nothing else writes history; nothing
-- ever updates or deletes a row here.
CREATE TABLE journal (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    at          TEXT NOT NULL,
    kind        TEXT NOT NULL,
    subject     TEXT,
    session     TEXT,
    detail_json TEXT
);

CREATE INDEX idx_journal_kind    ON journal(kind);
CREATE INDEX idx_journal_subject ON journal(subject);

-- Lexical search. Derived data: droppable and rebuildable from `nodes` at
-- any time (§60), which is why nothing else depends on it being present.
CREATE VIRTUAL TABLE nodes_fts USING fts5(
    id UNINDEXED,
    body,
    tokenize = 'porter unicode61'
);

-- Small key/value bag for things that are neither knowledge nor derived.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const V2: &str = r#"
-- A deliberately narrow entity-resolution key. It folds ASCII case and the
-- separators people commonly vary in identifiers (`AuthService`,
-- `auth_service`, `auth-service`) while retaining all other punctuation so
-- unrelated titles are not merged speculatively.
ALTER TABLE nodes ADD COLUMN title_key TEXT NOT NULL DEFAULT '';
UPDATE nodes
SET title_key = lower(replace(replace(replace(replace(replace(replace(
    replace(replace(title, ' ', ''), char(9), ''), char(10), ''),
    char(11), ''), char(12), ''), char(13), ''), '-', ''), '_', ''));
CREATE INDEX idx_nodes_title_key ON nodes(title_key);
"#;

const V3: &str = r#"
-- The source inventory: one row per file the engine scanned, holding what the
-- `sources` report needs and the modification time an incremental scan
-- compares against before it reads a file again.
--
-- Derived data in the same sense as `nodes_fts`, and for the same reason: it
-- describes files on disk, it is rebuilt by the next index pass, and deleting
-- every row loses no knowledge. `scanned = 0` is a first-class value: it marks
-- a file the scan saw and deliberately did not read, which is the difference
-- between "not indexed" and "there is nothing here to index".
CREATE TABLE source_files (
    scope      TEXT NOT NULL,
    path       TEXT NOT NULL,
    bytes      INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    mtime_ns   INTEGER NOT NULL,
    scanned    INTEGER NOT NULL,
    detail     TEXT,
    indexed_at TEXT NOT NULL,
    PRIMARY KEY (scope, path)
);

CREATE INDEX idx_source_files_path ON source_files(path);
"#;
