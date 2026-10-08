//! Structural lint, projected from the index.
//!
//! Lint asks three questions about a wiki: what does nothing link to, what
//! links to nothing, and what links at something that is not there. All
//! three are questions about *edges*, and the index is where the edges
//! already are.
//!
//! The app used to answer them by walking `wiki/` a third time — reading
//! every page, re-extracting every `[[wikilink]]`, and re-implementing link
//! resolution in TypeScript beside the copy that already lives in
//! [`super::index`]. Two implementations of "does this link resolve?" do not
//! stay equal, and the one the user sees findings from was the one nothing
//! else was tested against.
//!
//! So this is a projection, in the same sense as [`super::graph`]: it reads
//! nodes and edges and returns findings. Nothing here parses markdown, and
//! the resolution rule it reports on is the indexer's own, because the
//! unresolved links it reports come from the indexing pass rather than from
//! a second attempt at the same question.
//!
//! # What counts as a link
//!
//! A relationship between two *pages*: a `[[wikilink]]` in the body
//! ([`EdgeType::References`]) or a `related:` entry in the frontmatter
//! ([`EdgeType::RelatedTo`]). A `sources:` citation points at raw material
//! rather than at another page, so it neither rescues an orphan nor counts
//! as an outlink — a page that cites a PDF and nothing else is exactly as
//! disconnected from the wiki as one that cites nothing.
//!
//! Only established edges count. An edge a model proposed and nobody
//! accepted is a suggestion; letting it silence an orphan finding would let
//! a proposal quietly decide that a page is connected.
//!
//! [`EdgeType::References`]: crate::model::EdgeType::References
//! [`EdgeType::RelatedTo`]: crate::model::EdgeType::RelatedTo

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::NodeId;
use crate::model::EdgeType;
use crate::store::KnowledgeStore;

use super::graph::{self, GraphOrigin, KnowledgeGraph};
use super::index::{UnresolvedKind, UnresolvedLink};
use super::{VaultIndexer, VaultScanOptions};

/// What a finding says is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintFindingKind {
    /// No other page links to this one.
    Orphan,
    /// This page links at something the vault does not contain.
    BrokenLink,
    /// This page links to nothing.
    NoOutlinks,
}

/// How much a finding matters.
///
/// A broken link is a promise the wiki fails to keep, so it is a warning.
/// Being unlinked is a shape the wiki has, not an error in it: plenty of
/// vaults have deliberate leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintSeverity {
    Warning,
    Info,
}

/// One thing worth a person's attention, with the fix it suggests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintFinding {
    #[serde(rename = "type")]
    pub kind: LintFindingKind,
    pub severity: LintSeverity,
    /// Vault-relative path of the page the finding is about, which is also
    /// the page a fix edits.
    pub page: String,
    /// The indexed node behind `page`, so a caller can open the finding in
    /// the knowledge panels without re-deriving an id from a path.
    pub node: NodeId,
    pub detail: String,
    /// For a broken link, the target exactly as it was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken_target: Option<String>,
    /// A page this one could link to: the repair for a broken link, or the
    /// first outlink for a page that has none. Vault-relative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_target: Option<String>,
    /// A page that could link to this one, for an orphan. Vault-relative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_source: Option<String>,
}

/// What the user has told lint to stop mentioning.
///
/// Ignoring a check is a legitimate answer: a vault of standalone notes has
/// no use for orphan findings, and saying so once is better than dismissing
/// the same list every week.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LintOptions {
    pub ignore_orphan: bool,
    pub ignore_no_outlinks: bool,
    /// Pages to skip entirely, named by slug or by path with or without the
    /// `wiki/` prefix and the `.md` suffix.
    pub ignore_pages: Vec<String>,
}

/// What one lint pass found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultLintReport {
    /// Pages considered, which is every indexed page minus the ignored ones.
    pub pages: usize,
    pub findings: Vec<LintFinding>,
    /// False when the index pass behind this lint hit a scan budget. The
    /// findings are still real; the absence of a finding is not.
    pub scan_complete: bool,
    /// True when the indexer stopped collecting unresolved links before it
    /// ran out of them, so the broken-link findings are a sample.
    pub broken_links_truncated: bool,
}

