#[cfg(test)]
mod tests {
    use athenaeum_core::model::*;

    #[test]
    fn windows_paths_are_normalized_on_the_way_in() {
        let a = SourceAnchor::file(r"src\auth\oauth.ts");
        assert_eq!(a.file, "src/auth/oauth.ts");
    }

    #[test]
    fn more_locators_means_more_specific() {
        let bare = SourceAnchor::file("src/auth/oauth.ts");
        let precise = SourceAnchor::file("src/auth/oauth.ts")
            .with_symbol("OAuthCallback.handle")
            .with_lines(10, 40)
            .fingerprint("fn handle() {}", "whole file");
        assert!(precise.specificity() > bare.specificity());
    }

    #[test]
    fn hashing_is_stable_and_content_sensitive() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
        assert_eq!(hash("abc").len(), 64);
    }

    #[test]
    fn only_stale_counts_as_unresolved() {
        assert!(AnchorResolution::Exact.is_resolved());
        assert!(AnchorResolution::RelocatedBySymbol.is_resolved());
        assert!(!AnchorResolution::Stale.is_resolved());
    }
}
