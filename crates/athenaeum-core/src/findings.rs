//! What needs a decision, in the vocabulary the bridge publishes.
//!
//! This is the review queue a person actually reads: contradictions between
//! live claims, duplicates and near-duplicates that should be one page,
//! supersession that nobody has confirmed, anchors that no longer resolve,
//! rules resting on nothing, proposals nobody has accepted, knowledge nothing
//! can reach, and scans that stopped early.
//!
//! Three rules hold throughout, and they are the reason this is its own module
//! rather than a rename of [`crate::review`]:
//!
//! * **Nothing is deleted, or even modified.** Every function here reads.
//!   A finding is a statement that a decision is owed, never a decision.
//! * **Every finding is reproducible.** Same database, same findings, in the
//!   same order, with the same ids. Detection that is not deterministic cannot
//!   be reviewed, because the reviewer cannot tell a change in the knowledge
//!   from a change in the detector.
//! * **Every detection explains itself.** A near-duplicate carries the score
//!   and the features that matched; a detected contradiction carries the terms
//!   that oppose each other. "These look similar" is not reviewable.
//!
//! The wire vocabulary is fixed by the bridge contract, so the spellings here
//! (`stale_anchor`, `unreviewed_proposal`, and the rest) are part of the
//! protocol and must not be renamed to match an internal one.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::{EvidenceId, NodeId};
use crate::model::{hash, EdgeType, NodeStatus, NodeType, ProvenanceSource};
use crate::portable::memory;
use crate::status::scan_state;
use crate::store::{EdgeFilter, KnowledgeStore, NodeFilter, SourceScope};

/// Which question a finding is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// Two live claims that cannot both be true.
    Contradiction,
    /// Several live claims describe one subject; one governs and the rest are
    /// shadowed.
    Dispute,
    /// Its anchor no longer resolves to the source it was taken from.
    StaleAnchor,
    /// A rule or a decision resting on nothing.
    UnsupportedRule,
    /// A model proposed something and nobody has decided.
    UnreviewedProposal,
    /// Two live nodes are the same knowledge.
    Duplicate,
    /// Two live nodes are nearly the same knowledge, with the score to show it.
    NearDuplicate,
    /// Knowledge that has been superseded, or that supersedes without saying so.
    Superseded,
    /// A scan stopped before it reached the end of the tree.
    IncompleteScan,
    /// Live knowledge no relationship can reach.
    Orphan,
}

impl FindingKind {
    /// Every kind, in the order the bridge publishes them.
    pub const ALL: &'static [FindingKind] = &[
        FindingKind::Contradiction,
        FindingKind::Dispute,
        FindingKind::StaleAnchor,
        FindingKind::UnsupportedRule,
        FindingKind::UnreviewedProposal,
        FindingKind::Duplicate,
        FindingKind::NearDuplicate,
        FindingKind::Superseded,
        FindingKind::IncompleteScan,
        FindingKind::Orphan,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            FindingKind::Contradiction => "contradiction",
            FindingKind::Dispute => "dispute",
            FindingKind::StaleAnchor => "stale_anchor",
            FindingKind::UnsupportedRule => "unsupported_rule",
            FindingKind::UnreviewedProposal => "unreviewed_proposal",
            FindingKind::Duplicate => "duplicate",
            FindingKind::NearDuplicate => "near_duplicate",
            FindingKind::Superseded => "superseded",
            FindingKind::IncompleteScan => "incomplete_scan",
            FindingKind::Orphan => "orphan",
        }
    }
}

impl std::fmt::Display for FindingKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// How much a reader should care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    /// Handing this to an agent risks it acting on something wrong.
    Blocking,
    /// Not dangerous, but owed a decision.
    Warning,
    /// Worth knowing; nothing to do.
    Info,
}

impl FindingSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            FindingSeverity::Blocking => "blocking",
            FindingSeverity::Warning => "warning",
            FindingSeverity::Info => "info",
        }
    }
}

impl std::fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// A finding, with the detail the engine used to reach it.
///
/// [`ReviewFindingView`] is what crosses the bridge; this carries the matched
/// features as well, which is what makes a detection explainable to a person
/// reading a terminal rather than a JSON payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub kind: FindingKind,
    pub severity: FindingSeverity,
    pub title: String,
    pub detail: String,
    pub nodes: Vec<NodeId>,
    pub evidence: Vec<EvidenceId>,
    pub source_path: Option<String>,
    pub confidence: f32,
    /// The features a similarity or conflict test matched, when it matched any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl Finding {
    /// True when leaving this alone risks something wrong reaching an agent.
    pub fn is_blocking(&self) -> bool {
        self.severity == FindingSeverity::Blocking
    }
}

/// The same finding, in exactly the shape the bridge publishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFindingView {
    pub id: String,
    pub kind: FindingKind,
    pub severity: FindingSeverity,
    pub title: String,
    pub detail: String,
    pub nodes: Vec<NodeId>,
    pub evidence: Vec<EvidenceId>,
    pub source_path: Option<String>,
    /// The same judgement as [`Finding::confidence`], rounded for the wire.
    pub confidence: f64,
    pub blocking: bool,
}

impl From<Finding> for ReviewFindingView {
    fn from(finding: Finding) -> Self {
        Self {
            blocking: finding.is_blocking(),
            id: finding.id,
            kind: finding.kind,
            severity: finding.severity,
            title: finding.title,
            detail: finding.detail,
            nodes: finding.nodes,
            evidence: finding.evidence,
            source_path: finding.source_path,
            confidence: crate::model::wire_confidence(finding.confidence),
        }
    }
}

