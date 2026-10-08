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
