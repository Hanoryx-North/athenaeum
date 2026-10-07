//! The review queue: what in a knowledge base needs a decision (§79).
//!
//! Phase 6's acceptance test is that "contradictions remain visible" and that
//! stale knowledge can be restored rather than silently dropped. Visibility
//! is this module's whole job. It reads; it never rewrites a node's status,
//! never resolves a dispute, and never deletes anything. Deciding *which*
//! rival claim is right is a judgement, and a judgement belongs to the person
//! or to the runtime's verifier — but noticing that a decision is owed is
//! mechanical, and that part belongs here where it can run offline and be
//! tested without a model.
//!
//! Every finding is reproducible from the store alone. Run it twice on an
//! unchanged base and it reports the same things in the same order.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::authority;
use crate::error::Result;
use crate::id::NodeId;
use crate::model::{EdgeType, KnowledgeNode, NodeStatus, NodeType, ProvenanceSource};
use crate::store::{EdgeFilter, KnowledgeStore, NodeFilter};

/// Why a node is in the queue.
///
/// Ordered by how much a reader should care: a disputed claim is being handed
/// to agents right now, whereas an unreachable one is merely hard to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    /// Marked as contradicted and awaiting a decision (§17).
    Contradicted,
    /// Several live claims describe one subject; one governs and the rest are
    /// shadowed (§14).
    Disputed,
    /// Its anchor could not be recovered at all (§16).
    AnchorLost,
    /// Its source moved or changed; the claim may still hold (§17).
    Stale,
    /// A rule or a decision resting on nothing (§15).
    Unsupported,
    /// A relationship a model proposed, not yet accepted (§20).
    ProposedEdge,
    /// Live, but no traversable relationship reaches it. Navigation is
    /// retrieval (§22), so nothing but a literal search will ever find it.
    Unreachable,
    /// Keeps being retrieved and never helps: it matches searches without
    /// answering them (§41).
    Noisy,
    /// No title, or no words at all.
    Empty,
}

impl ReviewKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewKind::Contradicted => "contradicted",
            ReviewKind::Disputed => "disputed",
            ReviewKind::AnchorLost => "anchor_lost",
            ReviewKind::Stale => "stale",
            ReviewKind::Unsupported => "unsupported",
            ReviewKind::ProposedEdge => "proposed_edge",
            ReviewKind::Unreachable => "unreachable",
            ReviewKind::Noisy => "noisy",
            ReviewKind::Empty => "empty",
        }
    }

    /// True when leaving this alone risks handing an agent something wrong,
    /// as opposed to merely untidy. What makes `athenaeum check` exit
    /// non-zero.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            ReviewKind::Contradicted
                | ReviewKind::Disputed
                | ReviewKind::AnchorLost
                | ReviewKind::Empty
        )
    }
}

impl std::fmt::Display for ReviewKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// One thing in the knowledge base that needs a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFinding {
    pub kind: ReviewKind,
    pub node: NodeId,
    pub title: String,
    /// What is wrong, phrased for someone reading a terminal.
    pub problem: String,
    /// The other nodes this finding is about: the claim that governs a
    /// disputed one, the far end of a proposed relationship.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<NodeId>,
}

impl std::fmt::Display for ReviewFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.title.is_empty() {
            write!(f, "{}: {}", self.node, self.problem)
        } else {
            write!(f, "{} ({}): {}", self.node, self.title, self.problem)
        }
    }
}

/// What a sweep found, and over how much.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewReport {
    pub nodes: usize,
    pub findings: Vec<ReviewFinding>,
}

impl ReviewReport {
    pub fn counts(&self) -> BTreeMap<ReviewKind, usize> {
        let mut counts = BTreeMap::new();
        for finding in &self.findings {
            *counts.entry(finding.kind).or_default() += 1;
        }
        counts
    }

    /// True when something here should stop a build rather than just be read.
    pub fn has_blocking(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.kind.is_blocking())
    }

    pub fn of_kind(&self, kind: ReviewKind) -> impl Iterator<Item = &ReviewFinding> {
        self.findings.iter().filter(move |f| f.kind == kind)
    }
}