/// Budgets and thresholds for a sweep.
///
/// Every one of these exists because the alternative is a sweep whose cost is
/// the square of the knowledge base. They are options rather than constants so
/// that a status line and a review command can ask for different amounts of
/// work, and so that a caller can see exactly what was assumed.
#[derive(Debug, Clone)]
pub struct FindingOptions {
    /// Hard ceiling on returned findings. A review command wants the whole
    /// list; a status line wants a number and no cost.
    pub max_findings: usize,
    /// How similar two claims must be before they are called near-duplicates.
    pub near_duplicate_threshold: f32,
    /// Shortest token set worth comparing. Below this, similarity is noise.
    pub min_tokens: usize,
    /// How many candidate neighbours one node is compared against.
    pub max_candidates: usize,
    /// Whether to read anchored files to check that their hashes still match.
    /// Off for a status line: it is the only part of a sweep that touches disk.
    pub verify_anchors: bool,
    /// How many distinct files anchor verification will read.
    pub max_anchor_files: usize,
    /// Total bytes anchor verification will read.
    pub max_anchor_bytes: u64,
}

impl Default for FindingOptions {
    fn default() -> Self {
        Self {
            max_findings: 1_000,
            near_duplicate_threshold: 0.55,
            min_tokens: 3,
            max_candidates: 32,
            verify_anchors: true,
            max_anchor_files: 512,
            max_anchor_bytes: 16 * 1024 * 1024,
        }
    }
}

impl FindingOptions {
    /// The cheap sweep a status line uses: no disk, fewer findings.
    pub fn status() -> Self {
        Self {
            max_findings: 500,
            verify_anchors: false,
            ..Self::default()
        }
    }
}

/// Sweep the knowledge base and report everything that needs a decision.
pub fn detect(store: &KnowledgeStore, options: &FindingOptions) -> Result<Vec<Finding>> {
    let nodes = store.list_nodes(&NodeFilter::all())?;
    let edges = store.list_edges(&EdgeFilter::all())?;
    let titles = nodes
        .iter()
        .map(|node| (node.id.clone(), node.title.clone()))
        .collect::<HashMap<_, _>>();
    let live = nodes
        .iter()
        .filter(|node| node.is_live())
        .cloned()
        .collect::<Vec<_>>();

    let mut findings = Vec::new();
    findings.extend(contradictions(
        store, &nodes, &live, &edges, &titles, options,
    )?);
    findings.extend(duplicates(store, &live, options)?);
    findings.extend(supersession(store, &nodes, &edges, &titles)?);
    findings.extend(anchors(store, &nodes, options)?);
    findings.extend(unsupported(store, &nodes)?);
    findings.extend(proposals(store, &nodes, &edges, &titles, options)?);
    findings.extend(incomplete_scans(store)?);
    findings.extend(orphans(store, &nodes, &edges, options)?);

    findings.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    findings.dedup_by(|a, b| a.id == b.id);
    findings.truncate(options.max_findings);
    Ok(findings)
}

/// How many findings a sweep would report, without building the whole list.
pub fn count(store: &KnowledgeStore, options: &FindingOptions) -> Result<u64> {
    Ok(detect(store, options)?.len() as u64)
}

/// The review queue, in the bridge's shape.
pub fn review_findings(
    store: &KnowledgeStore,
    limit: Option<u32>,
) -> Result<Vec<ReviewFindingView>> {
    let mut options = FindingOptions::default();
    if let Some(limit) = limit {
        options.max_findings = limit as usize;
    }
    Ok(detect(store, &options)?
        .into_iter()
        .map(ReviewFindingView::from)
        .collect())
}

// ---------------------------------------------------------------------------
// Contradictions and disputes
// ---------------------------------------------------------------------------

/// Terms whose presence on one side and absence on the other is a disagreement
/// the engine can point at without guessing at meaning.
const OPPOSED_TERMS: &[(&str, &str)] = &[
    ("always", "never"),
    ("enable", "disable"),
    ("enabled", "disabled"),
    ("allow", "deny"),
    ("allow", "forbid"),
    ("must", "must not"),
    ("should", "should not"),
    ("include", "exclude"),
    ("required", "optional"),
    ("synchronous", "asynchronous"),
    ("encrypt", "plaintext"),
    ("safe", "unsafe"),
    ("before", "after"),
    ("increase", "decrease"),
    ("public", "private"),
    ("accept", "reject"),
    ("allow", "block"),
];

/// Negations: a claim that shares a proposition with another but denies it.
const NEGATIONS: &[&str] = &[
    "not",
    "never",
    "no",
    "cannot",
    "mustnt",
    "shouldnt",
    "dont",
    "doesnt",
    "without",
    "avoid",
    "forbid",
    "forbidden",
    "disable",
    "disabled",
    "prohibit",
    "deprecated",
];