/// How many findings get a suggested fix before the search stops.
///
/// Each suggestion costs one full-text query. A vault that is mostly
/// orphans wants its list of orphans quickly, and nobody acts on the
/// four-hundredth suggestion in a sitting.
const SUGGESTION_BUDGET: usize = 200;

/// How close a broken target must be to an existing page before the
/// difference is more likely a typo than a different page.
const BROKEN_LINK_MIN_SIMILARITY: f32 = 0.74;

/// Reconcile the index against the vault, then lint what it now holds.
///
/// Indexing first is what makes the findings current: linting a stale index
/// would report links the user fixed a minute ago.
pub fn scan_and_lint(
    store: &mut KnowledgeStore,
    root: impl AsRef<Path>,
    scan: VaultScanOptions,
    options: &LintOptions,
) -> Result<VaultLintReport> {
    let report = VaultIndexer::scan_and_index(store, root, scan)?;
    let mut lint = project(store, &report.unresolved, options)?;
    lint.scan_complete = report.scan_complete;
    lint.broken_links_truncated = report.unresolved_truncated();
    Ok(lint)
}

/// Lint the index as it stands, given the unresolved links from the pass
/// that produced it.
///
/// Split from [`scan_and_lint`] so the projection can be tested against a
/// store rather than against a directory, and so a caller that has just
/// indexed does not index twice.
pub fn project(
    store: &KnowledgeStore,
    unresolved: &[UnresolvedLink],
    options: &LintOptions,
) -> Result<VaultLintReport> {
    let graph = graph::project(store)?;
    let pages = Pages::of(&graph, options);

    let mut findings = Vec::new();
    // Broken links first, and not only for tidiness: they are the findings
    // whose suggestion is worth most — a repairable typo — and the
    // suggestion budget is spent in the order findings are produced. The
    // output is sorted at the end, so this order is not visible.
    let mut budget = SUGGESTION_BUDGET;

    for link in unresolved {
        let Some(&index) = pages.by_path.get(&link.from) else {
            // The link's page is ignored, or is not a page the graph draws.
            continue;
        };
        let node = &graph.nodes[index];
        let suggested_target = spend(&mut budget, || {
            nearest_page(&graph, &pages, &link.target, index)
        });
        findings.push(LintFinding {
            kind: LintFindingKind::BrokenLink,
            severity: LintSeverity::Warning,
            page: link.from.clone(),
            node: node.id.clone(),
            detail: match link.kind {
                UnresolvedKind::Link => {
                    format!("Broken link: [[{}]] — target page not found.", link.target)
                }
                UnresolvedKind::Source => format!(
                    "Broken citation: sources: {} — no such file under raw/sources/.",
                    link.target
                ),
            },
            broken_target: Some(link.target.clone()),
            suggested_target,
            suggested_source: None,
        });
    }

    for page in &pages.linted {
        let node = &graph.nodes[page.index];
        if !options.ignore_orphan && !pages.has_inbound(&node.id) {
            let suggested_source = spend(&mut budget, || {
                related_page(store, &graph, &pages, page.index, Direction::Source)
            });
            findings.push(LintFinding {
                kind: LintFindingKind::Orphan,
                severity: LintSeverity::Info,
                page: page.path.clone(),
                node: node.id.clone(),
                detail: "No other pages link to this page.".to_string(),
                broken_target: None,
                suggested_target: None,
                suggested_source,
            });
        }
        if !options.ignore_no_outlinks && !pages.has_outbound(&node.id) {
            let suggested_target = spend(&mut budget, || {
                related_page(store, &graph, &pages, page.index, Direction::Target)
            });
            findings.push(LintFinding {
                kind: LintFindingKind::NoOutlinks,
                severity: LintSeverity::Info,
                page: page.path.clone(),
                node: node.id.clone(),
                detail: "This page links to no other page.".to_string(),
                broken_target: None,
                suggested_target,
                suggested_source: None,
            });
        }
    }

    findings.sort_by(|left, right| {
        left.page
            .cmp(&right.page)
            .then_with(|| left.kind.order().cmp(&right.kind.order()))
            .then_with(|| left.broken_target.cmp(&right.broken_target))
    });

    Ok(VaultLintReport {
        pages: pages.linted.len(),
        findings,
        scan_complete: true,
        broken_links_truncated: false,
    })
}

