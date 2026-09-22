//! The closed vocabularies of the knowledge model.
//!
//! Every one of these is stored in SQLite as a short lowercase string rather
//! than an integer. Integers would be smaller, but the point of this database
//! is that a user can open it with any SQLite browser and read their own
//! knowledge (buildplan §61, "user-owned data"). A column that says
//! `decision` is worth more than one that says `7`.

use serde::{Deserialize, Serialize};

/// Returned when a string does not name any variant of a vocabulary.
///
/// Carries the vocabulary name as well as the offending value, because
/// "unknown value: `symbol`" is a much harder bug report to act on than
/// "unknown EdgeType: `symbol`".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownValue {
    pub vocabulary: &'static str,
    pub value: String,
}

impl std::fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown {}: {:?}", self.vocabulary, self.value)
    }
}

impl std::error::Error for UnknownValue {}

/// Generates `as_str` / `from_str` / `Display` for a fieldless enum, keeping
/// the on-disk spelling in exactly one place per variant.
macro_rules! str_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $(#[$vmeta:meta])* $variant:ident => $repr:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            pub fn as_str(&self) -> &'static str {
                match self { $( $name::$variant => $repr ),+ }
            }

            /// Parse without an error value, for callers that only care
            /// whether the string was valid.
            pub fn parse_opt(s: &str) -> Option<$name> {
                s.parse().ok()
            }
        }

        impl std::str::FromStr for $name {
            type Err = UnknownValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $( $repr => Ok($name::$variant), )+
                    _ => Err(UnknownValue {
                        vocabulary: stringify!($name),
                        value: s.to_string(),
                    }),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                // `pad`, not `write_str`: the latter silently ignores width
                // and alignment, so `{:<10}` on one of these would produce
                // ragged columns in every table that prints them.
                f.pad(self.as_str())
            }
        }
    };
}

str_enum! {
    /// What kind of thing a node *is* (buildplan §12).
    ///
    /// These are semantic objects, never text chunks: a node is "AuthService",
    /// not "bytes 4000-4500 of AuthService.ts" (§9).
    pub enum NodeType {
        Project => "project",
        Repository => "repository",
        Module => "module",
        File => "file",
        Symbol => "symbol",
        Concept => "concept",
        /// Established as true by evidence.
        Fact => "fact",
        /// An instruction that should govern behavior.
        Rule => "rule",
        /// A deliberate architectural or product decision.
        Decision => "decision",
        /// A repeatable way of doing something.
        Procedure => "procedure",
        /// A known failure mode or trap.
        Gotcha => "gotcha",
        /// Discovered but not yet sufficiently established.
        /// Default: anything a model merely asserted starts here (§13).
        #[default]
        Observation => "observation",
        Preference => "preference",
        Entity => "entity",
        Api => "api",
        Test => "test",
        Source => "source",
        Dependency => "dependency",
    }
}

impl NodeType {
    /// True when the node describes something that *exists* and is identified
    /// by where it is — a file, a declaration, a commit — rather than
    /// something somebody *asserted* about the project.
    ///
    /// The distinction matters wherever two nodes describing one subject would
    /// be a conflict. Two rules titled "Database" are rival claims and one of
    /// them governs; the `Error` enum and the `error` module are simply two
    /// different things whose names happen to collide. Structure cannot
    /// contradict structure, because it was read rather than judged.
    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            NodeType::Project
                | NodeType::Repository
                | NodeType::Module
                | NodeType::File
                | NodeType::Symbol
                | NodeType::Test
                | NodeType::Source
                | NodeType::Dependency
        )
    }
}

str_enum! {
    /// How two nodes relate (buildplan §8).
    pub enum EdgeType {
        DependsOn => "depends_on",
        Implements => "implements",
        ImplementedBy => "implemented_by",
        Contains => "contains",
        PartOf => "part_of",
        /// Default: the weakest useful claim, so the safest fallback.
        #[default]
        RelatedTo => "related_to",
        Requires => "requires",
        Verifies => "verifies",
        VerifiedBy => "verified_by",
        DerivedFrom => "derived_from",
        Contradicts => "contradicts",
        Supersedes => "supersedes",
        SupersededBy => "superseded_by",
        CausedBy => "caused_by",
        Tests => "tests",
        TestedBy => "tested_by",
        References => "references",
        ConstrainedBy => "constrained_by",
        Constrains => "constrains",
        HasGotcha => "has_gotcha",
        GotchaOf => "gotcha_of",
        EvidencedBy => "evidenced_by",
        ModifiedBy => "modified_by",
        /// A declaration invokes another declaration (§30).
        Calls => "calls",
        CalledBy => "called_by",
    }
}