fn contradictions(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
    live: &[crate::model::KnowledgeNode],
    edges: &[crate::model::KnowledgeEdge],
    titles: &HashMap<NodeId, String>,
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let in_play = nodes
        .iter()
        .filter(|node| node.is_live() || node.status == NodeStatus::Contradicted)
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();

    // Recorded contradictions first: a `contradicts` edge, or a node somebody
    // marked contradicted. These are facts, so they carry full confidence.
    let mut recorded: HashSet<(NodeId, NodeId)> = HashSet::new();
    for edge in edges {
        if edge.edge_type != EdgeType::Contradicts
            || !edge.is_traversable()
            || !in_play.contains(&edge.from)
            || !in_play.contains(&edge.to)
        {
            continue;
        }
        let mut pair = [edge.from.clone(), edge.to.clone()];
        pair.sort();
        recorded.insert((pair[0].clone(), pair[1].clone()));
        let evidence = evidence_of(store, &edge.from, &edge.to)?;
        findings.push(finding(
            FindingKind::Contradiction,
            FindingSeverity::Blocking,
            title_of(titles, &edge.from),
            format!(
                "disagrees with {:?}, and nobody has decided between them",
                title_of(titles, &edge.to)
            ),
            vec![edge.from.clone(), edge.to.clone()],
            evidence,
            None,
            1.0,
            vec![format!("recorded {} edge", EdgeType::Contradicts)],
        ));
    }

    for node in nodes {
        if node.status != NodeStatus::Contradicted {
            continue;
        }
        let rivals = edges
            .iter()
            .filter(|edge| {
                edge.edge_type == EdgeType::Contradicts
                    && edge.is_traversable()
                    && (edge.from == node.id || edge.to == node.id)
            })
            .map(|edge| {
                if edge.from == node.id {
                    edge.to.clone()
                } else {
                    edge.from.clone()
                }
            })
            .collect::<Vec<_>>();
        let already = rivals.iter().any(|rival| {
            let mut pair = [node.id.clone(), rival.clone()];
            pair.sort();
            recorded.contains(&(pair[0].clone(), pair[1].clone()))
        });
        if already {
            continue;
        }
        findings.push(finding(
            FindingKind::Contradiction,
            FindingSeverity::Blocking,
            node.title.clone(),
            "is marked contradicted and awaiting a decision".to_string(),
            std::iter::once(node.id.clone()).chain(rivals).collect(),
            store
                .evidence_for(&node.id)?
                .iter()
                .map(|e| e.id.clone())
                .collect(),
            None,
            1.0,
            vec!["node status contradicted".to_string()],
        ));
    }

    // Rival claims about one subject: several live claims, one governing.
    let mut clusters: BTreeMap<_, Vec<&crate::model::KnowledgeNode>> = BTreeMap::new();
    for node in live.iter().filter(|node| !node.node_type.is_structural()) {
        clusters
            .entry(crate::authority::subject_key(node))
            .or_default()
            .push(node);
    }
    for (_, cluster) in clusters {
        if cluster.len() < 2 {
            continue;
        }
        let owned = cluster
            .iter()
            .map(|node| (*node).clone())
            .collect::<Vec<_>>();
        let ranked = crate::authority::rank(&owned);
        let Some(governing) = ranked.first() else {
            continue;
        };
        for shadowed in ranked.iter().skip(1) {
            // Two claims that agree are not a dispute; only a detectable
            // disagreement is escalated to a contradiction, and it is named.
            if let Some((score, features)) = disagreement(governing, shadowed) {
                findings.push(finding(
                    FindingKind::Contradiction,
                    FindingSeverity::Blocking,
                    shadowed.title.clone(),
                    format!(
                        "asserts the opposite of {:?} about the same subject; {}",
                        governing.title,
                        features.join("; ")
                    ),
                    vec![shadowed.id.clone(), governing.id.clone()],
                    evidence_of(store, &shadowed.id, &governing.id)?,
                    None,
                    score,
                    features,
                ));
                continue;
            }
            findings.push(finding(
                FindingKind::Dispute,
                FindingSeverity::Blocking,
                shadowed.title.clone(),
                format!(
                    "describes the same subject as {:?}, which governs instead ({} at {} scope)",
                    governing.title, governing.authority, governing.scope
                ),
                vec![shadowed.id.clone(), governing.id.clone()],
                evidence_of(store, &shadowed.id, &governing.id)?,
                None,
                1.0,
                vec![format!(
                    "same subject key: {}",
                    governing.title.trim().to_lowercase()
                )],
            ));
        }
    }

    let _ = options;
    Ok(findings)
}

/// The score and the matched features when two claims about one subject
/// disagree, or `None` when they are merely different.
///
/// Two deterministic tests, both of which name what they found:
///
/// * **Opposed terms** — one claim says `enable` where the other says `disable`.
///   Stronger, because the pair was declared rather than inferred.
/// * **Negation asymmetry** — the claims share most of their words and exactly
///   one of them denies the shared proposition.
fn disagreement(
    a: &crate::model::KnowledgeNode,
    b: &crate::model::KnowledgeNode,
) -> Option<(f32, Vec<String>)> {
    let text_a = proposition(a);
    let text_b = proposition(b);
    let tokens_a = tokens(&text_a);
    let tokens_b = tokens(&text_b);

    let mut features = Vec::new();
    for (left, right) in OPPOSED_TERMS {
        let a_has_left = tokens_a.contains(*left);
        let a_has_right = tokens_a.contains(*right);
        let b_has_left = tokens_b.contains(*left);
        let b_has_right = tokens_b.contains(*right);
        if (a_has_left && b_has_right) || (a_has_right && b_has_left) {
            features.push(format!("opposed terms: {left:?} vs {right:?}"));
        }
    }

    let similarity = jaccard(&tokens_a, &tokens_b);
    if !features.is_empty() {
        // Two claims that say "enable X" and "disable X" are in conflict even
        // though they share almost every word; the score leans on the shared
        // subject rather than pretending the sentences are similar.
        return Some((0.7 + 0.2 * similarity, features));
    }

    let negations_a = tokens_a
        .iter()
        .filter(|token| NEGATIONS.contains(&token.as_str()))
        .count();
    let negations_b = tokens_b
        .iter()
        .filter(|token| NEGATIONS.contains(&token.as_str()))
        .count();
    if similarity >= 0.5 && (negations_a == 0) != (negations_b == 0) {
        features.push(format!(
            "same proposition ({:.2} of the same words), denied on one side only",
            similarity
        ));
        return Some((0.6 + 0.2 * similarity, features));
    }

    None
}

