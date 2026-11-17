//! Lexical search over node text.
//!
//! Buildplan §23/§55: this is *lookup*, not the architecture. It exists to
//! find seed nodes so that graph navigation has somewhere to start. The
//! index is derived data and can be dropped and rebuilt at any time (§60).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::nodes::{map_node, node_columns_for, NODE_COLUMNS};
use super::KnowledgeStore;
use crate::error::Result;
use crate::id::NodeId;
use crate::model::applicability::APPLICABLE_SQL;
use crate::model::{KnowledgeNode, RecallSelection};

/// A node matched by a search, with its relevance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub node: KnowledgeNode,
    /// Higher is better. Derived from BM25, sign-flipped so that callers do
    /// not have to remember that SQLite's ranking is negative-is-better.
    pub score: f32,
}

/// What the full-text index holds for a node, as a version.
///
/// Bump it whenever [`index_text`] changes. The index is derived data, so a database
/// written by an older build is simply re-indexed once on open instead of answering
/// from terms the current build would no longer produce. Version 1 was the title,
/// summary and content alone; version 2 adds the words inside camelCase identifiers.
const SEARCH_INDEX_VERSION: u32 = 2;

/// The `meta` key recording which [`SEARCH_INDEX_VERSION`] the stored index was built with.
const SEARCH_INDEX_VERSION_KEY: &str = "search_index_version";

impl KnowledgeStore {
    /// Task recall needs more precision than an interactive keyword search.
    /// Common sentence words must not seed unrelated projects in a shared wiki.
    pub fn search_context(&self, task: &str, limit: u32) -> Result<Vec<SearchHit>> {
        self.search_context_selected(task, limit, &RecallSelection::default())
    }

