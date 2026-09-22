//! Relationships: creating them, filtering them, and walking them both ways.

use rusqlite::{params, Row};

use super::{parse_enum, parse_time, to_sql_time, KnowledgeStore};
use crate::error::{Error, Result};
use crate::id::{EdgeId, NodeId};
use crate::journal::{EventKind, NewEvent};
use crate::model::{EdgeType, KnowledgeEdge, Neighbor, NodeStatus, Provenance};

const EDGE_COLUMNS: &str = "id, from_node, to_node, type, status, scope, confidence, note, \
     provenance_source, provenance_detail, provenance_session, created_at";

/// Which relationships to follow.
#[derive(Debug, Clone)]
pub struct EdgeFilter {
    /// Restrict to particular relationship types. Empty means all.
    pub types: Vec<EdgeType>,
    /// Skip edges that are merely proposed, superseded or archived.
    pub traversable_only: bool,
    /// Drop edges whose stated confidence is below this. Edges with no
    /// confidence recorded are kept — deterministic edges do not carry one.
    pub min_confidence: Option<f32>,
}

impl Default for EdgeFilter {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            traversable_only: true,
            min_confidence: None,
        }
    }
}

impl EdgeFilter {
    /// Edges suitable for navigation: valid or stale, any type.
    pub fn traversable() -> Self {
        Self::default()
    }

    /// Everything, including proposals awaiting review.
    pub fn all() -> Self {
        Self {
            traversable_only: false,
            ..Default::default()
        }
    }

    pub fn of_types(mut self, types: impl IntoIterator<Item = EdgeType>) -> Self {
        self.types = types.into_iter().collect();
        self
    }

    pub fn min_confidence(mut self, min: f32) -> Self {
        self.min_confidence = Some(min);
        self
    }

    fn admits(&self, edge: &KnowledgeEdge) -> bool {
        if self.traversable_only && !edge.is_traversable() {
            return false;
        }
        if !self.types.is_empty() && !self.types.contains(&edge.edge_type) {
            return false;
        }
        match (self.min_confidence, edge.confidence) {
            (Some(min), Some(actual)) => actual >= min,
            _ => true,
        }
    }
}

impl KnowledgeStore {
    /// Assert a relationship between two nodes.
    ///
    /// Idempotent: asserting the same `(from, type, to)` twice returns the
    /// edge that already exists rather than creating a second one. This is
    /// the mechanism that keeps repeated sessions from slowly turning the
    /// graph into the "semantic hairball" of §21.
    pub fn connect(&mut self, edge: KnowledgeEdge) -> Result<KnowledgeEdge> {
        if edge.from == edge.to {
            return Err(Error::SelfEdge(edge.from));
        }
        for endpoint in [&edge.from, &edge.to] {
            if self.find_node(endpoint)?.is_none() {
                return Err(Error::DanglingEdge(endpoint.clone()));
            }
        }

        if let Some(existing) = self.find_edge_between(&edge.from, &edge.to, edge.edge_type)? {
            return Ok(existing);
        }

        let session = edge.provenance.session.clone();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO edges (id, from_node, to_node, type, status, scope, confidence, note,
                                provenance_source, provenance_detail, provenance_session, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                edge.id,
                edge.from,
                edge.to,
                edge.edge_type.as_str(),
                edge.status.as_str(),
                edge.scope.as_str(),
                edge.confidence,
                edge.note,
                edge.provenance.source.as_str(),
                edge.provenance.detail,
                edge.provenance.session,
                to_sql_time(&edge.created_at),
            ],
        )?;

        let mut event =
            NewEvent::about(EventKind::EdgeCreated, &edge.id).with_detail(serde_json::json!({
                "from": edge.from.as_str(),
                "to": edge.to.as_str(),
                "type": edge.edge_type.as_str(),
                "status": edge.status.as_str(),
            }));
        if let Some(session) = session {
            event = event.with_session(session);
        }
        super::events::record_in(&tx, &event)?;
        tx.commit()?;