/// The title plus summary plus content, bounded: similarity is about what a
/// claim says, and a page's whole text is more than that.
fn proposition(node: &crate::model::KnowledgeNode) -> String {
    let mut text = String::with_capacity(node.title.len() + node.summary.len() + 512);
    text.push_str(&node.title);
    text.push(' ');
    text.push_str(&node.summary);
    text.push(' ');
    text.extend(node.content.chars().take(2_000));
    text
}

/// Lowercased alphanumeric words, which is the feature set everything here
/// compares. Deterministic by construction: no stemming, no model, no locale.
///
/// A single letter is noise; a single *digit* is not — "Retry budget is 3" and
/// "Retry budget three" differ by exactly that token, and dropping it would
/// hide the one word that says they are the same subject.
fn tokens(text: &str) -> HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.len() > 1 || word.chars().all(|character| character.is_ascii_digit()))
        .map(|word| word.to_lowercase())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count() as f32;
    let total = a.union(b).count() as f32;
    if total == 0.0 {
        0.0
    } else {
        shared / total
    }
}

// ---------------------------------------------------------------------------
// Duplicates and near-duplicates
// ---------------------------------------------------------------------------

fn duplicates(
    store: &KnowledgeStore,
    live: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    findings.extend(exact_duplicates(store, live, options)?);
    findings.extend(subject_duplicates(store, live, options)?);
    findings.extend(near_duplicates(store, live, options)?);
    findings.extend(title_duplicates(store, live, options)?);
    Ok(findings)
}

/// Two live claims whose titles describe the same subject.
///
/// "Retry budget three" and "Retry budget is 3" are one subject written twice,
/// and no raw token comparison sees it: one says `three` where the other says
/// `3`, and a two-letter stop word moves the score either way. Folding number
/// words to digits and dropping the words that carry no subject makes the two
/// titles identical, and the finding says exactly that, so a reviewer can
/// disagree with the normalisation rather than with a bare score.
///
/// Only titles are normalised this way. Doing it to whole bodies would merge
/// pages that merely share vocabulary.
fn subject_duplicates(
    store: &KnowledgeStore,
    live: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut subjects: BTreeMap<String, Vec<&crate::model::KnowledgeNode>> = BTreeMap::new();
    for node in live.iter().filter(|node| !node.node_type.is_structural()) {
        let subject = subject_tokens(&node.title);
        if subject.len() < 2 {
            continue;
        }
        subjects.entry(subject.join(" ")).or_default().push(node);
    }

    let mut findings = Vec::new();
    for (subject, group) in subjects {
        if group.len() < 2 {
            continue;
        }
        for pair in group.windows(2) {
            let matched = subject.split(' ').map(str::to_string).collect::<Vec<_>>();
            findings.push(finding(
                FindingKind::Duplicate,
                FindingSeverity::Warning,
                pair[0].title.clone(),
                format!(
                    "describes the same subject as {:?}: both titles reduce to {:?} once number \
                     words and stop words are normalised; one page should absorb the other",
                    pair[1].title,
                    matched.join(" ")
                ),
                vec![pair[0].id.clone(), pair[1].id.clone()],
                evidence_of(store, &pair[0].id, &pair[1].id)?,
                None,
                1.0,
                matched,
            ));
            if findings.len() >= options.max_findings {
                return Ok(findings);
            }
        }
    }
    Ok(findings)
}

/// The subject a title names: its words, with number words folded to digits and
/// the words that carry no subject removed.
fn subject_tokens(title: &str) -> Vec<String> {
    let mut tokens = tokens(title)
        .into_iter()
        .filter(|token| !SUBJECT_STOP_WORDS.contains(&token.as_str()))
        .map(|token| {
            NUMBER_WORDS
                .binary_search_by(|(word, _)| word.cmp(&token.as_str()))
                .map_or(token.clone(), |index| NUMBER_WORDS[index].1.to_string())
        })
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    tokens
}

/// Words that carry no subject of their own.
const SUBJECT_STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "of", "for", "to", "in", "on", "at", "and",
    "or", "it", "its", "this", "that", "these", "those", "with", "by", "as", "from", "then",
    "than", "into", "over", "under", "about", "note", "notes",
];

