//! Error type for the knowledge library.

use thiserror::Error;

use crate::id::{EdgeId, EvidenceId, NodeId};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no node with id {0}")]
    NodeNotFound(NodeId),

    #[error("no edge with id {0}")]
    EdgeNotFound(EdgeId),

    #[error("no evidence with id {0}")]
    EvidenceNotFound(EvidenceId),

    /// Raised when creating a node whose id is already taken. Distinct from
    /// a title clash: ids are the primary key, and a caller supplying its own
    /// readable id (`Decision_042`) needs to be told which id collided, not
    /// which title.
    #[error("a node with id {0} already exists")]
    DuplicateId(NodeId),

    /// Exact-title entity resolution at the Wiki boundary. The store permits
    /// duplicate titles because humans can legitimately name two concepts the
    /// same way; model curation must make that ambiguity explicit instead of
    /// quietly bloating the graph.
    #[error("a node titled {title:?} already exists as {existing}; update it or choose a distinct title")]
    DuplicateTitle { title: String, existing: NodeId },

    /// A high-precision identifier-style title match. This is separate from
    /// an exact title clash so a curator can explain why it selected the
    /// existing entity and attach new evidence to it.
    #[error(
        "the title {title:?} is a near-duplicate of existing node(s) [{candidates}]; attach evidence to an existing node or choose a clearly distinct title",
        candidates = format_node_ids(.existing)
    )]
    NearDuplicateTitle {
        title: String,
        existing: Vec<NodeId>,
    },

    /// No `.athenaeum/` directory was found. Reported in its own terms
    /// because this is the first error most users ever see, and "blob store:
    /// not found" tells them nothing about what to do next.
    #[error("no knowledge base found from {path}; run `athenaeum init` first")]
    NoWorkspace { path: String },

    /// Raised when an edge would point at a node that is not in the graph.
    /// Enforced by a foreign key, but reported in these terms because
    /// "FOREIGN KEY constraint failed" tells a caller nothing.
    #[error("edge references node {0}, which does not exist")]
    DanglingEdge(NodeId),

    #[error("a node cannot be related to itself ({0})")]
    SelfEdge(NodeId),

    #[error("invalid {field}: {value:?}")]
    InvalidValue { field: &'static str, value: String },

    #[error("node {id} failed validation: {reason}")]
    Invalid { id: NodeId, reason: String },

    #[error("knowledge store at {path}: {source}")]
    Storage {
        path: String,
        #[source]
        source: rusqlite::Error,
    },

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Any filesystem failure: creating the workspace, reading or writing a
    /// blob. Not named for the blob store, because it is not only the blob
    /// store that reaches the disk.
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("repository parser initialization failed for {language}: {message}")]
    RepositoryParser {
        language: &'static str,
        message: String,
    },

    #[error(
        "repository scan was incomplete; cannot {operation} safely without raising its scan limits"
    )]
    IncompleteRepositoryScan { operation: &'static str },

    /// The portable format or a sync refused something. The code is the protocol's, so a front end
    /// can report `invalid_path` or `conflict` without parsing the message.
    #[error("{message}")]
    Portable { code: String, message: String },

    /// The database on disk was written by a newer build than this one.
    /// Refusing is the correct move: a downgrade that silently drops columns
    /// destroys the user's knowledge (§61).
    #[error("knowledge base is at schema v{found}, but this build supports up to v{supported}; upgrade Athenaeum to open it")]
    SchemaTooNew { found: u32, supported: u32 },
}

fn format_node_ids(ids: &[NodeId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl Error {
    /// True when retrying the same call could plausibly succeed — used to
    /// decide whether a failed write is worth surfacing to the user.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Sqlite(rusqlite::Error::SqliteFailure(e, _)) => matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ),
            Error::Storage {
                source: rusqlite::Error::SqliteFailure(e, _),
                ..
            } => matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_names_the_id() {
        let err = Error::NodeNotFound(NodeId::from_existing("Decision_042"));
        assert!(err.to_string().contains("Decision_042"));
    }

    #[test]
    fn schema_downgrade_is_refused_with_an_actionable_message() {
        let err = Error::SchemaTooNew {
            found: 9,
            supported: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("upgrade Athenaeum"), "got {msg}");
    }

    #[test]
    fn plain_errors_are_not_transient() {
        assert!(!Error::SelfEdge(NodeId::new()).is_transient());
    }
}