impl EdgeType {
    /// The edge you would traverse coming back the other way.
    ///
    /// Buildplan §35 requires that a user who reached `OAuthCallbackTest` via
    /// `verified_by` can walk back out along `verifies`. Rather than writing
    /// two rows per relationship and risking them drifting out of sync, one
    /// row is stored and reverse traversal is resolved through this table.
    /// Symmetric relations invert to themselves.
    pub fn inverse(&self) -> EdgeType {
        use EdgeType::*;
        match self {
            DependsOn => Requires,
            Requires => DependsOn,
            Implements => ImplementedBy,
            ImplementedBy => Implements,
            Contains => PartOf,
            PartOf => Contains,
            Verifies => VerifiedBy,
            VerifiedBy => Verifies,
            Tests => TestedBy,
            TestedBy => Tests,
            Supersedes => SupersededBy,
            SupersededBy => Supersedes,
            ConstrainedBy => Constrains,
            Constrains => ConstrainedBy,
            HasGotcha => GotchaOf,
            GotchaOf => HasGotcha,
            Calls => CalledBy,
            CalledBy => Calls,
            // Symmetric, or with no meaningful named inverse.
            RelatedTo => RelatedTo,
            Contradicts => Contradicts,
            DerivedFrom => References,
            References => DerivedFrom,
            CausedBy => CausedBy,
            EvidencedBy => EvidencedBy,
            ModifiedBy => ModifiedBy,
        }
    }
}

str_enum! {
    /// Where a claim sits in the authority hierarchy (buildplan §14).
    ///
    /// Ordering matters here and is expressed by [`Authority::rank`], not by
    /// declaration order, so variants can be added later without silently
    /// reordering precedence.
    pub enum Authority {
        UserInstruction => "user_instruction",
        ProjectRule => "project_rule",
        ProjectDecision => "project_decision",
        VerifiedEvidence => "verified_evidence",
        VerifiedKnowledge => "verified_knowledge",
        UserPreference => "user_preference",
        ImportedKnowledge => "imported_knowledge",
        ExternalKnowledge => "external_knowledge",
        /// Default: generated text is never automatically authoritative (§13).
        #[default]
        UnverifiedObservation => "unverified_observation",
    }
}

/// What a claim must be able to show before it may be raised to a given
/// authority (§13, §47).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceRequirement {
    /// Nothing. The claim is offered for what it is.
    None,
    /// Something was actually checked, so anything except a source we did not
    /// check ourselves.
    Verified,
    /// The project decided it. §47's list: configuration, project
    /// documentation such as `AGENTS.md`, a recorded architectural decision,
    /// or the user saying so.
    Authoritative,
    /// Not grantable by verification at all.
    Never,
}

impl Authority {
    /// What evidence a claim must carry before it may be *raised* to this
    /// level.
    ///
    /// §47's worked example is a model proposing "never modify generated
    /// files" as a RULE. The system's answer is to ask why, and to promote it
    /// only on authoritative evidence. Without this, a model could assert a
    /// rule and then verify its own assertion into one.
    pub fn evidence_required(&self) -> EvidenceRequirement {
        use Authority::*;
        match self {
            // Only the user states a user instruction. That is a fact about
            // who said it, not a conclusion anything can check its way into.
            UserInstruction => EvidenceRequirement::Never,
            ProjectRule | ProjectDecision => EvidenceRequirement::Authoritative,
            VerifiedEvidence | VerifiedKnowledge => EvidenceRequirement::Verified,
            UserPreference | ImportedKnowledge | ExternalKnowledge | UnverifiedObservation => {
                EvidenceRequirement::None
            }
        }
    }

    /// Lower rank wins. `0` is the most authoritative.
    pub fn rank(&self) -> u8 {
        use Authority::*;
        match self {
            UserInstruction => 0,
            ProjectRule => 1,
            ProjectDecision => 2,
            VerifiedEvidence => 3,
            VerifiedKnowledge => 4,
            UserPreference => 5,
            ImportedKnowledge => 6,
            ExternalKnowledge => 7,
            UnverifiedObservation => 8,
        }
    }

