//! Node history (buildplan §36).
//!
//! The current row in `nodes` is current knowledge. Everything it used to say
//! lives here, because "Decision_042 v1 said the opposite" is frequently the
//! most useful thing in the database.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{Authority, NodeStatus};
use super::node::KnowledgeNode;
use crate::id::NodeId;

/// An immutable snapshot of a node as it was before an edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersion {
    pub node: NodeId,
    pub version: u32,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub authority: Authority,
    pub status: NodeStatus,
    /// Why the change was made, when the caller supplied a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

impl NodeVersion {
    /// Snapshot a node exactly as it stands, before its replacement is written.
    pub fn snapshot(node: &KnowledgeNode, reason: Option<String>) -> Self {
        Self {
            node: node.id.clone(),
            version: node.version,
            title: node.title.clone(),
            summary: node.summary.clone(),
            content: node.content.clone(),
            authority: node.authority,
            status: node.status,
            reason,
            recorded_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::enums::NodeType;
    use super::*;

    #[test]
    fn snapshot_captures_the_state_before_the_edit() {
        let mut node =
            KnowledgeNode::new(NodeType::Decision, "Use sessions").with_content("v1 body");
        let snap = NodeVersion::snapshot(&node, Some("superseded by PKCE".into()));

        node.content = "v2 body".into();
        node.version = 2;

        assert_eq!(snap.version, 1);
        assert_eq!(snap.content, "v1 body");
        assert_eq!(snap.reason.as_deref(), Some("superseded by PKCE"));
    }
}
