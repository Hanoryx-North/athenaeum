#[cfg(test)]
mod tests {
    use athenaeum_core::model::*;

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