/// Number words, so "three" and "3" are the same subject. Sorted by word, which
/// a binary search relies on.
const NUMBER_WORDS: &[(&str, &str)] = &[
    ("eight", "8"),
    ("eighteen", "18"),
    ("eleven", "11"),
    ("fifteen", "15"),
    ("five", "5"),
    ("four", "4"),
    ("fourteen", "14"),
    ("nine", "9"),
    ("nineteen", "19"),
    ("one", "1"),
    ("seven", "7"),
    ("seventeen", "17"),
    ("six", "6"),
    ("sixteen", "16"),
    ("ten", "10"),
    ("thirteen", "13"),
    ("three", "3"),
    ("twelve", "12"),
    ("twenty", "20"),
    ("two", "2"),
    ("zero", "0"),
];

/// Two live claims whose titles are nearly the same words.
///
/// Titles are short and unusually specific — "Retry budget is three" and "Retry
/// budget is 3" describe one subject and differ by a single token — so they are
/// compared on their own, at the same threshold, rather than being swamped by
/// whatever the bodies happen to share. Reported as duplicates when the title
/// token sets are identical, and near-duplicates otherwise, with the score and
/// the words that matched.
fn title_duplicates(
    store: &KnowledgeStore,
    live: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let candidates = live
        .iter()
        .filter(|node| !node.node_type.is_structural())
        .map(|node| (node, tokens(&node.title)))
        .filter(|(_, tokens)| tokens.len() >= 2)
        .collect::<Vec<_>>();

    let mut findings = Vec::new();
    let mut reported: HashSet<(NodeId, NodeId)> = HashSet::new();
    for (position, (left_node, left_tokens)) in candidates.iter().enumerate() {
        for (right_node, right_tokens) in candidates.iter().skip(position + 1) {
            // Same knowledge type or not: a rule and a gotcha with one title are
            // still worth asking about, but a structural node is never a claim.
            if left_node.id == right_node.id {
                continue;
            }
            let score = jaccard(left_tokens, right_tokens);
            if score < options.near_duplicate_threshold {
                continue;
            }
            let mut pair = [left_node.id.clone(), right_node.id.clone()];
            pair.sort();
            if !reported.insert((pair[0].clone(), pair[1].clone())) {
                continue;
            }
            let mut matched = left_tokens
                .intersection(right_tokens)
                .cloned()
                .collect::<Vec<_>>();
            matched.sort();
            let exact = left_tokens == right_tokens;
            findings.push(finding(
                if exact {
                    FindingKind::Duplicate
                } else {
                    FindingKind::NearDuplicate
                },
                FindingSeverity::Warning,
                left_node.title.clone(),
                format!(
                    "has nearly the same title as {:?} (score {:.2}); matched: {}",
                    right_node.title,
                    score,
                    matched.join(", ")
                ),
                vec![left_node.id.clone(), right_node.id.clone()],
                evidence_of(store, &left_node.id, &right_node.id)?,
                None,
                score,
                matched,
            ));
            if findings.len() >= options.max_findings {
                return Ok(findings);
            }
        }
    }
    Ok(findings)
}

/// Two live nodes that are the same knowledge: one title key, or one identical
/// non-empty body.
fn exact_duplicates(
    store: &KnowledgeStore,
    live: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let mut by_title: BTreeMap<String, Vec<&crate::model::KnowledgeNode>> = BTreeMap::new();
    let mut by_body: BTreeMap<&str, Vec<&crate::model::KnowledgeNode>> = BTreeMap::new();
    for node in live.iter().filter(|node| !node.node_type.is_structural()) {
        by_title
            .entry(crate::authority::subject_key(node).title)
            .or_default()
            .push(node);
        let body = node.content.trim();
        if body.len() >= 40 {
            by_body.entry(body).or_default().push(node);
        }
    }

    let mut reported: HashSet<(NodeId, NodeId)> = HashSet::new();
    let mut push = |findings: &mut Vec<Finding>,
                    a: &crate::model::KnowledgeNode,
                    b: &crate::model::KnowledgeNode,
                    feature: String|
     -> Result<()> {
        let mut pair = [a.id.clone(), b.id.clone()];
        pair.sort();
        if !reported.insert((pair[0].clone(), pair[1].clone())) {
            return Ok(());
        }
        findings.push(finding(
            FindingKind::Duplicate,
            FindingSeverity::Warning,
            a.title.clone(),
            format!(
                "is the same knowledge as {:?} ({}); one of them should be merged away",
                b.title, feature
            ),
            vec![a.id.clone(), b.id.clone()],
            evidence_of(store, &a.id, &b.id)?,
            None,
            1.0,
            vec![feature],
        ));
        Ok(())
    };

    for (_, group) in by_title {
        if group.len() < 2 {
            continue;
        }
        for pair in group.windows(2) {
            push(
                &mut findings,
                pair[0],
                pair[1],
                format!(
                    "same title key {:?}",
                    crate::authority::subject_key(pair[0]).title
                ),
            )?;
            if findings.len() >= options.max_findings {
                return Ok(findings);
            }
        }
    }
    for (_, group) in by_body {
        if group.len() < 2 {
            continue;
        }
        for pair in group.windows(2) {
            push(
                &mut findings,
                pair[0],
                pair[1],
                format!(
                    "identical body, {} characters",
                    pair[0].content.trim().len()
                ),
            )?;
            if findings.len() >= options.max_findings {
                return Ok(findings);
            }
        }
    }
    Ok(findings)
}

