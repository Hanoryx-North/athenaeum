#[cfg(test)]
mod tests {
    use athenaeum_core::id::*;
    use athenaeum_core::model::*;

    #[test]
    fn citations_prefer_the_symbol_then_the_line() {
        let node = NodeId::new();
        let with_symbol = Evidence::new(node.clone(), EvidenceKind::Symbol).with_anchor(
            SourceAnchor::file("src/auth/oauth.ts").with_symbol("OAuthCallback.handle"),
        );
        assert_eq!(
            with_symbol.citation(),
            "src/auth/oauth.ts:OAuthCallback.handle"
        );

        let with_line = Evidence::new(node.clone(), EvidenceKind::SourceFile)
            .with_anchor(SourceAnchor::file("src/auth/oauth.ts").with_lines(42, 50));
        assert_eq!(with_line.citation(), "src/auth/oauth.ts:42");

        let anchorless =
            Evidence::new(node, EvidenceKind::UserStatement).with_note("user said so in session 3");
        assert_eq!(anchorless.citation(), "user said so in session 3");
    }

    #[test]
    fn long_excerpts_are_flagged_for_the_blob_store() {
        let node = NodeId::new();
        let short = Evidence::new(node.clone(), EvidenceKind::SourceFile).with_excerpt("fn x() {}");
        assert!(!short.excerpt_needs_blob());
        let long = Evidence::new(node, EvidenceKind::SourceFile)
            .with_excerpt("x".repeat(INLINE_EXCERPT_LIMIT + 1));
        assert!(long.excerpt_needs_blob());
    }
}