    /// True when `self` outranks `other`.
    pub fn outranks(&self, other: &Authority) -> bool {
        self.rank() < other.rank()
    }
}

str_enum! {
    /// Lifecycle of a node (buildplan §17).
    ///
    /// Deliberately separate from confidence: a node can be both
    /// high-confidence and stale.
    pub enum NodeStatus {
        /// Proposed by a model, not yet accepted (§20).
        Proposed => "proposed",
        /// Default: knowledge is usable until something says otherwise.
        #[default]
        Valid => "valid",
        /// Its source moved or changed; the claim may still hold.
        Stale => "stale",
        /// Anchor recovery failed outright; a human should look (§16).
        StaleRequiresReview => "stale_requires_review",
        Superseded => "superseded",
        Contradicted => "contradicted",
        Archived => "archived",
    }
}

str_enum! {
    /// How far a piece of knowledge travels (buildplan §66).
    ///
    /// This is what makes knowledge packs possible later: `Universal` and
    /// `Technology` nodes are portable, `Project` and below are not.
    pub enum Scope {
        Universal => "universal",
        Technology => "technology",
        Organization => "organization",
        /// Default: most knowledge is about the project in front of you.
        #[default]
        Project => "project",
        Repository => "repository",
        File => "file",
    }
}

impl Scope {
    /// True when knowledge at this scope may be shared outside the machine
    /// it was created on without further review.
    pub fn is_portable(&self) -> bool {
        matches!(self, Scope::Universal | Scope::Technology)
    }

    /// How narrowly this knowledge applies. Higher is narrower.
    ///
    /// Used to break authority ties: "this project uses SQLite" beats the
    /// global "use PostgreSQL" not because it is more authoritative in kind,
    /// but because it is about the thing actually in front of us (§14).
    pub fn specificity(&self) -> u8 {
        match self {
            Scope::Universal => 0,
            Scope::Technology => 1,
            Scope::Organization => 2,
            Scope::Project => 3,
            Scope::Repository => 4,
            Scope::File => 5,
        }
    }
}

str_enum! {
    /// Who or what produced a claim (buildplan §20).
    ///
    /// A relationship derived from an `import` statement and one guessed by a
    /// model are both edges, but they are not the same kind of thing, and the
    /// graph must never forget which is which.
    pub enum ProvenanceSource {
        /// Stated directly by the user.
        User => "user",
        /// Extracted mechanically (imports, AST, git). No model involved.
        Deterministic => "deterministic",
        /// Proposed by a model.
        /// Default: assume a model said it unless told otherwise.
        #[default]
        Model => "model",
        /// Arrived in a knowledge pack.
        Import => "import",
    }
}

impl ProvenanceSource {
    /// Deterministic and user-stated claims can be trusted as facts;
    /// model-proposed ones must stay marked as proposals (§20).
    pub fn is_authoritative(&self) -> bool {
        matches!(
            self,
            ProvenanceSource::User | ProvenanceSource::Deterministic
        )
    }
}

str_enum! {
    /// Kinds of evidence a claim can rest on (buildplan §15).
    pub enum EvidenceKind {
        /// Default: the commonest kind of justification.
        #[default]
        SourceFile => "source_file",
        Symbol => "symbol",
        Test => "test",
        GitCommit => "git_commit",
        Documentation => "documentation",
        Configuration => "configuration",
        UserStatement => "user_statement",
        ArchitecturalDecision => "architectural_decision",
        ExternalSource => "external_source",
    }
}

impl EvidenceKind {
    /// Whether resting on this kind of evidence satisfies `requirement`.
    pub fn establishes(&self, requirement: EvidenceRequirement) -> bool {
        use EvidenceKind::*;
        match requirement {
            EvidenceRequirement::None => true,
            // Someone else's blog post is not something we checked.
            EvidenceRequirement::Verified => !matches!(self, ExternalSource),
            EvidenceRequirement::Authoritative => matches!(
                self,
                UserStatement | Configuration | Documentation | ArchitecturalDecision
            ),
            EvidenceRequirement::Never => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