/// Two live claims that are nearly the same, with the score that says so.
///
/// Blocked by the rarest shared token rather than compared pairwise: a
/// knowledge base of ten thousand pages is not asked to make fifty million
/// comparisons, and a near-duplicate that shares no rare word with anything is
/// not a near-duplicate.
fn near_duplicates(
    store: &KnowledgeStore,
    live: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let candidates = live
        .iter()
        .filter(|node| !node.node_type.is_structural())
        .map(|node| (node, tokens(&proposition(node))))
        .filter(|(_, tokens)| tokens.len() >= options.min_tokens)
        .collect::<Vec<_>>();

    // How many claims each word appears in. The rarest word a claim contains is
    // the most selective bucket it can be filed under.
    let mut frequency: HashMap<&str, usize> = HashMap::new();
    for (_, tokens) in &candidates {
        for token in tokens {
            *frequency.entry(token.as_str()).or_default() += 1;
        }
    }
    let mut buckets: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, (_, tokens)) in candidates.iter().enumerate() {
        if let Some(rarest) = tokens.iter().min_by_key(|token| {
            (
                frequency.get(token.as_str()).copied().unwrap_or(0),
                token.as_str(),
            )
        }) {
            buckets.entry(rarest.as_str()).or_default().push(index);
        }
    }

    let mut reported: HashSet<(NodeId, NodeId)> = HashSet::new();
    for (_, bucket) in buckets {
        if bucket.len() < 2 {
            continue;
        }
        for (position, left) in bucket.iter().enumerate() {
            let (left_node, left_tokens) = &candidates[*left];
            let compared = bucket
                .iter()
                .skip(position + 1)
                .take(options.max_candidates);
            for right in compared {
                let (right_node, right_tokens) = &candidates[*right];
                let score = jaccard(left_tokens, right_tokens);
                let shared = left_tokens.intersection(right_tokens).count();
                // Two short claims sharing one word are not near-duplicates, so
                // a body match must clear the threshold *and* share enough
                // words to mean something.
                if score < options.near_duplicate_threshold || shared < 3 {
                    continue;
                }
                let mut pair = [left_node.id.clone(), right_node.id.clone()];
                pair.sort();
                if !reported.insert((pair[0].clone(), pair[1].clone())) {
                    continue;
                }
                // The features are the words that carried the match, longest
                // first, so a reader can see why the two were called similar.
                let mut matched = left_tokens
                    .intersection(right_tokens)
                    .filter(|token| token.len() > 4)
                    .cloned()
                    .collect::<Vec<_>>();
                matched.sort();
                matched.truncate(8);
                findings.push(finding(
                    FindingKind::NearDuplicate,
                    FindingSeverity::Warning,
                    left_node.title.clone(),
                    format!(
                        "overlaps {:?} at {:.2} ({} shared words of {}); matched: {}",
                        right_node.title,
                        score,
                        shared,
                        left_tokens.union(right_tokens).count(),
                        matched.join(", ")
                    ),
                    vec![left_node.id.clone(), right_node.id.clone()],
                    evidence_of(store, &left_node.id, &right_node.id)?,
                    None,
                    score,
                    matched,
                ));
                if findings.len() >= options.max_findings {
                    return Ok(findings);
                }
            }
        }
    }
    Ok(findings)
}

// ---------------------------------------------------------------------------
// Supersession, anchors, unsupported rules, proposals, scans, orphans
// ---------------------------------------------------------------------------

fn supersession(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
    edges: &[crate::model::KnowledgeEdge],
    titles: &HashMap<NodeId, String>,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();

    for edge in edges {
        if edge.edge_type != EdgeType::Supersedes || !edge.is_traversable() {
            continue;
        }
        // `from supersedes to`: arriving at `to`, the reader is holding
        // knowledge that something else has replaced.
        let Some(superseded) = by_id.get(&edge.to) else {
            continue;
        };
        if !superseded.is_live() {
            continue;
        }
        findings.push(finding(
            FindingKind::Superseded,
            FindingSeverity::Warning,
            superseded.title.clone(),
            format!(
                "has been superseded by {:?} and is still being offered",
                title_of(titles, &edge.from)
            ),
            vec![edge.to.clone(), edge.from.clone()],
            evidence_of(store, &edge.to, &edge.from)?,
            None,
            1.0,
            vec!["recorded supersedes edge".to_string()],
        ));
    }

    for node in nodes {
        if node.status != NodeStatus::Superseded {
            continue;
        }
        findings.push(finding(
            FindingKind::Superseded,
            FindingSeverity::Info,
            node.title.clone(),
            "is retained but no longer offered; delete it or confirm the replacement".to_string(),
            vec![node.id.clone()],
            store
                .evidence_for(&node.id)?
                .iter()
                .map(|e| e.id.clone())
                .collect(),
            None,
            1.0,
            vec!["node status superseded".to_string()],
        ));
    }
    Ok(findings)
}

