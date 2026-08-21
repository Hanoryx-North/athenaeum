//! Attaching and reading evidence.

use rusqlite::{params, Row};

use super::{parse_enum, parse_time, to_sql_time, KnowledgeStore};
use crate::error::{Error, Result};
use crate::id::{EvidenceId, NodeId};
use crate::journal::{EventKind, NewEvent};
use crate::model::{Evidence, SourceAnchor};

const EVIDENCE_COLUMNS: &str = "id, node, kind, anchor_json, blob_hash, excerpt, note, created_at";

impl KnowledgeStore {
    /// Attach evidence to a node.
    ///
    /// Excerpts past [`crate::model::INLINE_EXCERPT_LIMIT`] are moved into the
    /// content-addressed blob store and referenced by hash, so a hundred
    /// nodes citing the same long file cost one copy of it (§59).
    pub fn attach_evidence(&mut self, mut evidence: Evidence) -> Result<Evidence> {
        if self.find_node(&evidence.node)?.is_none() {
            return Err(Error::NodeNotFound(evidence.node.clone()));
        }

        if evidence.excerpt_needs_blob() {
            if let Some(excerpt) = evidence.excerpt.take() {
                evidence.blob_hash = Some(self.blobs.put(&excerpt)?);
            }
        }

        let anchor_json = evidence
            .anchor
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO evidence
                (id, node, kind, anchor_json, anchor_file, anchor_symbol,
                 blob_hash, excerpt, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                evidence.id,
                evidence.node,
                evidence.kind.as_str(),
                anchor_json,
                evidence.anchor.as_ref().map(|a| a.file.clone()),
                evidence.anchor.as_ref().and_then(|a| a.symbol.clone()),
                evidence.blob_hash,
                evidence.excerpt,
                evidence.note,
                to_sql_time(&evidence.created_at),
            ],
        )?;
        super::events::record_in(
            &tx,
            &NewEvent::about(EventKind::EvidenceAttached, &evidence.id).with_detail(
                serde_json::json!({
                    "node": evidence.node.as_str(),
                    "kind": evidence.kind.as_str(),
                }),
            ),
        )?;
        tx.commit()?;

        Ok(evidence)
    }

    /// All evidence supporting a node, oldest first.
    pub fn evidence_for(&self, node: &NodeId) -> Result<Vec<Evidence>> {
        let sql = format!(
            "SELECT {EVIDENCE_COLUMNS} FROM evidence WHERE node = ?1 ORDER BY created_at ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![node], map_evidence)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Which knowledge cites a given source file.
    ///
    /// This is what makes staleness detection cheap: when a file changes, the
    /// nodes that might now be wrong are an index lookup away instead of a
    /// full scan of the graph (§17).
    pub fn evidence_for_file(&self, path: &str) -> Result<Vec<Evidence>> {
        let normalized = path.replace('\\', "/");
        let sql = format!("SELECT {EVIDENCE_COLUMNS} FROM evidence WHERE anchor_file = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![normalized], map_evidence)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The nodes whose evidence touches a given file.
    pub fn nodes_citing_file(&self, path: &str) -> Result<Vec<NodeId>> {
        let normalized = path.replace('\\', "/");
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT node FROM evidence WHERE anchor_file = ?1")?;
        let ids = stmt
            .query_map(params![normalized], |row| row.get::<_, NodeId>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    /// Read an evidence excerpt, transparently pulling it out of the blob
    /// store when it was too large to keep inline.
    pub fn evidence_text(&self, evidence: &Evidence) -> Result<Option<String>> {
        if let Some(excerpt) = &evidence.excerpt {
            return Ok(Some(excerpt.clone()));
        }
        match &evidence.blob_hash {
            Some(hash) => self.blobs.get(hash),
            None => Ok(None),
        }
    }

    pub fn remove_evidence(&mut self, id: &EvidenceId) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM evidence WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(Error::EvidenceNotFound(id.clone()));
        }
        Ok(())
    }
}

fn map_evidence(row: &Row<'_>) -> rusqlite::Result<Evidence> {
    let anchor_json: Option<String> = row.get(3)?;
    let anchor: Option<SourceAnchor> = anchor_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;

    Ok(Evidence {
        id: row.get(0)?,
        node: row.get(1)?,
        kind: parse_enum(&row.get::<_, String>(2)?, "evidence kind")?,
        anchor,
        blob_hash: row.get(4)?,
        excerpt: row.get(5)?,
        note: row.get(6)?,
        created_at: parse_time(&row.get::<_, String>(7)?)?,
    })
}
