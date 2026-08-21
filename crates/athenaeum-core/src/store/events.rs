//! Writing to and reading from the append-only journal.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{parse_enum, parse_time, to_sql_time, KnowledgeStore};
use crate::error::Result;
use crate::id::NodeId;
use crate::journal::{Event, EventKind, NewEvent, NodeUtility};

/// Which journal entries to read back.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub kind: Option<EventKind>,
    /// The node or edge id an event concerns.
    pub subject: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

impl EventFilter {
    pub fn about(subject: impl std::fmt::Display) -> Self {
        Self {
            subject: Some(subject.to_string()),
            ..Default::default()
        }
    }

    pub fn of_kind(mut self, kind: EventKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn since(mut self, since: DateTime<Utc>) -> Self {
        self.since = Some(since);
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl KnowledgeStore {
    /// Append an event.
    ///
    /// Use this to record retrieval and use, which is how a node's real
    /// utility gets measured rather than assumed (§41).
    pub fn record(&mut self, event: NewEvent) -> Result<()> {
        record_in(&self.conn, &event)
    }

    /// Record that a set of nodes was returned to an agent.
    pub fn record_retrieval(&mut self, ids: &[NodeId]) -> Result<()> {
        self.record_node_events(EventKind::NodeRetrieved, ids, None)
    }

    /// Record that an agent actually used a set of retrieved nodes.
    ///
    /// The external runtime reports this after completing work. Keeping the
    /// write here, rather than letting it touch the journal directly, makes
    /// the whole report atomic and preserves the storage boundary.
    pub fn record_use(
        &mut self,
        ids: &[NodeId],
        session: Option<&crate::id::SessionId>,
    ) -> Result<()> {
        self.record_node_events(EventKind::NodeUsed, ids, session)
    }

    fn record_node_events(
        &mut self,
        kind: EventKind,
        ids: &[NodeId],
        session: Option<&crate::id::SessionId>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for id in ids {
            let mut event = NewEvent::about(kind, id);
            if let Some(session) = session {
                event = event.with_session(session.clone());
            }
            record_in(&tx, &event)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Read the journal, newest first.
    pub fn events(&self, filter: &EventFilter) -> Result<Vec<Event>> {
        let mut sql = String::from(
            "SELECT seq, at, kind, subject, session, detail_json FROM journal WHERE 1 = 1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(kind) = filter.kind {
            args.push(Box::new(kind.as_str()));
            sql.push_str(&format!(" AND kind = ?{}", args.len()));
        }
        if let Some(subject) = &filter.subject {
            args.push(Box::new(subject.clone()));
            sql.push_str(&format!(" AND subject = ?{}", args.len()));
        }
        if let Some(since) = &filter.since {
            args.push(Box::new(to_sql_time(since)));
            sql.push_str(&format!(" AND at >= ?{}", args.len()));
        }
        sql.push_str(" ORDER BY seq DESC");
        if let Some(limit) = filter.limit {
            args.push(Box::new(limit));
            sql.push_str(&format!(" LIMIT ?{}", args.len()));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let events = stmt
            .query_map(refs.as_slice(), |row| {
                let detail: Option<String> = row.get(5)?;
                Ok(Event {
                    seq: row.get(0)?,
                    at: parse_time(&row.get::<_, String>(1)?)?,
                    kind: parse_enum(&row.get::<_, String>(2)?, "event kind")?,
                    subject: row.get(3)?,
                    session: row.get(4)?,
                    detail: detail
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(events)
    }

    /// How useful a node has actually been, counted from the journal.
    ///
    /// Derived on demand rather than cached on the node, so that reading
    /// knowledge never rewrites it (§41).
    pub fn utility(&self, id: &NodeId) -> Result<NodeUtility> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM journal WHERE subject = ?1 GROUP BY kind")?;
        let counts = stmt
            .query_map(params![id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u32))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut utility = NodeUtility::default();
        for (kind, count) in counts {
            match kind.parse::<EventKind>() {
                Ok(EventKind::NodeRetrieved) => utility.retrievals = count,
                Ok(EventKind::NodeUsed) => utility.uses = count,
                Ok(EventKind::NodeCited) => utility.citations = count,
                Ok(EventKind::NodeVerified) => utility.verifications = count,
                Ok(EventKind::NodeRejected) => utility.rejections = count,
                _ => {}
            }
        }
        Ok(utility)
    }
}

/// Append an event using an existing connection or transaction.
///
/// Taking a `&Connection` rather than `&mut self` is what lets a write and
/// its journal entry share one transaction: either both land or neither does.
pub(crate) fn record_in(conn: &Connection, event: &NewEvent) -> Result<()> {
    let detail = event
        .detail
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO journal (at, kind, subject, session, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            to_sql_time(&Utc::now()),
            event.kind.as_str(),
            event.subject,
            event.session,
            detail,
        ],
    )?;
    Ok(())
}