        Ok(edge)
    }

    /// Convenience for the common case: assert a model-proposed relationship.
    pub fn relate(
        &mut self,
        from: &NodeId,
        edge_type: EdgeType,
        to: &NodeId,
    ) -> Result<KnowledgeEdge> {
        self.connect(KnowledgeEdge::new(from.clone(), edge_type, to.clone()))
    }

    pub fn find_edge(&self, id: &EdgeId) -> Result<Option<KnowledgeEdge>> {
        let sql = format!("SELECT {EDGE_COLUMNS} FROM edges WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id], map_edge)?;
        match rows.next() {
            Some(edge) => Ok(Some(edge?)),
            None => Ok(None),
        }
    }

    pub fn get_edge(&self, id: &EdgeId) -> Result<KnowledgeEdge> {
        self.find_edge(id)?
            .ok_or_else(|| Error::EdgeNotFound(id.clone()))
    }

    /// The specific edge of `edge_type` between two nodes, if asserted.
    pub fn find_edge_between(
        &self,
        from: &NodeId,
        to: &NodeId,
        edge_type: EdgeType,
    ) -> Result<Option<KnowledgeEdge>> {
        let sql = format!(
            "SELECT {EDGE_COLUMNS} FROM edges WHERE from_node = ?1 AND to_node = ?2 AND type = ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![from, to, edge_type.as_str()], map_edge)?;
        match rows.next() {
            Some(edge) => Ok(Some(edge?)),
            None => Ok(None),
        }
    }

    /// Every node one step away, in either direction.
    ///
    /// Buildplan §35: reaching `OAuthCallbackTest` via `verified_by` must not
    /// be a one-way trip. Edges are stored once; the reverse view is produced
    /// here by inverting the relationship type, so the two can never drift
    /// apart the way a duplicated reverse row would.
    pub fn neighbors(&self, id: &NodeId, filter: &EdgeFilter) -> Result<Vec<Neighbor>> {
        let sql = format!(
            "SELECT {EDGE_COLUMNS} FROM edges WHERE from_node = ?1 OR to_node = ?1
             ORDER BY created_at ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let edges = stmt
            .query_map(params![id], map_edge)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(edges
            .into_iter()
            .filter(|edge| filter.admits(edge))
            .map(|edge| {
                let outbound = &edge.from == id;
                Neighbor {
                    node: if outbound {
                        edge.to.clone()
                    } else {
                        edge.from.clone()
                    },
                    edge_type: if outbound {
                        edge.edge_type
                    } else {
                        edge.edge_type.inverse()
                    },
                    outbound,
                    edge,
                }
            })
            .collect())
    }

    /// Every edge in the base, in creation order.
    ///
    /// The whole-graph counterpart of [`KnowledgeStore::edges_of`], for
    /// callers that would otherwise ask about every node in turn: a review
    /// sweep, a graph projection, an export.
    pub fn list_edges(&self, filter: &EdgeFilter) -> Result<Vec<KnowledgeEdge>> {
        let sql = format!("SELECT {EDGE_COLUMNS} FROM edges ORDER BY created_at ASC, id ASC");
        let mut stmt = self.conn.prepare(&sql)?;
        let edges = stmt
            .query_map([], map_edge)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(edges
            .into_iter()
            .filter(|edge| filter.admits(edge))
            .collect())
    }

    /// Every edge touching a node, unoriented.
    pub fn edges_of(&self, id: &NodeId) -> Result<Vec<KnowledgeEdge>> {
        let sql = format!("SELECT {EDGE_COLUMNS} FROM edges WHERE from_node = ?1 OR to_node = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let edges = stmt
            .query_map(params![id], map_edge)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(edges)
    }

    /// Accept a proposed edge, making it traversable.
    ///
    /// Journaled like every other mutation: this is the moment a relationship
    /// a model merely guessed starts being handed to agents as established
    /// (§20), which is exactly the transition the record exists to preserve.
    pub fn promote_edge(&mut self, id: &EdgeId) -> Result<KnowledgeEdge> {
        let mut edge = self.get_edge(id)?;
        if edge.status == NodeStatus::Valid {
            return Ok(edge);
        }
        let previous = edge.status;
        edge.status = NodeStatus::Valid;

        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE edges SET status = ?2 WHERE id = ?1",
            params![id, NodeStatus::Valid.as_str()],
        )?;
        super::events::record_in(
            &tx,
            &NewEvent::about(EventKind::EdgePromoted, id).with_detail(serde_json::json!({
                "from": previous.as_str(),
                "to": NodeStatus::Valid.as_str(),
                "type": edge.edge_type.as_str(),
            })),
        )?;
        tx.commit()?;

        Ok(edge)
    }

    pub fn remove_edge(&mut self, id: &EdgeId) -> Result<()> {
        let edge = self.get_edge(id)?;
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM edges WHERE id = ?1", params![id])?;
        super::events::record_in(
            &tx,
            &NewEvent::about(EventKind::EdgeRemoved, id).with_detail(serde_json::json!({
                "from": edge.from.as_str(),
                "to": edge.to.as_str(),
                "type": edge.edge_type.as_str(),
            })),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Mark `old` as superseded by `new`, wiring the relationship both into
    /// the graph and into the journal (§44).
    pub fn supersede(&mut self, old: &NodeId, new: &NodeId) -> Result<()> {
        if old == new {
            return Err(Error::InvalidValue {
                field: "supersede",
                value: "a claim cannot supersede itself".to_string(),
            });
        }
        self.connect(KnowledgeEdge::deterministic(
            new.clone(),
            EdgeType::Supersedes,
            old.clone(),
            "supersede",
        ))?;
        self.set_node_status(old, NodeStatus::Superseded)?;
        Ok(())
    }

    /// Record that two claims disagree, and stop offering either until
    /// somebody decides between them (§39's CONTRADICT, §17's lifecycle).
    ///
    /// Both ends are marked, not just one. Which of two disagreeing claims is
    /// wrong is exactly the question nobody has answered yet, and answering
    /// it by picking the older one would be a guess dressed up as a fact.
    /// Neither is deleted: §17 is explicit that old knowledge is kept, and
    /// resolving the dispute — with `supersede`, or by setting the survivor
    /// back to valid — is a separate, deliberate act.
    pub fn contradict(&mut self, a: &NodeId, b: &NodeId) -> Result<KnowledgeEdge> {
        if a == b {
            return Err(Error::InvalidValue {
                field: "contradict",
                value: "a claim cannot contradict itself".to_string(),
            });
        }
        let edge = self.connect(KnowledgeEdge::deterministic(
            a.clone(),
            EdgeType::Contradicts,
            b.clone(),
            "contradict",
        ))?;
        self.set_node_status(a, NodeStatus::Contradicted)?;
        self.set_node_status(b, NodeStatus::Contradicted)?;
        Ok(edge)
    }
}

pub(crate) fn map_edge(row: &Row<'_>) -> rusqlite::Result<KnowledgeEdge> {
    Ok(KnowledgeEdge {
        id: row.get(0)?,
        from: row.get(1)?,
        to: row.get(2)?,
        edge_type: parse_enum(&row.get::<_, String>(3)?, "edge type")?,
        status: parse_enum(&row.get::<_, String>(4)?, "status")?,
        scope: parse_enum(&row.get::<_, String>(5)?, "scope")?,
        confidence: row.get(6)?,
        note: row.get(7)?,
        provenance: Provenance {
            source: parse_enum(&row.get::<_, String>(8)?, "provenance source")?,
            detail: row.get(9)?,
            session: row.get(10)?,
        },
        created_at: parse_time(&row.get::<_, String>(11)?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::EventKind;
    use crate::model::{KnowledgeNode, NodeType};
    use crate::store::EventFilter;

    fn store_with_two_nodes() -> (KnowledgeStore, NodeId, NodeId) {
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let a = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "AuthService"))
            .unwrap();
        let b = store
            .create_node(KnowledgeNode::new(
                NodeType::Gotcha,
                "Session before validation",
            ))
            .unwrap();
        (store, a.id, b.id)
    }

    #[test]
    fn promoting_an_edge_is_journaled() {
        // Promotion is the moment model-guessed knowledge starts being
        // traversed as established (§20). A record that omits it cannot
        // answer "who decided to trust this?".
        let (mut store, a, b) = store_with_two_nodes();
        let edge = store.relate(&a, EdgeType::HasGotcha, &b).unwrap();
        assert_eq!(edge.status, NodeStatus::Proposed);

        let promoted = store.promote_edge(&edge.id).unwrap();
        assert_eq!(promoted.status, NodeStatus::Valid);
        assert!(promoted.is_traversable());

        let events = store
            .events(&EventFilter::about(&edge.id).of_kind(EventKind::EdgePromoted))
            .unwrap();
        assert_eq!(events.len(), 1, "promotion left no journal entry");
        assert_eq!(events[0].detail.as_ref().unwrap()["from"], "proposed");
    }

    #[test]
    fn promoting_an_already_valid_edge_is_a_no_op() {
        // Otherwise re-accepting an edge would pile up journal entries for a
        // transition that never happened.
        let (mut store, a, b) = store_with_two_nodes();
        let edge = store
            .connect(KnowledgeEdge::deterministic(
                a,
                EdgeType::HasGotcha,
                b,
                "scan",
            ))
            .unwrap();

        store.promote_edge(&edge.id).unwrap();
        store.promote_edge(&edge.id).unwrap();

        let events = store
            .events(&EventFilter::about(&edge.id).of_kind(EventKind::EdgePromoted))
            .unwrap();
        assert!(events.is_empty(), "a no-op promotion was journaled");
    }

    #[test]
    fn the_promoted_edge_is_persisted_not_just_returned() {
        let (mut store, a, b) = store_with_two_nodes();
        let edge = store.relate(&a, EdgeType::HasGotcha, &b).unwrap();
        store.promote_edge(&edge.id).unwrap();

        assert_eq!(store.get_edge(&edge.id).unwrap().status, NodeStatus::Valid);
        assert_eq!(
            store
                .neighbors(&a, &EdgeFilter::traversable())
                .unwrap()
                .len(),
            1
        );
    }
}