impl LintFindingKind {
    /// Stable order within a page: what the page promises and fails to
    /// deliver first, then what it is missing.
    fn order(self) -> u8 {
        match self {
            LintFindingKind::BrokenLink => 0,
            LintFindingKind::Orphan => 1,
            LintFindingKind::NoOutlinks => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// The pages under consideration, and how they are connected.
// ---------------------------------------------------------------------------

struct LintedPage {
    /// Index into `KnowledgeGraph::nodes`.
    index: usize,
    /// Vault-relative path.
    path: String,
}

/// Every page node in the graph, with the connectivity lint asks about.
struct Pages {
    linted: Vec<LintedPage>,
    /// Vault-relative path to graph node index, for the pages being
    /// linted. A link from a page that is not in here — an ignored one, or
    /// something that is not a page — is not reported.
    by_path: HashMap<String, usize>,
    inbound: HashSet<NodeId>,
    outbound: HashSet<NodeId>,
}

impl Pages {
    fn of(graph: &KnowledgeGraph, options: &LintOptions) -> Self {
        let ignored = options
            .ignore_pages
            .iter()
            .map(|value| normalize_target(value))
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>();

        let mut by_path = HashMap::new();
        let mut linted = Vec::new();
        // Node ids of page nodes, so an edge to a source or to curated
        // knowledge does not count as a link between pages.
        let mut page_ids = HashSet::new();

        for (index, node) in graph.nodes.iter().enumerate() {
            if node.origin != GraphOrigin::Page {
                continue;
            }
            let Some(path) = node.path.as_deref() else {
                continue;
            };
            // Every page counts as a *source* of links, ignored or not: a
            // page is not an orphan because the only page linking to it is
            // one the user muted.
            page_ids.insert(node.id.clone());
            // An ignore entry names a page by slug or by path. Matching a
            // bare basename against every nested page with that name would
            // let "foo" silently hide "archive/foo".
            if ignored.contains(&normalize_target(path)) {
                continue;
            }
            by_path.insert(path.to_string(), index);
            linted.push(LintedPage {
                index,
                path: path.to_string(),
            });
        }

        let mut inbound = HashSet::new();
        let mut outbound = HashSet::new();
        for edge in &graph.edges {
            if !edge.deterministic {
                continue;
            }
            if !matches!(edge.edge_type, EdgeType::References | EdgeType::RelatedTo) {
                continue;
            }
            if !page_ids.contains(&edge.from) || !page_ids.contains(&edge.to) {
                continue;
            }
            outbound.insert(edge.from.clone());
            inbound.insert(edge.to.clone());
        }

        Pages {
            linted,
            by_path,
            inbound,
            outbound,
        }
    }

    fn has_inbound(&self, id: &NodeId) -> bool {
        self.inbound.contains(id)
    }

    fn has_outbound(&self, id: &NodeId) -> bool {
        self.outbound.contains(id)
    }
}

/// Run `suggest` while there is budget for it.
fn spend<T>(budget: &mut usize, suggest: impl FnOnce() -> Option<T>) -> Option<T> {
    if *budget == 0 {
        return None;
    }
    *budget -= 1;
    suggest()
}

// ---------------------------------------------------------------------------
// Suggestions.
// ---------------------------------------------------------------------------

enum Direction {
    /// A page that could link to this one.
    Source,
    /// A page this one could link to.
    Target,
}

/// The most closely related other page, by the index's own ranking.
///
/// This deliberately reuses full-text search rather than growing a second
/// notion of relatedness. The knowledge model already decides what "related
/// to this text" means, every panel that searches gets that answer, and a
/// lint-only similarity score would be a fourth opinion about the same
/// vault.
fn related_page(
    store: &KnowledgeStore,
    graph: &KnowledgeGraph,
    pages: &Pages,
    index: usize,
    direction: Direction,
) -> Option<String> {
    let node = &graph.nodes[index];
    // The title is what a link to this page would be written as, so it is
    // also the best description of what a related page is about.
    let subject = store.find_node(&node.id).ok()??;
    let hits = store.similar(&subject, 8).ok()?;

    let already_linked = match direction {
        Direction::Source => HashSet::new(),
        Direction::Target => graph
            .edges
            .iter()
            .filter(|edge| edge.from == node.id)
            .map(|edge| edge.to.clone())
            .collect::<HashSet<_>>(),
    };

    // Ranked, not filtered by an absolute score: BM25 values are only
    // meaningful relative to other hits for the same query, so a numeric
    // bar would mean something different for every page. The top candidate
    // that is a page being linted and is not already linked is the
    // suggestion, and a suggestion is only ever an offer the user can
    // ignore.
    //
    // A page being *linted* rather than merely a page: acting on a
    // suggestion edits the page it names, and a muted page should not be
    // edited by a finding the user muted it to stop hearing about.
    hits.iter()
        .filter(|hit| !already_linked.contains(&hit.node.id))
        .find_map(|hit| {
            let candidate = graph
                .nodes
                .iter()
                .find(|candidate| candidate.id == hit.node.id)?;
            let path = candidate.path.as_deref()?;
            pages.by_path.contains_key(path).then(|| path.to_string())
        })
}

/// The existing page a broken target most plausibly meant.
///
/// Spelling, not meaning: a broken link is usually a typo, a renamed page
/// or a missing extension, and offering a semantically related page as the
/// repair for `[[chian-of-thought]]` would rewrite the link to point
/// somewhere the author never meant.
fn nearest_page(
    graph: &KnowledgeGraph,
    pages: &Pages,
    target: &str,
    holder: usize,
) -> Option<String> {
    let mut best: Option<(f32, &str)> = None;
    for page in &pages.linted {
        // A page cannot cite itself out of a broken link: the indexer drops
        // self-links, so "repairing" `[[chian-of-thought]]` on
        // chain-of-thought.md with its own name would leave the finding
        // exactly where it was.
        if page.index == holder {
            continue;
        }
        let node = &graph.nodes[page.index];
        let score = similarity(target, &page.path).max(similarity(target, &node.title));
        if score >= BROKEN_LINK_MIN_SIMILARITY && score > best.map_or(0.0, |(score, _)| score) {
            best = Some((score, page.path.as_str()));
        }
    }
    best.map(|(_, path)| path.to_string())
}

/// How alike two page names are, from 0 to 1.
fn similarity(left: &str, right: &str) -> f32 {
    let left = normalize_target(left);
    let right = normalize_target(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    let left_base = basename(&left);
    let right_base = basename(&right);
    if left_base == right_base {
        return 0.96;
    }
    if left.contains(&right) || right.contains(&left) {
        return 0.82;
    }
    // Below a handful of characters an edit distance says nothing: "ai"
    // and "ui" are one edit apart and unrelated.
    let left_chars = left_base.chars().collect::<Vec<_>>();
    let right_chars = right_base.chars().collect::<Vec<_>>();
    if left_chars.len() < 5 || right_chars.len() < 5 {
        return 0.0;
    }
    let distance = levenshtein(&left_chars, &right_chars) as f32;
    1.0 - distance / left_chars.len().max(right_chars.len()) as f32
}

fn levenshtein(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (i, left_char) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != right_char);
            current[j + 1] = (current[j] + 1).min(previous[j + 1] + 1).min(substitution);
        }
        previous.copy_from_slice(&current);
    }
    previous[right.len()]
}

/// Fold a link target, a path and an ignore entry into one spelling, so
/// `wiki/concepts/X.md`, `concepts/x` and `X` compare equal.
fn normalize_target(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value.trim().trim_start_matches("./");
    let value = value
        .strip_prefix("wiki/")
        .or_else(|| value.strip_prefix("Wiki/"))
        .unwrap_or(value);
    let value = value.strip_suffix(".md").unwrap_or(value);
    value.trim().to_lowercase()
}

fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}
