//! Lexical search over node text.
//!
//! Buildplan §23/§55: this is *lookup*, not the architecture. It exists to
//! find seed nodes so that graph navigation has somewhere to start. The
//! index is derived data and can be dropped and rebuilt at any time (§60).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::{nodes::map_node, KnowledgeStore};
use crate::error::Result;
use crate::id::NodeId;
use crate::model::KnowledgeNode;

/// A node matched by a search, with its relevance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub node: KnowledgeNode,
    /// Higher is better. Derived from BM25, sign-flipped so that callers do
    /// not have to remember that SQLite's ranking is negative-is-better.
    pub score: f32,
}

impl KnowledgeStore {
    /// Find candidate nodes by text.
    ///
    /// Returns *candidates*, not an answer (§24). The wiki agent decides what
    /// is actually relevant by navigating out from these.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        // A query of pure punctuation matches nothing rather than erroring
        // out of FTS5's parser.
        let Some(terms) = fts_terms(query) else {
            return Ok(Vec::new());
        };

        // Precision first: require every term. This is what a caller
        // searching for `oauth_callback` or `AuthService` wants.
        let hits = self.search_matching(&terms.join(" "), limit)?;
        if !hits.is_empty() || terms.len() < 2 {
            return Ok(hits);
        }

        // Nothing matched every term. That is the normal case when the query
        // is a task rather than a term — no node contains all of "fix", "the",
        // "oauth", "callback" and "bug". Widen to any-term matching and let
        // BM25 rank: nodes hitting more of the rare words come out on top,
        // and common words contribute almost nothing to the score.
        self.search_matching(&terms.join(" OR "), limit)
    }

    fn search_matching(&self, match_expr: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let sql = "SELECT n.id, n.type, n.title, n.summary, n.content, n.scope, n.authority,
                          n.status, n.provenance_source, n.provenance_detail,
                          n.provenance_session, n.last_verified_at, n.created_at,
                          n.updated_at, n.version, bm25(nodes_fts) AS rank
                   FROM nodes_fts
                   JOIN nodes n ON n.id = nodes_fts.id
                   WHERE nodes_fts MATCH ?1
                     AND n.status IN ('valid', 'stale', 'stale_requires_review')
                   ORDER BY rank
                   LIMIT ?2";

        let mut stmt = self.conn.prepare(sql)?;
        let hits = stmt
            .query_map(params![match_expr, limit], |row| {
                let bm25: f64 = row.get(15)?;
                Ok(SearchHit {
                    node: map_node(row)?,
                    score: -bm25 as f32,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(hits)
    }

    /// Drop and rebuild the search index from the nodes table.
    ///
    /// Safe to call at any time. Needed after a direct SQL edit, an import,
    /// or a tokenizer change (§60).
    pub fn rebuild_search_index(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM nodes_fts", [])?;
        {
            let sql = "SELECT id, type, title, summary, content, scope, authority, status,
                              provenance_source, provenance_detail, provenance_session,
                              last_verified_at, created_at, updated_at, version
                       FROM nodes";
            let mut stmt = tx.prepare(sql)?;
            let nodes = stmt
                .query_map([], map_node)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for node in &nodes {
                index_node(&tx, node)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn index_node(conn: &Connection, node: &KnowledgeNode) -> Result<()> {
    conn.execute(
        "INSERT INTO nodes_fts (id, body) VALUES (?1, ?2)",
        params![node.id, node.search_text()],
    )?;
    Ok(())
}

pub(crate) fn reindex_node(conn: &Connection, node: &KnowledgeNode) -> Result<()> {
    unindex_node(conn, &node.id)?;
    index_node(conn, node)
}

pub(crate) fn unindex_node(conn: &Connection, id: &NodeId) -> Result<()> {
    conn.execute("DELETE FROM nodes_fts WHERE id = ?1", params![id])?;
    Ok(())
}

/// Turn arbitrary user text into a safe FTS5 MATCH expression.
///
/// FTS5's query language treats quotes, `*`, `^`, `NEAR` and friends as
/// syntax, so a user searching for `auth"` would otherwise get a parse error
/// rather than results. Every token is stripped to word characters and
/// requoted, which loses operator support and gains never crashing on a
/// search box.
fn fts_terms(raw: &str) -> Option<Vec<String>> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|term| {
            term.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\""))
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(raw: &str) -> Option<Vec<String>> {
        fts_terms(raw)
    }

    #[test]
    fn quotes_and_operators_are_neutralized() {
        // Every token is requoted, so FTS5 sees data rather than syntax and
        // a user typing a stray quote gets results instead of a parse error.
        assert_eq!(terms("oauth"), Some(vec!["\"oauth\"".to_string()]));
        assert_eq!(
            terms("oauth\" OR x"),
            Some(vec!["\"oauth\"".into(), "\"OR\"".into(), "\"x\"".into()])
        );
        assert_eq!(
            terms("auth NEAR/2 session"),
            Some(vec![
                "\"auth\"".into(),
                "\"NEAR2\"".into(),
                "\"session\"".into()
            ])
        );
    }

    #[test]
    fn punctuation_only_queries_match_nothing() {
        assert_eq!(terms("!!!"), None);
        assert_eq!(terms("   "), None);
    }

    #[test]
    fn hyphens_and_underscores_survive_because_identifiers_use_them() {
        assert_eq!(
            terms("oauth_callback"),
            Some(vec!["\"oauth_callback\"".to_string()])
        );
        assert_eq!(
            terms("well-known"),
            Some(vec!["\"well-known\"".to_string()])
        );
    }
}