    pub fn search_context_selected(
        &self,
        task: &str,
        limit: u32,
        selection: &RecallSelection,
    ) -> Result<Vec<SearchHit>> {
        selection.validate()?;
        let terms = context_terms(task);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let minimum = if terms.len() >= 4 { 2 } else { 1 };
        let query = terms
            .iter()
            .map(|term| format!("\"{term}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let hits = self.search_matching_selected(
            &query,
            limit.saturating_mul(4).min(100),
            Some(selection),
        )?;
        Ok(hits
            .into_iter()
            .filter(|hit| {
                // Source paths and registry text can match a task without
                // contributing a lesson. Require a semantic entry point in
                // the title or summary before counting body matches.
                let heading = format!("{} {}", hit.node.title, hit.node.summary);
                let expanded_heading =
                    format!("{} {}", heading, identifier_parts(&heading)).to_lowercase();
                let heading_words: std::collections::HashSet<_> = expanded_heading
                    .split(|c: char| !c.is_alphanumeric())
                    .collect();
                if !terms
                    .iter()
                    .any(|term| heading_words.contains(term.as_str()))
                {
                    return false;
                }
                // Older capture appended provenance to the body. It remains
                // searchable interactively but must not establish relevance.
                let body = hit
                    .node
                    .content
                    .split("\n\nWorkspace:")
                    .next()
                    .unwrap_or_default();
                let text = format!("{} {} {}", hit.node.title, hit.node.summary, body);
                let expanded = format!("{} {}", text, identifier_parts(&text)).to_lowercase();
                let words: std::collections::HashSet<_> =
                    expanded.split(|c: char| !c.is_alphanumeric()).collect();
                terms
                    .iter()
                    .filter(|term| words.contains(term.as_str()))
                    .take(minimum)
                    .count()
                    >= minimum
            })
            .take(limit as usize)
            .collect())
    }
    pub fn core_context_seeds(
        &self,
        selection: &RecallSelection,
        limit: u32,
    ) -> Result<Vec<NodeId>> {
        selection.validate()?;
        let sql = format!(
            "SELECT n.id FROM nodes n WHERE n.status IN ('valid', 'stale', 'stale_requires_review')
            AND json_extract(n.applicability, '$.core') = 1
            AND n.provenance_source != 'model' AND {APPLICABLE_SQL}
            ORDER BY (json_array_length(n.applicability, '$.projects') > 0) DESC,
                     (json_array_length(n.applicability, '$.technologies') > 0) DESC,
                     n.title, n.id LIMIT ?2"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let nodes = statement
            .query_map(
                params![
                    rusqlite::types::Null,
                    limit,
                    serde_json::to_string(selection)?
                ],
                |row| row.get(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(nodes)
    }

    /// Re-index when the stored full-text index was built by an older build.
    ///
    /// A fresh database has no recorded version and no nodes, so this is a no-op rebuild that
    /// stamps it. An index recorded by a *newer* build is left alone: it only holds more terms
    /// than this build would write, and rebuilding it down would just make two builds sharing
    /// one project undo each other's work on every open.
    pub(crate) fn ensure_search_index_current(&mut self) -> Result<()> {
        let stored: Option<u32> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![SEARCH_INDEX_VERSION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse().ok());
        if stored.is_some_and(|version| version >= SEARCH_INDEX_VERSION) {
            return Ok(());
        }
        self.rebuild_search_index()?;
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SEARCH_INDEX_VERSION_KEY, SEARCH_INDEX_VERSION.to_string()],
        )?;
        Ok(())
    }

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

    /// What else is about the same thing as `node`.
    ///
    /// "More like this": the same lexical index, asked in the other
    /// direction. Any-term matching rather than every-term, because a
    /// title is a phrase and the nodes worth relating to it rarely repeat
    /// it word for word — `Attention Head` and `Attention` are about the
    /// same subject and share one word of two.
    ///
    /// The node itself is never its own neighbour, and it is excluded
    /// before the limit applies so asking for one candidate returns one.
    pub fn similar(&self, node: &KnowledgeNode, limit: u32) -> Result<Vec<SearchHit>> {
        let Some(terms) = fts_terms(&node.title) else {
            return Ok(Vec::new());
        };
        let hits = self.search_matching(&terms.join(" OR "), limit.saturating_add(1))?;
        Ok(hits
            .into_iter()
            .filter(|hit| hit.node.id != node.id)
            .take(limit as usize)
            .collect())
    }

    fn search_matching(&self, match_expr: &str, limit: u32) -> Result<Vec<SearchHit>> {
        self.search_matching_selected(match_expr, limit, None)
    }

    fn search_matching_selected(
        &self,
        match_expr: &str,
        limit: u32,
        selection: Option<&RecallSelection>,
    ) -> Result<Vec<SearchHit>> {
        // `rank` sits immediately after the node columns, which is the index
        // `map_node` stops at.
        let rank_column = NODE_COLUMNS.split(',').count();
        let sql = format!(
            "SELECT {}, bm25(nodes_fts) AS rank
             FROM nodes_fts
             JOIN nodes n ON n.id = nodes_fts.id
             WHERE nodes_fts MATCH ?1
               AND n.status IN ('valid', 'stale', 'stale_requires_review')
               AND (?3 IS NULL OR {APPLICABLE_SQL})
             ORDER BY rank, n.id
             LIMIT ?2",
            node_columns_for("n")
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let hits = stmt
            .query_map(
                params![
                    match_expr,
                    limit,
                    selection.map(serde_json::to_string).transpose()?
                ],
                |row| {
                    let bm25: f64 = row.get(rank_column)?;
                    Ok(SearchHit {
                        node: map_node(row)?,
                        score: -bm25 as f32,
                    })
                },
            )?
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
            let sql = format!("SELECT {NODE_COLUMNS} FROM nodes");
            let mut stmt = tx.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            // Streamed rather than collected: a rebuild on a large knowledge
            // base should not need the whole nodes table in memory at once.
            while let Some(row) = rows.next()? {
                index_node(&tx, &map_node(row)?)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn index_node(conn: &Connection, node: &KnowledgeNode) -> Result<()> {
    conn.execute(
        "INSERT INTO nodes_fts (id, body) VALUES (?1, ?2)",
        params![node.id, index_text(node)],
    )?;
    Ok(())
}

/// The text FTS5 indexes for a node: its search text, then the words inside its identifiers.
///
/// FTS5's `unicode61` tokenizer splits on punctuation, so `oauth_callback` and `oauth-callback`
/// are already two terms. It does not split on case, so `OAuthCallback` is one term and a query
/// written the way a person or a model phrases it — `oauth callback` — never reaches a node named
/// by the identifier. In a knowledge base about code that is most of the nodes. The original token
/// stays in the text, so searching for the identifier itself still matches.
fn index_text(node: &KnowledgeNode) -> String {
    let text = node.search_text();
    let parts = identifier_parts(&text);
    if parts.is_empty() {
        text
    } else {
        format!("{text}\n{parts}")
    }
}

/// The words inside every camelCase, PascalCase or digit-joined identifier in `text`, space separated.
///
/// Each identifier is cut two ways and both are kept. Cutting only where a lowercase letter or
/// digit meets an uppercase one keeps `OAuth` whole, which is what a query says. Cutting as well
/// where an acronym ends splits `HTTPServer` into `HTTP` and `Server`, which a plain cut would
/// not, and also yields `Auth` from `OAuth`, which is harmless. Single characters are dropped,
/// because `O` and `I` are noise rather than terms. Text with no identifiers returns an empty string.
fn identifier_parts(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        // Shorter than two characters per part cannot contain a boundary worth indexing.
        if token.chars().count() < 4 {
            continue;
        }
        let whole = split_identifier(token, false);
        let acronyms = split_identifier(token, true);
        if whole.len() < 2 && acronyms.len() < 2 {
            continue;
        }
        let first_of_token = out.len();
        for part in whole.into_iter().chain(acronyms) {
            if part.chars().count() >= 2 && part != token && !out[first_of_token..].contains(&part)
            {
                out.push(part);
            }
        }
    }
    out.join(" ")
}

/// Cut one alphanumeric token at its case boundaries.
///
/// A boundary falls before an uppercase letter that follows a lowercase letter or a digit. With
/// `split_acronyms` it also falls before the last capital of a run that is followed by lowercase
/// (`HTTPServer` → `HTTP`, `Server`).
fn split_identifier(token: &str, split_acronyms: bool) -> Vec<&str> {
    let chars: Vec<(usize, char)> = token.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (previous, current) = (chars[i - 1].1, chars[i].1);
        let next = chars.get(i + 1).map(|&(_, c)| c);
        let after_lower_or_digit =
            (previous.is_lowercase() || previous.is_numeric()) && current.is_uppercase();
        let acronym_ends = split_acronyms
            && previous.is_uppercase()
            && current.is_uppercase()
            && next.is_some_and(char::is_lowercase);
        if after_lower_or_digit || acronym_ends {
            parts.push(&token[chars[start].0..chars[i].0]);
            start = i;
        }
    }
    parts.push(&token[chars[start].0..]);
    parts
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

fn context_terms(task: &str) -> Vec<String> {
    const COMMON: &[&str] = &[
        "a",
        "an",
        "and",
        "are",
        "as",
        "at",
        "be",
        "been",
        "before",
        "but",
        "by",
        "can",
        "check",
        "could",
        "do",
        "does",
        "ensure",
        "every",
        "everything",
        "fix",
        "for",
        "from",
        "get",
        "has",
        "have",
        "how",
        "if",
        "in",
        "into",
        "is",
        "it",
        "its",
        "like",
        "make",
        "me",
        "my",
        "not",
        "of",
        "on",
        "or",
        "our",
        "please",
        "so",
        "some",
        "sure",
        "that",
        "the",
        "their",
        "them",
        "then",
        "there",
        "these",
        "they",
        "this",
        "through",
        "to",
        "up",
        "use",
        "using",
        "verify",
        "want",
        "was",
        "we",
        "were",
        "what",
        "when",
        "which",
        "while",
        "will",
        "with",
        "work",
        "works",
        "would",
        "you",
        "your",
    ];
    let mut terms = Vec::new();
    for term in task
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
    {
        if term.chars().count() < 2 || COMMON.contains(&term.as_str()) || terms.contains(&term) {
            continue;
        }
        terms.push(term);
        if terms.len() == 32 {
            break;
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeType;

    #[test]
    fn source_registry_matches_do_not_establish_task_relevance() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(
                KnowledgeNode::new(NodeType::Fact, "Imported source registry")
                    .with_summary("A source was registered.")
                    .with_content(
                        "Workspace: /projects/oauth/callback\nSources: oauth_callback.rs",
                    ),
            )
            .unwrap();
        assert!(store
            .search_context("debug oauth callback", 5)
            .unwrap()
            .is_empty());
        // Interactive search still finds the reference when explicitly requested.
        assert!(!store.search("oauth callback", 5).unwrap().is_empty());
        let useful = store.create_node(KnowledgeNode::new(NodeType::Gotcha, "OAuth callback must validate state")
            .with_summary("Reject a callback whose state does not match the initiating session to prevent CSRF.")).unwrap();
        let hits = store.search_context("debug oauth callback", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node.id, useful.id);
    }

    #[test]
    fn task_recall_does_not_seed_unrelated_claims_from_common_words() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "VPN connection status").with_summary(
                    "The connection requires evidence before it is shown as connected.",
                ),
            )
            .unwrap();
        assert!(store
            .search_context(
                "Verify and fix the persistent background memory worker and its model routing",
                5
            )
            .unwrap()
            .is_empty());
        let memory = store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Background memory worker")
                    .with_summary("Its extraction model is configured separately."),
            )
            .unwrap();
        let hits = store
            .search_context(
                "Verify and fix the persistent background memory worker and its model routing",
                5,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node.id, memory.id);
        assert!(store
            .search_context("please make sure it works", 5)
            .unwrap()
            .is_empty());
    }

    fn terms(raw: &str) -> Option<Vec<String>> {
        fts_terms(raw)
    }

    fn parts(text: &str) -> Vec<String> {
        identifier_parts(text)
            .split(' ')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn camel_case_identifiers_give_up_their_words() {
        // `OAuth` stays whole, because that is how a query spells it; the plain cut and the
        // acronym cut are both kept, so `Auth` is a harmless extra.
        assert_eq!(parts("OAuthCallback"), ["OAuth", "Callback", "Auth"]);
        assert_eq!(parts("AuthService"), ["Auth", "Service"]);
        assert_eq!(parts("parseURL"), ["parse", "URL"]);
        assert_eq!(parts("HTTPServer"), ["HTTP", "Server"]);
    }

    #[test]
    fn digits_join_the_word_before_them() {
        assert_eq!(parts("utf8Decode"), ["utf8", "Decode"]);
        assert_eq!(parts("Base64Encode"), ["Base64", "Encode"]);
        assert_eq!(parts("HTTP2Server"), ["HTTP2", "Server"]);
    }

    #[test]
    fn text_with_no_identifiers_adds_nothing() {
        // Plain words, snake_case and kebab-case are already split by the tokenizer, and a
        // sentence-initial capital is not an identifier.
        assert_eq!(identifier_parts("Sessions expire after 24h"), "");
        assert_eq!(identifier_parts("oauth_callback and oauth-callback"), "");
        assert_eq!(identifier_parts("ALLCAPS API ID"), "");
        assert_eq!(identifier_parts(""), "");
    }

    #[test]
    fn single_characters_are_not_terms() {
        assert!(!parts("OAuthCallback").iter().any(|part| part == "O"));
        // The leading `i` is dropped; the word after it is still worth finding.
        assert_eq!(parts("iPhone"), ["Phone"]);
    }

    #[test]
    fn a_word_is_listed_once_per_identifier() {
        assert_eq!(parts("FooFoo"), ["Foo"]);
    }

    #[test]
    fn a_query_in_words_finds_a_node_named_by_its_identifier() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let node = store
            .create_node(
                KnowledgeNode::new(NodeType::Symbol, "OAuthCallback")
                    .with_summary("Handles the redirect."),
            )
            .unwrap();
        let unrelated = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "Sessions").with_summary("Expire."))
            .unwrap();

        for query in [
            "oauth callback",
            "callback",
            "OAuth",
            "OAuthCallback",
            "auth callback",
        ] {
            let ids: Vec<_> = store
                .search(query, 10)
                .unwrap()
                .into_iter()
                .map(|hit| hit.node.id)
                .collect();
            assert!(
                ids.contains(&node.id),
                "{query:?} should find OAuthCallback"
            );
            assert!(
                !ids.contains(&unrelated.id),
                "{query:?} should not find Sessions"
            );
        }
    }

