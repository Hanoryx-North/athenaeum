//! Deciding which of several conflicting claims applies (buildplan §14).
//!
//! The worked example from the plan:
//!
//! ```text
//! Global:       Use PostgreSQL.
//! Project:      This project uses SQLite.
//! Current task: This migration specifically requires PostgreSQL.
//! ```
//!
//! All three are true. The system's job is not to delete two of them, it is
//! to know which one governs right now.

use crate::model::{Authority, KnowledgeNode, NodeStatus};

/// Pick the claim that governs, from a set of candidates about one subject.
///
/// Ordering, in strict precedence:
///
/// 1. **Authority rank** — a user instruction beats a project rule beats an
///    unverified observation.
/// 2. **Scope specificity** — at equal authority, the claim about the
///    narrower thing wins. This is what makes the SQLite/PostgreSQL example
///    resolve correctly.
/// 3. **Recency** — at equal authority and scope, the more recently updated
///    claim wins.
///
/// Contradicted and superseded candidates are excluded outright: they are
/// retained as history, not offered as current answers.
pub fn resolve(candidates: &[KnowledgeNode]) -> Option<&KnowledgeNode> {
    candidates
        .iter()
        .filter(|node| node.is_live())
        .min_by(|a, b| {
            a.authority
                .rank()
                .cmp(&b.authority.rank())
                .then_with(|| b.scope.specificity().cmp(&a.scope.specificity()))
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        })
}

/// Candidates ordered exactly as [`resolve`] ranks them, most authoritative
/// first — for showing a user *why* one claim won.
pub fn rank(candidates: &[KnowledgeNode]) -> Vec<&KnowledgeNode> {
    let mut ordered: Vec<&KnowledgeNode> = candidates.iter().filter(|n| n.is_live()).collect();
    ordered.sort_by(|a, b| {
        a.authority
            .rank()
            .cmp(&b.authority.rank())
            .then_with(|| b.scope.specificity().cmp(&a.scope.specificity()))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    ordered
}

/// What two claims must agree on to be talking about the same thing.
///
/// Deliberately narrow: claims at different scopes are usually both correct
/// in their own domain ("React state is immutable" and "this file mutates a
/// cache deliberately" do not contradict each other), so only same-scope
/// disagreement is treated as a conflict worth surfacing.
///
/// It is ordered, and the vocabularies it is built from are not, on purpose:
/// sorting `Authority` by declaration order would quietly disagree with
/// [`Authority::rank`], so the enums stay unordered and this carries their
/// written spellings instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subject {
    pub scope: &'static str,
    pub node_type: &'static str,
    /// The title, trimmed and lowercased.
    pub title: String,
}

/// The subject a claim is about.
///
/// Grouping by this is how a review sweep finds a whole cluster of rival
/// claims in one pass instead of comparing every node with every other.
pub fn subject_key(node: &KnowledgeNode) -> Subject {
    Subject {
        scope: node.scope.as_str(),
        node_type: node.node_type.as_str(),
        title: node.title.trim().to_lowercase(),
    }
}

/// True when two claims are in genuine conflict rather than simply differing.
pub fn conflicts(a: &KnowledgeNode, b: &KnowledgeNode) -> bool {
    if a.id == b.id {
        return false;
    }
    if a.status == NodeStatus::Contradicted || b.status == NodeStatus::Contradicted {
        return true;
    }
    subject_key(a) == subject_key(b)
}

/// Whether a claim is solid enough to state without hedging.
///
/// Anything a model merely proposed is not, no matter how confident the
/// wording (§13).
pub fn is_trustworthy(node: &KnowledgeNode) -> bool {
    is_trustworthy_parts(node.status, node.authority)
}

/// [`is_trustworthy`] for callers holding the two fields rather than the
/// node — a brief carries a summary of a node, not the node itself, and two
/// copies of this rule would eventually disagree.
pub fn is_trustworthy_parts(status: NodeStatus, authority: Authority) -> bool {
    status == NodeStatus::Valid && authority.rank() <= Authority::VerifiedKnowledge.rank()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeType, Scope};

    fn node(title: &str, authority: Authority, scope: Scope) -> KnowledgeNode {
        KnowledgeNode::new(NodeType::Rule, title)
            .with_authority(authority)
            .with_scope(scope)
    }

    #[test]
    fn higher_authority_wins_regardless_of_scope() {
        let candidates = vec![
            node("Use SQLite", Authority::UnverifiedObservation, Scope::File),
            node(
                "Use PostgreSQL",
                Authority::UserInstruction,
                Scope::Universal,
            ),
        ];
        assert_eq!(resolve(&candidates).unwrap().title, "Use PostgreSQL");
    }

    #[test]
    fn at_equal_authority_the_narrower_scope_wins() {
        // The plan's own example: a global preference loses to the project
        // rule that contradicts it.
        let candidates = vec![
            node("Use PostgreSQL", Authority::ProjectRule, Scope::Universal),
            node("Use SQLite", Authority::ProjectRule, Scope::Project),
        ];
        assert_eq!(resolve(&candidates).unwrap().title, "Use SQLite");
    }

    #[test]
    fn superseded_claims_are_never_returned() {
        let mut stale = node("Old rule", Authority::UserInstruction, Scope::Project);
        stale.status = NodeStatus::Superseded;
        let current = node("New rule", Authority::UnverifiedObservation, Scope::Project);
        let candidates = vec![stale, current];
        assert_eq!(resolve(&candidates).unwrap().title, "New rule");
    }

    #[test]
    fn nothing_live_resolves_to_nothing() {
        let mut archived = node("Gone", Authority::UserInstruction, Scope::Project);
        archived.status = NodeStatus::Archived;
        assert!(resolve(&[archived]).is_none());
        assert!(resolve(&[]).is_none());
    }

    #[test]
    fn ranking_is_consistent_with_resolution() {
        let candidates = vec![
            node("c", Authority::ExternalKnowledge, Scope::Project),
            node("a", Authority::UserInstruction, Scope::Project),
            node("b", Authority::ProjectRule, Scope::Project),
        ];
        let ranked = rank(&candidates);
        assert_eq!(ranked[0].title, "a");
        assert_eq!(ranked.last().unwrap().title, "c");
        assert_eq!(resolve(&candidates).unwrap().id, ranked[0].id);
    }

    #[test]
    fn different_scopes_are_not_treated_as_conflicting() {
        let global = node("Immutability", Authority::ProjectRule, Scope::Technology);
        let local = node("Immutability", Authority::ProjectRule, Scope::File);
        assert!(!conflicts(&global, &local));

        let same_scope_a = node("Immutability", Authority::ProjectRule, Scope::Project);
        let same_scope_b = node("immutability", Authority::ProjectRule, Scope::Project);
        assert!(conflicts(&same_scope_a, &same_scope_b));
    }

    #[test]
    fn model_proposals_are_not_trustworthy_on_their_own() {
        let guessed = node(
            "Probably fine",
            Authority::UnverifiedObservation,
            Scope::Project,
        );
        assert!(!is_trustworthy(&guessed));

        let verified = node("Established", Authority::VerifiedEvidence, Scope::Project);
        assert!(is_trustworthy(&verified));
    }
}
