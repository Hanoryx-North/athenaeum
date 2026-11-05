#[cfg(test)]
mod tests {
    use athenaeum_core::model::*;

    #[test]
    fn new_nodes_are_unverified_by_default() {
        let n = KnowledgeNode::new(NodeType::Fact, "Sessions expire after 24h");
        assert_eq!(n.authority, Authority::UnverifiedObservation);
        assert_eq!(n.version, 1);
        assert!(n.is_live());
        assert!(!n.needs_review());
    }

    #[test]
    fn superseded_nodes_are_retained_but_not_live() {
        let n = KnowledgeNode::new(NodeType::Decision, "Use sessions")
            .with_status(NodeStatus::Superseded);
        assert!(!n.is_live());
    }

    #[test]
    fn stale_nodes_stay_live_because_staleness_is_not_falsehood() {
        // §17: a node can be high-confidence and stale at the same time.
        let n = KnowledgeNode::new(NodeType::Fact, "OAuth validates state")
            .with_status(NodeStatus::Stale)
            .with_authority(Authority::ProjectDecision);
        assert!(n.is_live());
    }

    #[test]
    fn a_title_match_outranks_a_body_match() {
        // The inline original asserted the private `search_text` repeats the title; the behaviour
        // that matters, and the one reachable from the public interface, is the ranking it buys.
        let mut store = athenaeum_core::store::KnowledgeStore::open_in_memory().unwrap();
        let in_body = store
            .create_node(
                KnowledgeNode::new(NodeType::Concept, "Session handling")
                    .with_content("covers OAuth callbacks"),
            )
            .unwrap();
        let in_title = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "OAuth").with_content("body"))
            .unwrap();
        let hits = store.search("OAuth", 5).unwrap();
        assert_eq!(
            hits.first().map(|hit| hit.node.id.clone()),
            Some(in_title.id),
            "{hits:?}"
        );
        assert!(hits.iter().any(|hit| hit.node.id == in_body.id));
    }
}