/// Sweep the whole knowledge base.
///
/// Reads every node once, every edge once, and evidence only for the node
/// types that are held to an evidence bar — so the cost is the size of the
/// base, not the square of it.
pub fn sweep(store: &KnowledgeStore) -> Result<ReviewReport> {
    let nodes = store.list_nodes(&NodeFilter::all())?;
    let edges = store.list_edges(&EdgeFilter::all())?;
    let utilities = store.utilities()?;

    let mut findings = Vec::new();
    let mut reachable = HashSet::new();

    let titles = nodes
        .iter()
        .map(|node| (node.id.clone(), node.title.clone()))
        .collect::<HashMap<_, _>>();
    // Claims still in play: being offered, or withheld pending a decision.
    // A superseded or archived claim is neither, and cannot be a party to an
    // open dispute.
    let in_play = nodes
        .iter()
        .filter(|node| node.is_live() || node.status == NodeStatus::Contradicted)
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();

    // Who each node is recorded as disagreeing with, so a contradiction
    // finding can name the other side instead of leaving the reader to go
    // looking for it.
    let mut disagrees_with: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for edge in &edges {
        if edge.is_traversable() {
            reachable.insert(edge.from.clone());
            reachable.insert(edge.to.clone());
        }
        if edge.edge_type == EdgeType::Contradicts
            && edge.is_traversable()
            && in_play.contains(&edge.from)
            && in_play.contains(&edge.to)
        {
            disagrees_with
                .entry(edge.from.clone())
                .or_default()
                .push(edge.to.clone());
            disagrees_with
                .entry(edge.to.clone())
                .or_default()
                .push(edge.from.clone());
        }
    }

    for node in &nodes {
        if node.title.trim().is_empty() {
            findings.push(finding(ReviewKind::Empty, node, "has no title"));
        } else if node.summary.trim().is_empty() && node.content.trim().is_empty() {
            findings.push(finding(ReviewKind::Empty, node, "has no content at all"));
        }

        // A contradiction is visible either way it was recorded: a node
        // marked contradicted, or an accepted `contradicts` relationship
        // between two claims still being offered. Phase 6 asks that they stay
        // visible, not that they be recorded one particular way.
        let rivals = disagrees_with.get(&node.id);
        if node.status == NodeStatus::Contradicted || (rivals.is_some() && node.is_live()) {
            let named = rivals
                .map(|ids| {
                    ids.iter()
                        .map(|id| {
                            titles
                                .get(id)
                                .map(|title| format!("{title:?}"))
                                .unwrap_or_else(|| id.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            findings.push(ReviewFinding {
                kind: ReviewKind::Contradicted,
                node: node.id.clone(),
                title: node.title.clone(),
                problem: if named.is_empty() {
                    "is contradicted and awaiting a decision".to_string()
                } else {
                    format!("disagrees with {named}, and nobody has decided between them")
                },
                related: rivals.cloned().unwrap_or_default(),
            });
        }

        match node.status {
            NodeStatus::StaleRequiresReview => findings.push(finding(
                ReviewKind::AnchorLost,
                node,
                "no longer resolves to anything in the source it came from",
            )),
            NodeStatus::Stale => findings.push(finding(
                ReviewKind::Stale,
                node,
                "rests on source that has moved or changed since it was written",
            )),
            _ => {}
        }

        // A rule or decision nobody can justify is the kind of knowledge that
        // quietly becomes wrong (§15). An observation is allowed to be just
        // an observation.
        if matches!(node.node_type, NodeType::Rule | NodeType::Decision)
            && store.evidence_for(&node.id)?.is_empty()
        {
            findings.push(finding(
                ReviewKind::Unsupported,
                node,
                &format!("is a {} with no evidence", node.node_type),
            ));
        }

        if node.is_live() && !reachable.contains(&node.id) {
            findings.push(finding(
                ReviewKind::Unreachable,
                node,
                "has no traversable relationship, so navigation can never reach it",
            ));
        }

        // §40's warning, measured rather than guessed at: the wiki should get
        // more useful as it grows, not noisier.
        if let Some(utility) = utilities.get(&node.id) {
            if utility.looks_like_noise() {
                findings.push(finding(
                    ReviewKind::Noisy,
                    node,
                    &format!(
                        "has been retrieved {} times and used none of them",
                        utility.retrievals
                    ),
                ));
            }
        }
    }

    findings.extend(disputes(&nodes));

    for edge in &edges {
        if edge.status != NodeStatus::Proposed {
            continue;
        }
        let Some(title) = titles.get(&edge.from) else {
            continue;
        };
        let target = titles
            .get(&edge.to)
            .cloned()
            .unwrap_or_else(|| edge.to.to_string());
        let by = match edge.provenance.source {
            ProvenanceSource::Model => "a model",
            ProvenanceSource::Import => "an import",
            _ => "something",
        };
        findings.push(ReviewFinding {
            kind: ReviewKind::ProposedEdge,
            node: edge.from.clone(),
            title: title.clone(),
            problem: format!(
                "{by} proposed {} → {target}; it is not traversed until promoted",
                edge.edge_type
            ),
            related: vec![edge.to.clone()],
        });
    }

    // Stable order, so two sweeps of one base read identically and a diff
    // between two sweeps means something changed.
    findings.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.node.cmp(&b.node))
            .then_with(|| a.problem.cmp(&b.problem))
    });

    Ok(ReviewReport {
        nodes: nodes.len(),
        findings,
    })
}

/// Rival claims about one subject.
///
/// Grouping by [`authority::subject_key`] finds a whole cluster in one pass.
/// The claim [`authority::rank`] puts first is the one an agent is actually
/// being handed, so the findings are raised against the others: those are
/// what someone has to merge, supersede or re-scope.
///
/// Only claims take part. Structure cannot contradict structure — the `Error`
/// enum and the `error` module share a name and disagree about nothing.
fn disputes(nodes: &[KnowledgeNode]) -> Vec<ReviewFinding> {
    let mut clusters: BTreeMap<_, Vec<&KnowledgeNode>> = BTreeMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.is_live() && !node.node_type.is_structural())
    {
        clusters
            .entry(authority::subject_key(node))
            .or_default()
            .push(node);
    }

    let mut findings = Vec::new();
    for (_, cluster) in clusters {
        if cluster.len() < 2 {
            continue;
        }
        let owned = cluster
            .iter()
            .map(|node| (*node).clone())
            .collect::<Vec<_>>();
        let ranked = authority::rank(&owned);
        let Some(governing) = ranked.first() else {
            continue;
        };
        for shadowed in ranked.iter().skip(1) {
            findings.push(ReviewFinding {
                kind: ReviewKind::Disputed,
                node: shadowed.id.clone(),
                title: shadowed.title.clone(),
                problem: format!(
                    "describes the same subject as {:?}, which governs instead \
                     ({} at {} scope)",
                    governing.title, governing.authority, governing.scope
                ),
                related: vec![governing.id.clone()],
            });
        }
    }
    findings
}

fn finding(kind: ReviewKind, node: &KnowledgeNode, problem: &str) -> ReviewFinding {
    ReviewFinding {
        kind,
        node: node.id.clone(),
        title: node.title.clone(),
        problem: problem.to_string(),
        related: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{EventKind, NewEvent};
    use crate::model::{
        Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, NodeType, Scope, SourceAnchor,
    };

    fn store() -> KnowledgeStore {
        KnowledgeStore::open_in_memory().unwrap()
    }

    /// A node plus the one relationship that keeps it off the unreachable
    /// list, so a test about something else is not also a test about that.
    fn anchored(store: &mut KnowledgeStore, node: KnowledgeNode) -> NodeId {
        let id = store.create_node(node).unwrap().id;
        let other = store
            .create_node(
                KnowledgeNode::new(NodeType::Concept, format!("context for {id}"))
                    .with_summary("s"),
            )
            .unwrap();
        store
            .connect(KnowledgeEdge::deterministic(
                id.clone(),
                EdgeType::RelatedTo,
                other.id,
                "test fixture",
            ))
            .unwrap();
        id
    }

    fn kinds(report: &ReviewReport) -> Vec<ReviewKind> {
        report.findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn a_healthy_knowledge_base_has_nothing_to_report() {
        let mut store = store();
        let node = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Decision, "Use SQLite").with_summary("Local-first."),
        );
        store
            .attach_evidence(
                Evidence::new(node, EvidenceKind::SourceFile)
                    .with_anchor(SourceAnchor::file("Cargo.toml")),
            )
            .unwrap();

        let report = sweep(&store).unwrap();
        assert_eq!(report.findings, Vec::new());
        assert_eq!(report.nodes, 2);
        assert!(!report.has_blocking());
    }

    #[test]
    fn a_decision_with_no_evidence_is_reported_but_an_observation_is_not() {
        let mut store = store();
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Decision, "Use SQLite").with_summary("Local-first."),
        );
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Observation, "Looks slow").with_summary("s"),
        );

        let report = sweep(&store).unwrap();
        assert_eq!(kinds(&report), vec![ReviewKind::Unsupported]);
        // Untidy, not wrong: this alone should not fail a build.
        assert!(!report.has_blocking());
    }

    #[test]
    fn an_empty_node_is_reported() {
        let mut store = store();
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Concept, "Nothing here"),
        );

        let report = sweep(&store).unwrap();
        assert_eq!(kinds(&report), vec![ReviewKind::Empty]);
        assert!(report.findings[0].problem.contains("no content"));
        assert!(report.has_blocking());
    }

    #[test]
    fn each_lifecycle_state_that_needs_a_decision_says_which_one_it_is() {
        for (status, expected) in [
            (NodeStatus::Contradicted, ReviewKind::Contradicted),
            (NodeStatus::StaleRequiresReview, ReviewKind::AnchorLost),
            (NodeStatus::Stale, ReviewKind::Stale),
        ] {
            let mut store = store();
            let node = anchored(
                &mut store,
                KnowledgeNode::new(NodeType::Fact, "Sessions last 24h").with_summary("s"),
            );
            store.set_node_status(&node, status).unwrap();

            let report = sweep(&store).unwrap();
            assert!(
                kinds(&report).contains(&expected),
                "{status} produced {:?}",
                kinds(&report)
            );
        }
    }

    #[test]
    fn a_recorded_contradiction_names_the_claim_it_disagrees_with() {
        let mut store = store();
        let a = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Fact, "Sessions last 24h").with_summary("s"),
        );
        let b = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Fact, "Sessions last an hour").with_summary("s"),
        );
        store.contradict(&a, &b).unwrap();

        let report = sweep(&store).unwrap();
        let found = report.of_kind(ReviewKind::Contradicted).collect::<Vec<_>>();
        assert_eq!(found.len(), 2, "{:#?}", report.findings);
        // Both ends are marked: which of two disagreeing claims is wrong is
        // exactly the question nobody has answered yet.
        assert!(found
            .iter()
            .any(|f| f.node == a && f.problem.contains("\"Sessions last an hour\"")));
        assert!(found
            .iter()
            .any(|f| f.node == b && f.problem.contains("\"Sessions last 24h\"")));
        assert!(report.has_blocking());
    }

    #[test]
    fn superseding_one_of_two_contradicting_claims_settles_it() {
        let mut store = store();
        let old = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Fact, "Sessions last 24h").with_summary("s"),
        );
        let new = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Fact, "Sessions last an hour").with_summary("s"),
        );
        store.contradict(&old, &new).unwrap();
        store.supersede(&old, &new).unwrap();
        // The survivor is put back into service; the loser stays as history.
        store.set_node_status(&new, NodeStatus::Valid).unwrap();

        let report = sweep(&store).unwrap();
        assert!(
            report.of_kind(ReviewKind::Contradicted).next().is_none(),
            "{:#?}",
            report.findings
        );
        assert_eq!(
            store.get_node(&old).unwrap().status,
            NodeStatus::Superseded,
            "superseded knowledge is kept, not deleted (§17)"
        );
    }

    #[test]
    fn a_claim_cannot_supersede_or_contradict_itself() {
        let mut store = store();
        let node = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Fact, "Only claim").with_summary("s"),
        );
        assert!(store.supersede(&node, &node).is_err());
        assert!(store.contradict(&node, &node).is_err());
        assert_eq!(store.get_node(&node).unwrap().status, NodeStatus::Valid);
    }

    #[test]
    fn rival_claims_are_reported_against_the_one_that_governs() {
        // The plan's own §14 example, as a review finding.
        let mut store = store();
        let governing = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "Database")
                .with_summary("This project uses SQLite.")
                .with_authority(Authority::ProjectRule)
                .with_scope(Scope::Project),
        );
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "database")
                .with_summary("Use PostgreSQL.")
                .with_authority(Authority::UnverifiedObservation)
                .with_scope(Scope::Project),
        );

        let report = sweep(&store).unwrap();
        let disputed = report.of_kind(ReviewKind::Disputed).collect::<Vec<_>>();
        assert_eq!(disputed.len(), 1, "{:#?}", report.findings);
        // The finding is raised against the shadowed claim, because that is
        // the one somebody has to do something about.
        assert_eq!(disputed[0].title, "database");
        assert_eq!(disputed[0].related, vec![governing.clone()]);
        // The sentence names the rival claim; the id is there for machines.
        assert!(disputed[0].problem.contains("\"Database\""));
        assert!(disputed[0].problem.contains("project_rule"));
        assert!(report.has_blocking());
    }

    #[test]
    fn two_declarations_that_merely_share_a_name_are_not_a_dispute() {
        // Regression: the `Error` enum and the `error` module collided once
        // the subject key lowercased them, and the sweep reported a
        // contradiction between two things that disagree about nothing.
        let mut store = store();
        for (kind, title) in [("enum", "Error"), ("module", "error")] {
            anchored(
                &mut store,
                KnowledgeNode::new(NodeType::Symbol, title)
                    .with_summary(format!("Exported {kind}."))
                    .with_scope(Scope::File),
            );
        }

        let report = sweep(&store).unwrap();
        assert!(
            report.of_kind(ReviewKind::Disputed).next().is_none(),
            "{:#?}",
            report.findings
        );
    }

    #[test]
    fn claims_at_different_scopes_are_not_a_dispute() {
        // "React state is immutable" and "this file mutates a cache
        // deliberately" are both true (§14).
        let mut store = store();
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "Immutability")
                .with_summary("Universal.")
                .with_scope(Scope::Technology),
        );
        anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "Immutability")
                .with_summary("Not in this file.")
                .with_scope(Scope::File),
        );

        let report = sweep(&store).unwrap();
        assert!(report.of_kind(ReviewKind::Disputed).next().is_none());
    }

    #[test]
    fn superseded_claims_do_not_dispute_the_claims_that_replaced_them() {
        let mut store = store();
        let old = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "Database").with_summary("Postgres."),
        );
        let new = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Rule, "Database").with_summary("SQLite."),
        );
        store.supersede(&old, &new).unwrap();

        let report = sweep(&store).unwrap();
        assert!(
            report.of_kind(ReviewKind::Disputed).next().is_none(),
            "{:#?}",
            report.findings
        );
    }

    #[test]
    fn a_proposed_relationship_is_listed_until_it_is_promoted() {
        let mut store = store();
        let from = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "AuthService").with_summary("s"))
            .unwrap();
        let to = store
            .create_node(KnowledgeNode::new(NodeType::Concept, "SessionManager").with_summary("s"))
            .unwrap();
        let edge = store
            .connect(
                KnowledgeEdge::new(from.id.clone(), EdgeType::Requires, to.id.clone())
                    .with_note("model guess"),
            )
            .unwrap();

        let report = sweep(&store).unwrap();
        let proposed = report.of_kind(ReviewKind::ProposedEdge).collect::<Vec<_>>();
        assert_eq!(proposed.len(), 1, "{:#?}", report.findings);
        assert_eq!(proposed[0].node, from.id);
        assert_eq!(proposed[0].related, vec![to.id.clone()]);
        assert!(proposed[0].problem.contains("SessionManager"));
        // A proposal is also not a traversable relationship, so both ends are
        // unreachable until it is accepted (§20, §22).
        assert_eq!(report.of_kind(ReviewKind::Unreachable).count(), 2);

        store.promote_edge(&edge.id).unwrap();
        let report = sweep(&store).unwrap();
        assert!(report.of_kind(ReviewKind::ProposedEdge).next().is_none());
        assert!(report.of_kind(ReviewKind::Unreachable).next().is_none());
    }

    #[test]
    fn a_node_nothing_links_to_is_reported_as_unreachable() {
        let mut store = store();
        store
            .create_node(KnowledgeNode::new(NodeType::Concept, "Island").with_summary("s"))
            .unwrap();

        let report = sweep(&store).unwrap();
        assert_eq!(kinds(&report), vec![ReviewKind::Unreachable]);
        assert!(report.findings[0].problem.contains("navigation"));
    }

    #[test]
    fn archived_knowledge_is_inspected_but_not_chased_for_relationships() {
        // `NodeFilter::all()`, not `live()`: a sweep that skipped archived
        // nodes could not report anything about the history it retains. But
        // an archived node is not offered to anyone, so it being unreachable
        // is not a finding.
        let mut store = store();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Rule, "Old rule").with_summary("s"))
            .unwrap();
        store
            .set_node_status(&node.id, NodeStatus::Archived)
            .unwrap();

        let report = sweep(&store).unwrap();
        assert_eq!(kinds(&report), vec![ReviewKind::Unsupported]);
    }

    #[test]
    fn a_node_that_keeps_surfacing_and_never_helps_is_reported() {
        // §41 measures whether a node earns its place; §40 says the wiki
        // should get more useful as it grows, not noisier.
        let mut store = store();
        let node = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Observation, "Matches everything").with_summary("s"),
        );
        let helpful = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Observation, "Actually answers things").with_summary("s"),
        );
        for _ in 0..6 {
            store
                .record(NewEvent::about(EventKind::NodeRetrieved, &node))
                .unwrap();
            store
                .record(NewEvent::about(EventKind::NodeRetrieved, &helpful))
                .unwrap();
            store
                .record(NewEvent::about(EventKind::NodeUsed, &helpful))
                .unwrap();
        }

        let report = sweep(&store).unwrap();
        let noisy = report.of_kind(ReviewKind::Noisy).collect::<Vec<_>>();
        assert_eq!(noisy.len(), 1, "{:#?}", report.findings);
        assert_eq!(noisy[0].node, node);
        assert!(noisy[0].problem.contains("6 times"));
        // Reading the base did not change it: utility is derived, never
        // written onto the node (§41).
        assert_eq!(store.get_node(&node).unwrap().version, 1);
    }

    #[test]
    fn the_whole_base_reading_of_utility_agrees_with_the_per_node_one() {
        let mut store = store();
        let node = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Observation, "Read a few times").with_summary("s"),
        );
        for kind in [
            EventKind::NodeRetrieved,
            EventKind::NodeRetrieved,
            EventKind::NodeUsed,
            EventKind::NodeCited,
        ] {
            store.record(NewEvent::about(kind, &node)).unwrap();
        }

        let bulk = store.utilities().unwrap();
        assert_eq!(
            bulk.get(&node).copied(),
            Some(store.utility(&node).unwrap())
        );
        assert_eq!(bulk[&node].retrievals, 2);
        // A node nobody has read has not failed to be useful, so it is absent
        // rather than present and zero.
        let unread = anchored(
            &mut store,
            KnowledgeNode::new(NodeType::Observation, "Never read").with_summary("s"),
        );
        assert!(!store.utilities().unwrap().contains_key(&unread));
    }

    #[test]
    fn two_sweeps_of_one_base_read_identically() {
        let mut store = store();
        for title in ["b", "a", "c"] {
            store
                .create_node(KnowledgeNode::new(NodeType::Rule, title))
                .unwrap();
        }
        assert_eq!(sweep(&store).unwrap(), sweep(&store).unwrap());
    }
}
