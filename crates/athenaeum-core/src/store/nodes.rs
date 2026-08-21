//! Node create / read / update / delete, with versioning and journaling.

use rusqlite::{params, Row};

use super::{parse_enum, parse_time, to_sql_time, KnowledgeStore};
use crate::error::{Error, Result};
use crate::id::{NodeId, SessionId};
use crate::journal::{EventKind, NewEvent};
use crate::model::{
    Authority, KnowledgeNode, NodeStatus, NodeType, NodeVersion, Provenance, Scope,
};

/// The node columns, in the order [`map_node`] reads them by position.
///
/// Declared once and shared with [`super::search`], because a second
/// hand-written copy that drifts out of order would not fail to compile — it
/// would quietly map one column's value into another column's field.
pub(crate) const NODE_COLUMNS: &str =
    "id, type, title, summary, content, scope, authority, status, \
     provenance_source, provenance_detail, provenance_session, last_verified_at, \
     created_at, updated_at, version";

/// [`NODE_COLUMNS`] qualified with a table alias, for queries that join.
pub(crate) fn node_columns_for(alias: &str) -> String {
    NODE_COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Which nodes to list.
///
/// Defaults to "everything that is currently live", because handing an agent
/// superseded knowledge as though it were current is the failure mode this
/// whole design exists to avoid.
#[derive(Debug, Clone, Default)]
pub struct NodeFilter {
    pub node_type: Option<NodeType>,
    pub status: Option<NodeStatus>,
    pub scope: Option<Scope>,
    pub authority: Option<Authority>,
    /// When true (the default), superseded and archived nodes are excluded.
    pub live_only: bool,
    pub limit: Option<u32>,
}

impl NodeFilter {
    /// Live nodes only.
    pub fn live() -> Self {
        Self {
            live_only: true,
            ..Default::default()
        }
    }

    /// Everything, including superseded and archived history.
    pub fn all() -> Self {
        Self::default()
    }

    pub fn of_type(mut self, node_type: NodeType) -> Self {
        self.node_type = Some(node_type);
        self
    }

    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = Some(authority);
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl KnowledgeStore {
    /// Insert a node.
    ///
    /// Fails if the id is already taken; use [`KnowledgeStore::update_node`]
    /// to change an existing one so that its history is preserved.
    pub fn create_node(&mut self, node: KnowledgeNode) -> Result<KnowledgeNode> {
        let session = node.provenance.session.clone();
        let tx = self.conn.transaction()?;

        // `.ok()` here would discard a genuine SQLite failure as though it
        // meant "no such row", and the caller would then see a confusing
        // constraint error instead of the real fault.
        match tx.query_row(
            "SELECT 1 FROM nodes WHERE id = ?1",
            params![node.id],
            |_| Ok(()),
        ) {
            Ok(()) => return Err(Error::DuplicateId(node.id.clone())),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }

        tx.execute(
            "INSERT INTO nodes (id, type, title, title_key, summary, content, scope, authority, status,
                                provenance_source, provenance_detail, provenance_session,
                                last_verified_at, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                node.id,
                node.node_type.as_str(),
                node.title,
                title_key(&node.title),
                node.summary,
                node.content,
                node.scope.as_str(),
                node.authority.as_str(),
                node.status.as_str(),
                node.provenance.source.as_str(),
                node.provenance.detail,
                node.provenance.session,
                node.last_verified_at.as_ref().map(to_sql_time),
                to_sql_time(&node.created_at),
                to_sql_time(&node.updated_at),
                node.version,
            ],
        )?;

        super::search::index_node(&tx, &node)?;

        let mut event = NewEvent::about(EventKind::NodeCreated, &node.id).with_detail(
            serde_json::json!({ "type": node.node_type.as_str(), "title": node.title }),
        );
        if let Some(session) = session {
            event = event.with_session(session);
        }
        super::events::record_in(&tx, &event)?;

        tx.commit()?;
        Ok(node)
    }

    /// Fetch a node, or `None` if there is no such id.
    pub fn find_node(&self, id: &NodeId) -> Result<Option<KnowledgeNode>> {
        let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id], map_node)?;
        match rows.next() {
            Some(node) => Ok(Some(node?)),
            None => Ok(None),
        }
    }

    /// Fetch a node, erroring if it is missing.
    pub fn get_node(&self, id: &NodeId) -> Result<KnowledgeNode> {
        self.find_node(id)?
            .ok_or_else(|| Error::NodeNotFound(id.clone()))
    }

    /// Look a node up by title, case-insensitively.
    ///
    /// This is the first move of entity resolution: before creating
    /// `AuthService`, the curator asks whether `authservice` is already known,
    /// so that discoveries attach to existing knowledge instead of spawning
    /// near-duplicates (§39, §40).
    pub fn find_node_by_title(&self, title: &str) -> Result<Option<KnowledgeNode>> {
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes
             WHERE title = ?1 COLLATE NOCASE
             ORDER BY CASE status WHEN 'valid' THEN 0 ELSE 1 END, updated_at DESC
             LIMIT 1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![title], map_node)?;
        match rows.next() {
            Some(node) => Ok(Some(node?)),
            None => Ok(None),
        }
    }

    /// Find conservative title lookalikes without scanning the node table.
    ///
    /// The key intentionally recognizes only case, whitespace, `_` and `-`
    /// variations. Plurals, abbreviations and spelling similarity still need
    /// human or curator judgement; silently merging those would be worse than
    /// retaining a duplicate.
    pub fn find_nodes_by_title_key(&self, title: &str) -> Result<Vec<KnowledgeNode>> {
        let key = title_key(title);
        if key.chars().count() < 5 {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes
             WHERE title_key = ?1 AND title != ?2 COLLATE NOCASE
             ORDER BY CASE status WHEN 'valid' THEN 0 ELSE 1 END, updated_at DESC, id ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let nodes = stmt
            .query_map(params![key, title], map_node)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(nodes)
    }

    /// List nodes matching `filter`, most recently updated first.
    pub fn list_nodes(&self, filter: &NodeFilter) -> Result<Vec<KnowledgeNode>> {
        let mut sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE 1 = 1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(t) = filter.node_type {
            args.push(Box::new(t.as_str()));
            sql.push_str(&format!(" AND type = ?{}", args.len()));
        }
        if let Some(s) = filter.status {
            args.push(Box::new(s.as_str()));
            sql.push_str(&format!(" AND status = ?{}", args.len()));
        }
        if let Some(s) = filter.scope {
            args.push(Box::new(s.as_str()));
            sql.push_str(&format!(" AND scope = ?{}", args.len()));
        }
        if let Some(a) = filter.authority {
            args.push(Box::new(a.as_str()));
            sql.push_str(&format!(" AND authority = ?{}", args.len()));
        }
        if filter.live_only {
            sql.push_str(" AND status IN ('valid', 'stale', 'stale_requires_review')");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        if let Some(limit) = filter.limit {
            args.push(Box::new(limit));
            sql.push_str(&format!(" LIMIT ?{}", args.len()));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let nodes = stmt
            .query_map(refs.as_slice(), map_node)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(nodes)
    }

    /// Replace a node's contents, archiving what it said before.
    ///
    /// The previous body is snapshotted into `node_versions` and the version
    /// counter is bumped, so "what did Decision_042 say before PKCE?" stays
    /// answerable forever (§36).
    pub fn update_node(
        &mut self,
        node: KnowledgeNode,
        reason: Option<String>,
    ) -> Result<KnowledgeNode> {
        let actor = node.provenance.clone();
        self.update_node_with_actor(node, reason, actor)
    }

    /// Replace a node while attributing the edit independently from the
    /// claim's original provenance.
    ///
    /// A model editing a user-stated decision must not turn that decision
    /// into a model-originated claim. The actor belongs in the journal; the
    /// node keeps describing where the knowledge itself came from.
    pub fn update_node_attributed(
        &mut self,
        node: KnowledgeNode,
        reason: Option<String>,
        actor: Provenance,
    ) -> Result<KnowledgeNode> {
        self.update_node_with_actor(node, reason, actor)
    }

    fn update_node_with_actor(
        &mut self,
        node: KnowledgeNode,
        reason: Option<String>,
        actor: Provenance,
    ) -> Result<KnowledgeNode> {
        let previous = self.get_node(&node.id)?;

        let mut updated = node;
        updated.version = previous.version + 1;
        updated.updated_at = chrono::Utc::now();
        updated.created_at = previous.created_at;

        let snapshot = NodeVersion::snapshot(&previous, reason);
        let session = actor.session.clone();

        let tx = self.conn.transaction()?;

        tx.execute(
            "INSERT INTO node_versions
                (node, version, title, summary, content, authority, status, reason, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                snapshot.node,
                snapshot.version,
                snapshot.title,
                snapshot.summary,
                snapshot.content,
                snapshot.authority.as_str(),
                snapshot.status.as_str(),
                snapshot.reason,
                to_sql_time(&snapshot.recorded_at),
            ],
        )?;

        tx.execute(
            "UPDATE nodes SET type = ?2, title = ?3, title_key = ?4, summary = ?5,
                              content = ?6, scope = ?7, authority = ?8, status = ?9,
                              provenance_source = ?10, provenance_detail = ?11,
                              provenance_session = ?12, last_verified_at = ?13,
                              updated_at = ?14, version = ?15
             WHERE id = ?1",
            params![
                updated.id,
                updated.node_type.as_str(),
                updated.title,
                title_key(&updated.title),
                updated.summary,
                updated.content,
                updated.scope.as_str(),
                updated.authority.as_str(),
                updated.status.as_str(),
                updated.provenance.source.as_str(),
                updated.provenance.detail,
                updated.provenance.session,
                updated.last_verified_at.as_ref().map(to_sql_time),
                to_sql_time(&updated.updated_at),
                updated.version,
            ],
        )?;

        super::search::reindex_node(&tx, &updated)?;

        let mut event =
            NewEvent::about(EventKind::NodeUpdated, &updated.id).with_detail(serde_json::json!({
                "version": updated.version,
                "actorSource": actor.source.as_str(),
                "actorDetail": actor.detail,
            }));
        if let Some(session) = session {
            event = event.with_session(session);
        }
        super::events::record_in(&tx, &event)?;

        tx.commit()?;
        Ok(updated)
    }

    /// Move a node to a new status, journaling the transition.
    ///
    /// Preferred over a full [`KnowledgeStore::update_node`] when only the
    /// lifecycle changed: marking something stale is not an edit to what it
    /// claims, and should not create a new version.
    pub fn set_node_status(&mut self, id: &NodeId, status: NodeStatus) -> Result<KnowledgeNode> {
        let mut node = self.get_node(id)?;
        if node.status == status {
            return Ok(node);
        }

        let kind = match status {
            NodeStatus::Superseded => EventKind::KnowledgeSuperseded,
            NodeStatus::Contradicted => EventKind::KnowledgeContradicted,
            NodeStatus::Stale | NodeStatus::StaleRequiresReview => EventKind::KnowledgeStale,
            _ => EventKind::NodeUpdated,
        };
        let previous = node.status;
        node.status = status;
        node.updated_at = chrono::Utc::now();

        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE nodes SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![node.id, status.as_str(), to_sql_time(&node.updated_at)],
        )?;
        super::events::record_in(
            &tx,
            &NewEvent::about(kind, &node.id).with_detail(
                serde_json::json!({ "from": previous.as_str(), "to": status.as_str() }),
            ),
        )?;
        tx.commit()?;

        Ok(node)
    }

    /// Record that a node was confirmed against its evidence.
    pub fn mark_verified(
        &mut self,
        id: &NodeId,
        authority: Option<Authority>,
    ) -> Result<KnowledgeNode> {
        let mut node = self.get_node(id)?;
        node.last_verified_at = Some(chrono::Utc::now());
        node.status = NodeStatus::Valid;
        if let Some(authority) = authority {
            node.authority = authority;
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE nodes SET status = ?2, authority = ?3, last_verified_at = ?4 WHERE id = ?1",
            params![
                node.id,
                node.status.as_str(),
                node.authority.as_str(),
                node.last_verified_at.as_ref().map(to_sql_time),
            ],
        )?;
        super::events::record_in(&tx, &NewEvent::about(EventKind::NodeVerified, &node.id))?;
        tx.commit()?;

        Ok(node)
    }

    /// Delete a node outright, along with its edges, evidence and history.
    ///
    /// Rarely the right call: superseding preserves the record, and history
    /// is usually the valuable part (§17). Offered because a user must be
    /// able to remove knowledge from their own machine.
    pub fn delete_node(&mut self, id: &NodeId) -> Result<()> {
        let node = self.get_node(id)?;
        let tx = self.conn.transaction()?;
        // Edges, evidence and versions go with it via ON DELETE CASCADE.
        tx.execute("DELETE FROM nodes WHERE id = ?1", params![id])?;
        super::search::unindex_node(&tx, id)?;
        super::events::record_in(
            &tx,
            &NewEvent::about(EventKind::NodeDeleted, id)
                .with_detail(serde_json::json!({ "title": node.title })),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Every archived revision of a node, newest first.
    pub fn history(&self, id: &NodeId) -> Result<Vec<NodeVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT node, version, title, summary, content, authority, status, reason, recorded_at
             FROM node_versions WHERE node = ?1 ORDER BY version DESC",
        )?;
        let versions = stmt
            .query_map(params![id], |row| {
                Ok(NodeVersion {
                    node: row.get(0)?,
                    version: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    content: row.get(4)?,
                    authority: parse_enum(&row.get::<_, String>(5)?, "authority")?,
                    status: parse_enum(&row.get::<_, String>(6)?, "status")?,
                    reason: row.get(7)?,
                    recorded_at: parse_time(&row.get::<_, String>(8)?)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(versions)
    }

    /// Note that an agent session produced this node, for later attribution.
    pub fn attribute_to_session(&mut self, id: &NodeId, session: SessionId) -> Result<()> {
        self.conn.execute(
            "UPDATE nodes SET provenance_session = ?2 WHERE id = ?1",
            params![id, session],
        )?;
        Ok(())
    }
}

pub(crate) fn map_node(row: &Row<'_>) -> rusqlite::Result<KnowledgeNode> {
    let last_verified: Option<String> = row.get(11)?;
    Ok(KnowledgeNode {
        id: row.get(0)?,
        node_type: parse_enum(&row.get::<_, String>(1)?, "node type")?,
        title: row.get(2)?,
        summary: row.get(3)?,
        content: row.get(4)?,
        scope: parse_enum(&row.get::<_, String>(5)?, "scope")?,
        authority: parse_enum(&row.get::<_, String>(6)?, "authority")?,
        status: parse_enum(&row.get::<_, String>(7)?, "status")?,
        provenance: Provenance {
            source: parse_enum(&row.get::<_, String>(8)?, "provenance source")?,
            detail: row.get(9)?,
            session: row.get(10)?,
        },
        last_verified_at: last_verified.as_deref().map(parse_time).transpose()?,
        created_at: parse_time(&row.get::<_, String>(12)?)?,
        updated_at: parse_time(&row.get::<_, String>(13)?)?,
        version: row.get(14)?,
    })
}

/// Stable, deliberately conservative key shared by migration-compatible
/// writes and Wiki-boundary entity resolution.
pub(crate) fn title_key(title: &str) -> String {
    title
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '-' && *ch != '_')
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeType;

    #[test]
    fn a_clashing_id_is_reported_as_an_id_clash_not_a_title_clash() {
        // A caller supplying its own readable id needs to be told which id
        // collided. Naming the incoming title instead points at a node that
        // does not exist.
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Validate state first")
                    .with_id("Decision_042"),
            )
            .unwrap();

        let err = store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Something else").with_id("Decision_042"),
            )
            .unwrap_err();

        let message = err.to_string();
        assert!(matches!(err, Error::DuplicateId(_)), "got {err:?}");
        assert!(message.contains("Decision_042"), "got {message}");
        assert!(!message.contains("Something else"), "got {message}");
    }

    #[test]
    fn a_rejected_duplicate_leaves_nothing_behind() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        store
            .create_node(KnowledgeNode::new(NodeType::Fact, "One").with_id("dup"))
            .unwrap();
        let _ = store.create_node(KnowledgeNode::new(NodeType::Fact, "Two").with_id("dup"));

        assert_eq!(store.stats().unwrap().nodes, 1);
        assert_eq!(
            store.get_node(&NodeId::from_existing("dup")).unwrap().title,
            "One"
        );
    }

    #[test]
    fn the_column_list_and_its_aliased_form_stay_in_step() {
        // `map_node` reads by position, so these must describe the same
        // columns in the same order or rows silently map into wrong fields.
        let plain: Vec<&str> = NODE_COLUMNS.split(',').map(str::trim).collect();
        let aliased = node_columns_for("n");
        let qualified: Vec<String> = aliased.split(',').map(|c| c.trim().to_string()).collect();

        assert_eq!(plain.len(), qualified.len());
        for (plain, qualified) in plain.iter().zip(&qualified) {
            assert_eq!(qualified, &format!("n.{plain}"));
        }
    }

    #[test]
    fn near_title_lookup_is_separator_aware_but_not_fuzzy() {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let existing = store
            .create_node(KnowledgeNode::new(NodeType::Symbol, "AuthService"))
            .unwrap();

        let matches = store.find_nodes_by_title_key("auth_service").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, existing.id);
        assert!(store
            .find_nodes_by_title_key("AuthServices")
            .unwrap()
            .is_empty());
    }
}
