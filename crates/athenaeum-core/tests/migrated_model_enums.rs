#[cfg(test)]
mod tests {
    use athenaeum_core::model::enums::EvidenceRequirement;
    use athenaeum_core::model::*;

    #[test]
    fn round_trips_through_string_form() {
        for t in NodeType::ALL {
            assert_eq!(t.as_str().parse::<NodeType>(), Ok(*t));
        }
        for t in EdgeType::ALL {
            assert_eq!(t.as_str().parse::<EdgeType>(), Ok(*t));
        }
        for a in Authority::ALL {
            assert_eq!(a.as_str().parse::<Authority>(), Ok(*a));
        }
    }

    #[test]
    fn every_node_type_is_either_structure_or_a_claim() {
        // Adding a type without deciding which it is would silently put it
        // on the claim side and start reporting disputes about it.
        let structural = NodeType::ALL.iter().filter(|t| t.is_structural()).count();
        assert_eq!(structural, 8, "a node type changed side");
        assert!(NodeType::Symbol.is_structural());
        assert!(!NodeType::Rule.is_structural());
        assert!(!NodeType::Concept.is_structural());
    }

    #[test]
    fn an_unknown_value_names_the_vocabulary_it_failed_against() {
        let err = "symbol".parse::<EdgeType>().unwrap_err();
        assert_eq!(err.vocabulary, "EdgeType");
        assert_eq!(err.to_string(), r#"unknown EdgeType: "symbol""#);
        assert_eq!(EdgeType::parse_opt("symbol"), None);
    }

    #[test]
    fn every_inverse_is_involutive() {
        // If this ever fails, reverse traversal would land somewhere the
        // forward traversal cannot return from.
        for t in EdgeType::ALL {
            assert_eq!(t.inverse().inverse(), *t, "{t} inverse is not involutive");
        }
    }

    #[test]
    fn authority_ranks_are_unique_and_ordered() {
        let mut ranks: Vec<u8> = Authority::ALL.iter().map(|a| a.rank()).collect();
        let count = ranks.len();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), count, "two authority levels share a rank");
        assert!(Authority::UserInstruction.outranks(&Authority::ProjectRule));
        assert!(Authority::ProjectRule.outranks(&Authority::UnverifiedObservation));
        assert!(!Authority::ExternalKnowledge.outranks(&Authority::UserInstruction));
    }

    #[test]
    fn a_rule_cannot_be_promoted_by_a_model_reading_its_own_output() {
        // §47: only authoritative evidence promotes a rule.
        let bar = Authority::ProjectRule.evidence_required();
        assert_eq!(bar, EvidenceRequirement::Authoritative);
        assert!(EvidenceKind::Documentation.establishes(bar));
        assert!(EvidenceKind::Configuration.establishes(bar));
        assert!(EvidenceKind::UserStatement.establishes(bar));
        assert!(!EvidenceKind::ExternalSource.establishes(bar));
        // A file containing generated code shows what *is*, not what should
        // be, so it does not establish a rule about what to do with it.
        assert!(!EvidenceKind::SourceFile.establishes(bar));

        // A fact about behaviour is a different question, and the code is
        // exactly the right evidence for it.
        let checked = Authority::VerifiedEvidence.evidence_required();
        assert!(EvidenceKind::SourceFile.establishes(checked));
        assert!(EvidenceKind::Test.establishes(checked));
        assert!(!EvidenceKind::ExternalSource.establishes(checked));

        // Nothing verifies its way into being something the user said.
        let never = Authority::UserInstruction.evidence_required();
        assert!(EvidenceKind::ALL
            .iter()
            .all(|kind| !kind.establishes(never)));
    }

    #[test]
    fn unverified_is_the_default_authority() {
        assert_eq!(Authority::default(), Authority::UnverifiedObservation);
    }

    #[test]
    fn display_honours_width_so_tables_line_up() {
        // Regression: a Display impl written with `write_str` ignores the
        // format spec, which silently breaks every column-aligned listing.
        assert_eq!(format!("{:<10}|", NodeType::Fact), "fact      |");
        assert_eq!(format!("{:<12}|", NodeStatus::Valid), "valid       |");
    }
}