/// Stale anchors, both the recorded ones and the ones a check finds now.
fn anchors(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();

    for node in nodes {
        match node.status {
            NodeStatus::StaleRequiresReview => findings.push(finding(
                FindingKind::StaleAnchor,
                FindingSeverity::Blocking,
                node.title.clone(),
                "no longer resolves to anything in the source it came from".to_string(),
                vec![node.id.clone()],
                store
                    .evidence_for(&node.id)?
                    .iter()
                    .map(|e| e.id.clone())
                    .collect(),
                anchor_file(store, &node.id)?,
                1.0,
                vec!["node status stale_requires_review".to_string()],
            )),
            NodeStatus::Stale => findings.push(finding(
                FindingKind::StaleAnchor,
                FindingSeverity::Warning,
                node.title.clone(),
                "rests on source that has moved or changed since it was written".to_string(),
                vec![node.id.clone()],
                store
                    .evidence_for(&node.id)?
                    .iter()
                    .map(|e| e.id.clone())
                    .collect(),
                anchor_file(store, &node.id)?,
                1.0,
                vec!["node status stale".to_string()],
            )),
            _ => {}
        }
    }

    if !options.verify_anchors {
        return Ok(findings);
    }

    // Read the anchored files, once each, and check the hashes the anchors
    // recorded. This is the only part of a sweep that touches disk, which is
    // why it is bounded and switchable.
    let project = store.project_root();
    let evidence = store.anchored_evidence()?;
    let mut checked: HashMap<String, Option<String>> = HashMap::new();
    let mut files_read = 0_usize;
    let mut bytes_read = 0_u64;
    for item in evidence {
        let Some(anchor) = &item.anchor else {
            continue;
        };
        // An anchor that recorded only a path is still an anchor: the file it
        // names can be deleted, and saying so is the whole point of tracking it.
        // The hashes are checked when they exist, and their absence is not a
        // reason to look away.
        let Some(project) = project.as_deref() else {
            continue;
        };
        let path = anchor.file.replace('\\', "/");
        if !checked.contains_key(&path) {
            if files_read >= options.max_anchor_files {
                break;
            }
            files_read += 1;
            let content = std::fs::read_to_string(project.join(&path)).ok();
            bytes_read += content.as_ref().map_or(0, |text| text.len() as u64);
            checked.insert(path.clone(), content);
            if bytes_read > options.max_anchor_bytes {
                break;
            }
        }
        let Some(content) = checked.get(&path).and_then(Option::as_deref) else {
            findings.push(finding(
                FindingKind::StaleAnchor,
                FindingSeverity::Blocking,
                title_of_node(nodes, &item.node),
                format!("its evidence points at {path}, which is no longer there"),
                vec![item.node.clone()],
                vec![item.id.clone()],
                Some(path.clone()),
                1.0,
                vec!["anchored file is missing".to_string()],
            ));
            continue;
        };
        let content_matches = anchor
            .content_hash
            .as_deref()
            .is_none_or(|expected| expected == hash(content));
        let region_matches = match (
            anchor.region_hash.as_deref(),
            anchor.line_start,
            anchor.line_end,
        ) {
            (Some(expected), Some(start), Some(end)) => {
                crate::repository::region_by_lines(content, start, end)
                    .is_some_and(|region| hash(region) == expected)
            }
            _ => true,
        };
        if !content_matches || !region_matches {
            findings.push(finding(
                FindingKind::StaleAnchor,
                FindingSeverity::Warning,
                title_of_node(nodes, &item.node),
                format!(
                    "its evidence anchored {} has changed since ({}); re-check the claim",
                    path,
                    if region_matches {
                        "the whole file no longer hashes to the recorded value"
                    } else {
                        "the anchored region no longer hashes to the recorded value"
                    }
                ),
                vec![item.node.clone()],
                vec![item.id.clone()],
                Some(path.clone()),
                0.9,
                vec![if region_matches {
                    "content hash mismatch".to_string()
                } else {
                    "region hash mismatch".to_string()
                }],
            ));
        }
    }
    Ok(findings)
}

fn unsupported(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for node in nodes {
        if !matches!(node.node_type, NodeType::Rule | NodeType::Decision) || !node.is_live() {
            continue;
        }
        if store.evidence_for(&node.id)?.is_empty() {
            findings.push(finding(
                FindingKind::UnsupportedRule,
                FindingSeverity::Warning,
                node.title.clone(),
                format!("is a {} with no evidence behind it", node.node_type),
                vec![node.id.clone()],
                Vec::new(),
                None,
                1.0,
                vec!["no evidence rows".to_string()],
            ));
        }
    }
    Ok(findings)
}

fn proposals(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
    edges: &[crate::model::KnowledgeEdge],
    titles: &HashMap<NodeId, String>,
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for node in nodes.iter().filter(|node| memory::is_proposal(node)) {
        let attribution = node
            .provenance
            .session
            .as_ref()
            .map(|session| format!(" in session {session}"))
            .unwrap_or_default();
        findings.push(finding(
            FindingKind::UnreviewedProposal,
            FindingSeverity::Info,
            node.title.clone(),
            format!(
                "proposed by {}{attribution} and not yet accepted; accepting writes it to the vault",
                node.provenance.detail.as_deref().unwrap_or("a model")
            ),
            vec![node.id.clone()],
            store
                .evidence_for(&node.id)?
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect(),
            None,
            1.0,
            vec!["model provenance, not vault-owned".to_string()],
        ));
        if findings.len() >= options.max_findings {
            return Ok(findings);
        }
    }

    for edge in edges
        .iter()
        .filter(|edge| edge.status == NodeStatus::Proposed)
    {
        let by = match edge.provenance.source {
            ProvenanceSource::Model => "a model",
            ProvenanceSource::Import => "an import",
            _ => "something",
        };
        findings.push(finding(
            FindingKind::UnreviewedProposal,
            FindingSeverity::Info,
            title_of(titles, &edge.from),
            format!(
                "{by} proposed {} → {}; it is not traversed until promoted",
                edge.edge_type,
                title_of(titles, &edge.to)
            ),
            vec![edge.from.clone(), edge.to.clone()],
            Vec::new(),
            None,
            1.0,
            vec![format!("edge status proposed ({})", edge.provenance.source)],
        ));
        if findings.len() >= options.max_findings {
            return Ok(findings);
        }
    }
    Ok(findings)
}

