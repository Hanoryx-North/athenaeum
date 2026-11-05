#[cfg(test)]
mod tests {
    use athenaeum_core::id::*;

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