    #[test]
    fn an_edit_reindexes_the_words_inside_the_new_identifier() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let mut node = store
            .create_node(KnowledgeNode::new(NodeType::Symbol, "Handler"))
            .unwrap();
        assert!(store.search("session store", 5).unwrap().is_empty());

        node.title = "SessionStore".to_string();
        store.update_node(node.clone(), None).unwrap();

        let hits = store.search("session store", 5).unwrap();
        assert_eq!(hits.first().map(|hit| &hit.node.id), Some(&node.id));
    }

    #[test]
    fn a_database_indexed_by_an_older_build_is_reindexed_once_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let mut store = KnowledgeStore::open(dir.path()).unwrap();
            let node = store
                .create_node(KnowledgeNode::new(NodeType::Symbol, "OAuthCallback"))
                .unwrap();
            // What version 1 wrote: the plain search text, and no recorded version.
            store.conn.execute("DELETE FROM nodes_fts", []).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO nodes_fts (id, body) VALUES (?1, ?2)",
                    params![node.id, node.search_text()],
                )
                .unwrap();
            store
                .conn
                .execute(
                    "DELETE FROM meta WHERE key = ?1",
                    params![SEARCH_INDEX_VERSION_KEY],
                )
                .unwrap();
            assert!(store.search("oauth callback", 5).unwrap().is_empty());
            node.id
        };

        let store = KnowledgeStore::open(dir.path()).unwrap();

        let hits = store.search("oauth callback", 5).unwrap();
        assert_eq!(hits.first().map(|hit| &hit.node.id), Some(&id));
    }

    #[test]
    fn an_index_from_a_newer_build_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = KnowledgeStore::open(dir.path()).unwrap();
            store
                .create_node(KnowledgeNode::new(NodeType::Symbol, "OAuthCallback"))
                .unwrap();
            store
                .conn
                .execute(
                    "UPDATE meta SET value = ?2 WHERE key = ?1",
                    params![
                        SEARCH_INDEX_VERSION_KEY,
                        (SEARCH_INDEX_VERSION + 1).to_string()
                    ],
                )
                .unwrap();
            // A sentinel row a rebuild would erase.
            store
                .conn
                .execute(
                    "INSERT INTO nodes_fts (id, body) VALUES ('sentinel', 'zzzsentinel')",
                    [],
                )
                .unwrap();
        }

        let store = KnowledgeStore::open(dir.path()).unwrap();

        let still_there: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM nodes_fts WHERE id = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            still_there, 1,
            "an index recorded by a newer build must not be rebuilt"
        );
    }

    #[test]
    fn reopening_a_current_index_does_not_rebuild_it() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = KnowledgeStore::open(dir.path()).unwrap();
            store
                .create_node(KnowledgeNode::new(NodeType::Symbol, "OAuthCallback"))
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO nodes_fts (id, body) VALUES ('sentinel', 'zzzsentinel')",
                    [],
                )
                .unwrap();
        }

        let store = KnowledgeStore::open(dir.path()).unwrap();

        let still_there: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM nodes_fts WHERE id = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still_there, 1, "a current index must be opened as is");
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
