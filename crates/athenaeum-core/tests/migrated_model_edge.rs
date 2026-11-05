#[cfg(test)]
mod tests {
    use athenaeum_core::id::*;
    use athenaeum_core::model::*;

    #[test]
    fn model_edges_start_as_proposals_and_deterministic_ones_do_not() {
        let a = NodeId::new();
        let b = NodeId::new();
        let guessed = KnowledgeEdge::new(a.clone(), EdgeType::RelatedTo, b.clone());
        assert_eq!(guessed.status, NodeStatus::Proposed);
        assert!(!guessed.is_traversable());

        let derived = KnowledgeEdge::deterministic(a, EdgeType::DependsOn, b, "import statement");
        assert_eq!(derived.status, NodeStatus::Valid);
        assert_eq!(derived.provenance.source, ProvenanceSource::Deterministic);
        assert!(derived.is_traversable());
    }

    #[test]
    fn confidence_is_clamped() {
        let e = KnowledgeEdge::new(NodeId::new(), EdgeType::RelatedTo, NodeId::new())
            .with_confidence(4.2);
        assert_eq!(e.confidence, Some(1.0));
    }
}
