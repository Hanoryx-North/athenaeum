//! Typed identifiers.
//!
//! Nodes, edges and evidence all carry UUID-shaped ids, which makes them
//! trivially easy to mix up when every one of them is a bare `String`. These
//! newtypes cost nothing at runtime and mean the compiler rejects
//! `get_node(edge.id)` instead of the database returning "not found" at 2am.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Mint a fresh identifier.
            pub fn new() -> Self {
                Self(format!("{}_{}", $prefix, uuid::Uuid::new_v4().simple()))
            }

            /// Adopt an existing identifier, e.g. one read back from the
            /// database or supplied by the caller so that a node has a
            /// stable, human-meaningful id.
            pub fn from_existing(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::Borrowed(
                    rusqlite::types::ValueRef::Text(self.0.as_bytes()),
                ))
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                value.as_str().map(|s| Self(s.to_string()))
            }
        }
    };
}

id_type!(
    /// Identifies a [`crate::model::KnowledgeNode`].
    NodeId,
    "node"
);
id_type!(
    /// Identifies a [`crate::model::KnowledgeEdge`].
    EdgeId,
    "edge"
);
id_type!(
    /// Identifies a piece of [`crate::model::Evidence`].
    EvidenceId,
    "ev"
);
id_type!(
    /// Identifies an agent session that produced knowledge.
    SessionId,
    "sess"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_prefixed() {
        let a = NodeId::new();
        let b = NodeId::new();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("node_"), "got {a}");
        assert!(EdgeId::new().as_str().starts_with("edge_"));
        assert!(EvidenceId::new().as_str().starts_with("ev_"));
    }

    #[test]
    fn adopted_ids_are_preserved_verbatim() {
        // Callers are allowed to pick readable ids like "Decision_042"
        // (the buildplan uses these throughout); we must not rewrite them.
        let id = NodeId::from_existing("Decision_042");
        assert_eq!(id.as_str(), "Decision_042");
    }
}