/// The last scans, when either stopped before the end of the tree.
///
/// This is the finding that stops "we indexed everything" from being a claim
/// nobody can check: the scan recorded what it skipped, and this reports it.
fn incomplete_scans(store: &KnowledgeStore) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for scope in SourceScope::ALL {
        let Some(state) = scan_state(store, *scope)? else {
            continue;
        };
        if state.complete && state.skipped.is_empty() {
            continue;
        }
        let reasons = state
            .skipped
            .iter()
            .take(5)
            .map(|file| format!("{} ({})", file.path, file.reason))
            .collect::<Vec<_>>();
        let detail = if state.complete {
            format!(
                "the last {scope} scan read {} file(s) and skipped {}; skipped: {}",
                state.scanned,
                state.skipped_total,
                reasons.join(", ")
            )
        } else {
            format!(
                "the last {scope} scan stopped before the end of the tree after {} file(s), \
                 skipping {}; skipped: {}",
                state.scanned,
                state.skipped_total,
                reasons.join(", ")
            )
        };
        findings.push(finding(
            FindingKind::IncompleteScan,
            FindingSeverity::Warning,
            format!("{scope} scan incomplete"),
            detail,
            Vec::new(),
            Vec::new(),
            state.skipped.first().map(|file| file.path.clone()),
            if state.complete { 0.8 } else { 1.0 },
            state
                .skipped
                .iter()
                .map(|file| format!("{}: {}", file.path, file.reason))
                .collect(),
        ));
    }
    Ok(findings)
}

fn orphans(
    store: &KnowledgeStore,
    nodes: &[crate::model::KnowledgeNode],
    edges: &[crate::model::KnowledgeEdge],
    options: &FindingOptions,
) -> Result<Vec<Finding>> {
    let mut reachable = HashSet::new();
    for edge in edges.iter().filter(|edge| edge.is_traversable()) {
        reachable.insert(edge.from.clone());
        reachable.insert(edge.to.clone());
    }
    let mut findings = Vec::new();
    // Only claims can be orphaned in a way somebody must fix. A file with no
    // declarations and a commit nobody linked are things the index read, not
    // assertions waiting for a home, and reporting them buries the pages that
    // genuinely link to nothing.
    for node in nodes
        .iter()
        .filter(|node| node.is_live() && !node.node_type.is_structural())
    {
        if reachable.contains(&node.id) {
            continue;
        }
        findings.push(finding(
            FindingKind::Orphan,
            FindingSeverity::Warning,
            node.title.clone(),
            "has no traversable relationship, so navigation can never reach it".to_string(),
            vec![node.id.clone()],
            store
                .evidence_for(&node.id)?
                .iter()
                .map(|e| e.id.clone())
                .collect(),
            None,
            1.0,
            vec!["no traversable edge".to_string()],
        ));
        if findings.len() >= options.max_findings {
            break;
        }
    }
    Ok(findings)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn finding(
    kind: FindingKind,
    severity: FindingSeverity,
    title: String,
    detail: String,
    nodes: Vec<NodeId>,
    evidence: Vec<EvidenceId>,
    source_path: Option<String>,
    confidence: f32,
    features: Vec<String>,
) -> Finding {
    // The id is derived from what the finding is about, never from when it was
    // found, so a finding keeps its identity across runs and a UI list does not
    // churn.
    let mut identity = format!("{}|{}|{}", kind.as_str(), title, detail);
    for node in &nodes {
        identity.push('|');
        identity.push_str(node.as_str());
    }
    Finding {
        id: format!("finding_{}_{}", kind.as_str(), &hash(&identity)[..16]),
        kind,
        severity,
        title,
        detail,
        nodes,
        evidence,
        source_path,
        confidence: confidence.clamp(0.0, 1.0),
        features,
    }
}

fn title_of(titles: &HashMap<NodeId, String>, id: &NodeId) -> String {
    titles.get(id).cloned().unwrap_or_else(|| id.to_string())
}

fn title_of_node(nodes: &[crate::model::KnowledgeNode], id: &NodeId) -> String {
    nodes
        .iter()
        .find(|node| node.id == *id)
        .map(|node| node.title.clone())
        .unwrap_or_else(|| id.to_string())
}

/// The evidence of one node, or of two when a finding is about a pair.
fn evidence_of(store: &KnowledgeStore, a: &NodeId, b: &NodeId) -> Result<Vec<EvidenceId>> {
    let mut evidence = store
        .evidence_for(a)?
        .into_iter()
        .map(|evidence| evidence.id)
        .collect::<Vec<_>>();
    if a != b {
        evidence.extend(
            store
                .evidence_for(b)?
                .into_iter()
                .map(|evidence| evidence.id),
        );
    }
    evidence.sort();
    evidence.dedup();
    Ok(evidence)
}

/// The first anchored file a node's evidence points at, for a finding's
/// `sourcePath`.
fn anchor_file(store: &KnowledgeStore, node: &NodeId) -> Result<Option<String>> {
    Ok(store
        .evidence_for(node)?
        .into_iter()
        .filter_map(|evidence| evidence.anchor.map(|anchor| anchor.file))
        .next())
}
