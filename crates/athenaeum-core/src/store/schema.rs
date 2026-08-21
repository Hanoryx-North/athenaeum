//! Schema definition and migration.
//!
//! One SQLite file holds the whole knowledge base (buildplan §56/§57): nodes,
//! edges, evidence, history and the event journal. No vector database is
//! required to open a project.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// The schema version this build writes and understands.
pub const SCHEMA_VERSION: u32 = 2;

/// Migrations, applied in order. Index `n` migrates from version `n` to
/// `n + 1`, so appending to this array is the only way to change the schema.
/// Never edit an entry that has shipped.
const MIGRATIONS: &[&str] = &[V1, V2];

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

#[cfg(test)]
mod tests {
    use super::*;

    fn open_migrated() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn migration_reaches_the_current_version() {
        let conn = open_migrated();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn migration_count_matches_the_declared_version() {
        // A migration added without bumping SCHEMA_VERSION would never run.
        assert_eq!(MIGRATIONS.len() as u32, SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = open_migrated();
        migrate(&mut conn).expect("second migrate must be harmless");
    }

    #[test]
    fn title_keys_are_backfilled_when_upgrading_v1() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        conn.execute_batch(V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO nodes
                (id, type, title, scope, authority, status, provenance_source,
                 created_at, updated_at)
             VALUES ('n1', 'symbol', 'Auth_Service', 'project',
                     'imported_knowledge', 'valid', 'deterministic',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let key: String = conn
            .query_row("SELECT title_key FROM nodes WHERE id = 'n1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(key, "authservice");
    }

    #[test]
    fn a_newer_database_is_refused_rather_than_downgraded() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
            .unwrap();
        let err = migrate(&mut conn).unwrap_err();
        assert!(matches!(err, Error::SchemaTooNew { .. }), "got {err:?}");
    }

    #[test]
    fn foreign_keys_are_actually_enforced() {
        let conn = open_migrated();
        // An edge pointing at a node that was never created must be rejected;
        // otherwise traversal would find ids it cannot load.
        let result = conn.execute(
            "INSERT INTO edges (id, from_node, to_node, type, status, scope,
                                provenance_source, created_at)
             VALUES ('e1', 'ghost', 'ghost2', 'related_to', 'valid', 'project',
                     'model', '2026-01-01T00:00:00Z')",
            [],
        );
        assert!(result.is_err(), "dangling edge was accepted");
    }

    #[test]
    fn fts5_is_available_in_the_bundled_sqlite() {
        // The whole point of vendoring SQLite is that this holds on every
        // platform, not just whichever one the developer happens to use.
        let conn = open_migrated();
        conn.execute(
            "INSERT INTO nodes_fts (id, body) VALUES ('n1', 'oauth callback')",
            [],
        )
        .unwrap();
        let found: String = conn
            .query_row(
                "SELECT id FROM nodes_fts WHERE nodes_fts MATCH 'oauth'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, "n1");
    }
}
