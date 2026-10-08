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
