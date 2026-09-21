//! Turning a [`VaultSnapshot`] into nodes, edges and evidence.
//!
//! See the [module documentation](super) for what this layer is allowed to
//! claim and why every write it makes is reversible by deleting the
//! database.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::{EvidenceId, NodeId};
use crate::model::{
    hash, Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, NodeStatus,
    NodeType, Provenance, ProvenanceSource, Scope, SourceAnchor,
};
use crate::store::{KnowledgeStore, NodeFilter};

use super::page::{slug_of, VaultPage};
use super::{VaultScanOptions, VaultSnapshot, VaultSource};

/// Provenance detail prefix marking a node this indexer owns because a wiki
/// page exists. Anything without one of these markers is somebody else's —
/// the user's, the agent's — and is never touched.
///
/// The page's own declared type is appended after a colon
/// (`vault_index:page:concept`), and a declared `origin:` after that
/// (`vault_index:page:concept:deep-research`). The vault's vocabulary is
/// finer than the knowledge model's — `query`, `synthesis` and `thesis` all
/// reduce to a handful of [`NodeType`]s — and where a page came from is
/// precisely what provenance is for, so recording both there keeps them
/// available to a reader without inventing columns.
pub const PAGE_PROVENANCE: &str = "vault_index:page";
/// Provenance detail marking a node backed by a file in `raw/sources/`.
pub const SOURCE_PROVENANCE: &str = "vault_index:source";

const EDGE_LINK: &str = "vault_index:wikilink";
const EDGE_RELATED: &str = "vault_index:related";
const EDGE_SOURCE: &str = "vault_index:sources";
const EDGE_PREFIX: &str = "vault_index:";
/// Evidence rows are identified by id rather than provenance, which the
/// evidence table does not carry.
const EVIDENCE_PREFIX: &str = "ev_vault_";

/// Which promise a page failed to keep.
///
/// The two are worth telling apart: a missing page is usually a page the
/// wiki intends to have, and a missing source file is usually a citation
/// that was written before the file was dropped in, or after it moved.
/// Offering to create the first is helpful; offering to create the second
/// would invent evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedKind {
    /// A `[[wikilink]]` or a `related:` entry naming no page.
    #[default]
    Link,
    /// A `sources:` entry naming no file under `raw/sources/`.
    Source,
}

/// A link that names nothing the vault contains.
///
/// Worth reporting rather than silently dropping: an unresolved link is
/// usually a page the wiki intends to have and does not yet, which is
/// exactly the queue a "create missing page" action works from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedLink {
    /// Vault-relative path of the page holding the link.
    pub from: String,
    /// The link target exactly as written.
    pub target: String,
    #[serde(default)]
    pub kind: UnresolvedKind,
}

/// How many unresolved links a report carries before it stops collecting.
/// A vault mid-ingest can have thousands; the count stays exact either way.
const MAX_REPORTED_UNRESOLVED: usize = 500;

/// What one indexing pass found and changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultIndexReport {
    /// False when a scan budget truncated the walk. An incomplete pass
    /// never deletes.
    pub scan_complete: bool,
    pub pages: usize,
    pub sources: usize,
    /// Links that resolved to a page and became edges.
    pub links_resolved: usize,
    pub links_unresolved: usize,
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub nodes_removed: usize,
    pub edges_created: usize,
    pub edges_removed: usize,
    pub evidence_attached: usize,
    pub evidence_updated: usize,
    pub evidence_removed: usize,
    /// A bounded sample of unresolved links, sorted for stable output.
    pub unresolved: Vec<UnresolvedLink>,
    pub warnings: Vec<String>,
}

impl VaultIndexReport {
    /// True when the pass wrote nothing at all, which is what re-indexing an
    /// unchanged vault must do.
    pub fn is_no_op(&self) -> bool {
        self.nodes_created == 0
            && self.nodes_updated == 0
            && self.nodes_removed == 0
            && self.edges_created == 0
            && self.edges_removed == 0
            && self.evidence_attached == 0
            && self.evidence_updated == 0
            && self.evidence_removed == 0
    }

    /// True when [`Self::unresolved`] is a sample rather than the whole
    /// list, because the collector hit its budget.
    ///
    /// A caller that believed a truncated sample was complete would show a
    /// list of broken links that never shrinks as they are fixed.
    pub fn unresolved_truncated(&self) -> bool {
        self.unresolved.len() >= MAX_REPORTED_UNRESOLVED
    }
}

/// Builds and maintains the derived index over a vault.
#[derive(Debug, Clone, Copy, Default)]
pub struct VaultIndexer;

impl VaultIndexer {
    /// Scan the vault at `root` and reconcile the store against it.
    pub fn scan_and_index(
        store: &mut KnowledgeStore,
        root: impl AsRef<Path>,
        options: VaultScanOptions,
    ) -> Result<VaultIndexReport> {
        let snapshot = VaultSnapshot::scan(root.as_ref(), options)?;
        Self::index(store, &snapshot)
    }

    /// Reconcile the store against an already-scanned vault.
    ///
    /// Writes only differences. Re-running this against an unchanged
    /// snapshot performs no writes and returns a report for which
    /// [`VaultIndexReport::is_no_op`] holds.
    pub fn index(store: &mut KnowledgeStore, snapshot: &VaultSnapshot) -> Result<VaultIndexReport> {
        let mut report = VaultIndexReport {
            scan_complete: snapshot.complete,
            pages: snapshot.pages.len(),
            sources: snapshot.sources.len(),
            warnings: snapshot.warnings.clone(),
            ..VaultIndexReport::default()
        };

        let mut owned = HashSet::new();
        for page in &snapshot.pages {
            let node = page_node(page);
            owned.insert(node.id.clone());
            ensure_node(store, node, &mut report)?;
        }
        for source in &snapshot.sources {
            let node = source_node(source);
            owned.insert(node.id.clone());
            ensure_node(store, node, &mut report)?;
        }

        // A truncated scan is not evidence that anything was deleted.
        if snapshot.complete {
            remove_departed(store, &owned, &mut report)?;
        }

        let resolver = Resolver::build(snapshot);
        let mut unresolved = Vec::new();

        for page in &snapshot.pages {
            let node = page_node_id(&page.path);
            let (edges, evidence) = desired_for_page(page, &resolver, &mut report, &mut unresolved);
            reconcile_edges(store, &node, &edges, &mut report)?;
            reconcile_evidence(store, &node, &evidence, &mut report)?;
        }
        for source in &snapshot.sources {
            let node = source_node_id(&source.identity);
            let evidence = desired_for_source(source);
            reconcile_edges(store, &node, &BTreeMap::new(), &mut report)?;
            reconcile_evidence(store, &node, &evidence, &mut report)?;
        }

        report.links_unresolved = unresolved.len();
        unresolved.sort();
        unresolved.dedup();
        unresolved.truncate(MAX_REPORTED_UNRESOLVED);
        report.unresolved = unresolved;

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// The vault's vocabulary, translated into the knowledge model's.
// ---------------------------------------------------------------------------

/// Map a vault page type onto a [`NodeType`].
///
/// The two vocabularies were designed for different jobs — the vault's
/// describes what a *page* is, the model's describes what a *claim* is — so
/// the mapping is a judgement call in a few places and is written out here
/// rather than scattered:
///
/// * `query` and `thesis` are open questions and working hypotheses, so
///   they land on [`NodeType::Observation`]: discovered, not established.
/// * `synthesis` and `finding` are conclusions drawn from cited sources,
///   so they land on [`NodeType::Fact`].
/// * `comparison` is analysis of relationships between things, which is
///   closest to [`NodeType::Concept`].
/// * anything unrecognized — a custom directory from a project template,
///   a type a person invented — becomes [`NodeType::Observation`]. That is
///   the model's own default for "discovered but not yet established", and
///   it means an unfamiliar page is indexed rather than refused.
fn node_type_for(page_type: &str) -> NodeType {
    // A type the knowledge model already knows by that exact name wins, so
    // a page declaring `type: decision` or `type: gotcha` needs no entry.
    if let Some(known) = NodeType::parse_opt(page_type) {
        return known;
    }
    match page_type {
        "overview" => NodeType::Project,
        // The vault's two governance documents.
        "schema" => NodeType::Rule,
        "purpose" => NodeType::Decision,
        "comparison" | "theme" | "plot-thread" | "topic" => NodeType::Concept,
        "synthesis" | "finding" | "reflection" => NodeType::Fact,
        "query" | "thesis" | "meeting" | "goal" | "journal" | "note" => NodeType::Observation,
        "methodology" | "habit" | "protocol" => NodeType::Procedure,
        "character" | "stakeholder" | "person" | "organization" => NodeType::Entity,
        "chapter" | "paper" | "article" => NodeType::Source,
        _ => NodeType::Observation,
    }
}

/// Where a page sits in the authority hierarchy.
///
/// A declared `authority:` always wins — the frontmatter is part of the
/// vault, so a person or the agent writing one there is stating it in the
/// layer that owns it. The fallbacks exist so a page that predates the
/// convention still gets something better than the floor value.
fn authority_for(page: &VaultPage) -> Authority {
    if let Some(declared) = page.authority {
        return declared;
    }
    match page.path.as_str() {
        // `schema.md` governs how every other page is written, and
        // `purpose.md` states what the project is for. Both are project
        // rules in the strict sense: they are meant to govern behaviour.
        "schema.md" => Authority::ProjectRule,
        "purpose.md" => Authority::ProjectDecision,
        // A page citing source material rests on something outside itself.
        _ if !page.sources.is_empty() => Authority::ExternalKnowledge,
        // Everything else is prose somebody wrote. True, probably; verified,
        // not yet. Nothing here promotes it — that is a curation decision,
        // and curation is not a deterministic pass's business.
        _ => Authority::UnverifiedObservation,
    }
}

fn page_node(page: &VaultPage) -> KnowledgeNode {
    KnowledgeNode::new(node_type_for(&page.page_type), page.title.clone())
        .with_id(page_node_id(&page.path))
        .with_summary(page.summary.clone())
        .with_content(page.body.trim().to_string())
        .with_scope(Scope::Project)
        .with_authority(authority_for(page))
        .with_status(NodeStatus::Valid)
        .with_provenance(Provenance::deterministic(page_provenance(page)))
}

/// `vault_index:page:concept:deep-research` — the marker, the vault's own
/// page type, and where the page says it came from. Trailing segments are
/// omitted when the page declares nothing.
fn page_provenance(page: &VaultPage) -> String {
    match (page.page_type.as_str(), page.origin.as_deref()) {
        ("", None) => PAGE_PROVENANCE.to_string(),
        (page_type, None) => format!("{PAGE_PROVENANCE}:{page_type}"),
        // A page with an origin but no type still needs the type slot
        // filled, or the two would be indistinguishable when parsed back.
        ("", Some(origin)) => format!("{PAGE_PROVENANCE}::{origin}"),
        (page_type, Some(origin)) => format!("{PAGE_PROVENANCE}:{page_type}:{origin}"),
    }
}

fn source_node(source: &VaultSource) -> KnowledgeNode {
    KnowledgeNode::new(NodeType::Source, source.identity.clone())
        .with_id(source_node_id(&source.identity))
        .with_summary(format!(
            "Source material at {} ({}).",
            source.path,
            human_bytes(source.bytes)
        ))
        .with_scope(Scope::File)
        .with_authority(Authority::ExternalKnowledge)
        .with_status(NodeStatus::Valid)
        .with_provenance(Provenance::deterministic(SOURCE_PROVENANCE))
}

/// Node ids are a hash of the vault-relative path, not of the title.
///
/// The file is the source of truth, so the file's location is the identity.
/// Renaming a page's `title:` keeps its node and its history; moving the
/// file is a different page, which is the same rule Obsidian's own links
/// follow.
fn page_node_id(path: &str) -> NodeId {
    NodeId::from_existing(format!("vault_page_{}", hash(path)))
}

fn source_node_id(identity: &str) -> NodeId {
    NodeId::from_existing(format!("vault_source_{}", hash(identity)))
}

fn page_evidence_id(path: &str) -> EvidenceId {
    EvidenceId::from_existing(format!("{EVIDENCE_PREFIX}page_{}", hash(path)))
}

fn source_evidence_id(identity: &str) -> EvidenceId {
    EvidenceId::from_existing(format!("{EVIDENCE_PREFIX}source_{}", hash(identity)))
}

fn citation_evidence_id(page_path: &str, identity: &str) -> EvidenceId {
    EvidenceId::from_existing(format!(
        "{EVIDENCE_PREFIX}cite_{}",
        hash(&format!("{page_path}\u{0}{identity}"))
    ))
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} {}", UNITS[0]);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

// ---------------------------------------------------------------------------
// Resolving `[[links]]` to nodes.
// ---------------------------------------------------------------------------

/// Resolves a wikilink target to the node it names.
///
/// Obsidian resolves `[[x]]` against filenames, and the vault's convention
/// is `kebab-case.md`, so the file stem is the primary key. Titles, paths
/// and case-folded spellings are accepted as fallbacks in that order, which
/// is what makes `[[Chain of Thought]]` find `chain-of-thought.md` without
/// letting a loose case-folded match beat an exact filename.
#[derive(Debug, Default)]
struct Resolver {
    pages: HashMap<String, NodeId>,
    sources: HashMap<String, NodeId>,
}

impl Resolver {
    fn build(snapshot: &VaultSnapshot) -> Self {
        let mut resolver = Resolver::default();

        // Tiers, most exact first. Within a tier the first page to claim a
        // key keeps it, and pages arrive in sorted path order, so the
        // outcome does not depend on filesystem ordering.
        for page in &snapshot.pages {
            let id = page_node_id(&page.path);
            resolver.claim(page.slug.clone(), &id);
            resolver.claim(page.path.trim_end_matches(".md").to_string(), &id);
            if let Some(rest) = page.path.strip_prefix("wiki/") {
                resolver.claim(rest.trim_end_matches(".md").to_string(), &id);
            }
        }
        for page in &snapshot.pages {
            resolver.claim(page.title.clone(), &page_node_id(&page.path));
        }
        for page in &snapshot.pages {
            let id = page_node_id(&page.path);
            for variant in fold(&page.slug) {
                resolver.claim(variant, &id);
            }
            for variant in fold(&page.title) {
                resolver.claim(variant, &id);
            }
        }

        for source in &snapshot.sources {
            let id = source_node_id(&source.identity);
            resolver
                .sources
                .entry(source.identity.to_ascii_lowercase())
                .or_insert(id);
        }

        resolver
    }

    fn claim(&mut self, key: String, id: &NodeId) {
        if key.is_empty() {
            return;
        }
        self.pages.entry(key).or_insert_with(|| id.clone());
    }

    fn page(&self, raw: &str) -> Option<&NodeId> {
        let target = raw
            .trim()
            .replace('\\', "/")
            .trim_start_matches("./")
            .trim_end_matches(".md")
            .trim()
            .to_string();
        if target.is_empty() {
            return None;
        }

        if let Some(id) = self.pages.get(&target) {
            return Some(id);
        }
        for variant in fold(&target) {
            if let Some(id) = self.pages.get(&variant) {
                return Some(id);
            }
        }
        // `[[wiki/concepts/x]]` written as a path when only the stem is
        // indexed, and the reverse.
        let stem = slug_of(&target);
        if stem != target {
            if let Some(id) = self.pages.get(&stem) {
                return Some(id);
            }
            for variant in fold(&stem) {
                if let Some(id) = self.pages.get(&variant) {
                    return Some(id);
                }
            }
        }
        None
    }

    fn source(&self, identity: &str) -> Option<&NodeId> {
        self.sources.get(&identity.to_ascii_lowercase())
    }
}

/// Case-folded spellings of a name, most exact first.
fn fold(value: &str) -> Vec<String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return Vec::new();
    }
    let hyphenated = lower.split_whitespace().collect::<Vec<_>>().join("-");
    let spaced = lower.replace(['-', '_'], " ");
    let spaced = spaced.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut variants = vec![lower];
    for variant in [hyphenated, spaced] {
        if !variants.contains(&variant) {
            variants.push(variant);
        }
    }
    variants
}

// ---------------------------------------------------------------------------
// What the vault says a page's edges and evidence should be.
// ---------------------------------------------------------------------------

/// One relationship the vault asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DesiredEdge {
    edge_type: EdgeType,
    /// Which part of the page asked for it, recorded as the edge's
    /// provenance detail so a later pass can tell its own edges apart.
    detail: &'static str,
}

/// Desired outgoing edges, keyed by target and edge type so the map itself
/// enforces "one relationship of one type between two nodes".
///
/// Keyed on the edge type's stored spelling rather than the enum, because
/// [`EdgeType`] deliberately has no `Ord`: [`Authority`] is the enum in this
/// vocabulary family whose ordering carries meaning, and it expresses that
/// through `rank()` rather than declaration order. Deriving `Ord` across the
/// family to satisfy a map key here would put a second, silently different
/// ordering next to it.
type DesiredEdges = BTreeMap<(NodeId, &'static str), DesiredEdge>;
type DesiredEvidence = BTreeMap<EvidenceId, Evidence>;

fn desired_for_page(
    page: &VaultPage,
    resolver: &Resolver,
    report: &mut VaultIndexReport,
    unresolved: &mut Vec<UnresolvedLink>,
) -> (DesiredEdges, DesiredEvidence) {
    let from = page_node_id(&page.path);
    let mut edges = DesiredEdges::new();
    let mut evidence = DesiredEvidence::new();

    // The page's own file, so "which knowledge touches this file?" answers
    // with the page itself as well as with everything citing it.
    let id = page_evidence_id(&page.path);
    evidence.insert(
        id.clone(),
        Evidence::new(from.clone(), EvidenceKind::Documentation)
            .with_id(id)
            .with_anchor(SourceAnchor {
                content_hash: Some(page.content_hash.clone()),
                ..SourceAnchor::file(&page.path)
            }),
    );

    // `related:` states a relationship; a body link merely refers to one.
    // Keeping them as different edge types preserves that distinction,
    // which is the whole reason edges are typed.
    for (targets, edge_type, detail) in [
        (&page.related, EdgeType::RelatedTo, EDGE_RELATED),
        (&page.links, EdgeType::References, EDGE_LINK),
    ] {
        for target in targets {
            match resolver.page(target) {
                Some(to) if *to != from => {
                    let key = (to.clone(), edge_type.as_str());
                    if edges
                        .insert(key, DesiredEdge { edge_type, detail })
                        .is_none()
                    {
                        report.links_resolved += 1;
                    }
                }
                // A page linking to itself is not a relationship.
                Some(_) => {}
                None => unresolved.push(UnresolvedLink {
                    from: page.path.clone(),
                    target: target.clone(),
                    kind: UnresolvedKind::Link,
                }),
            }
        }
    }

    for identity in &page.sources {
        let Some(to) = resolver.source(identity) else {
            // A `sources:` entry naming a file that is not in the vault is
            // a broken citation, reported the same way a broken link is.
            unresolved.push(UnresolvedLink {
                from: page.path.clone(),
                target: identity.clone(),
                kind: UnresolvedKind::Source,
            });
            continue;
        };
        edges.insert(
            (to.clone(), EdgeType::DerivedFrom.as_str()),
            DesiredEdge {
                edge_type: EdgeType::DerivedFrom,
                detail: EDGE_SOURCE,
            },
        );

        let id = citation_evidence_id(&page.path, identity);
        evidence.insert(
            id.clone(),
            Evidence::new(from.clone(), EvidenceKind::ExternalSource)
                .with_id(id)
                .with_anchor(SourceAnchor::file(format!(
                    "{}/{identity}",
                    super::scan::RAW_SOURCES_DIR
                )))
                .with_note(format!("Cited by {} as a source.", page.path)),
        );
    }

    (edges, evidence)
}

fn desired_for_source(source: &VaultSource) -> DesiredEvidence {
    let node = source_node_id(&source.identity);
    let id = source_evidence_id(&source.identity);
    let mut evidence = DesiredEvidence::new();
    evidence.insert(
        id.clone(),
        Evidence::new(node, EvidenceKind::ExternalSource)
            .with_id(id)
            .with_anchor(SourceAnchor::file(&source.path)),
    );
    evidence
}

// ---------------------------------------------------------------------------
// Reconciliation: write the difference, and only the difference.
// ---------------------------------------------------------------------------

fn ensure_node(
    store: &mut KnowledgeStore,
    desired: KnowledgeNode,
    report: &mut VaultIndexReport,
) -> Result<()> {
    match store.find_node(&desired.id)? {
        None => {
            store.create_node(desired)?;
            report.nodes_created += 1;
        }
        Some(existing) if node_changed(&existing, &desired) => {
            let mut updated = desired;
            updated.created_at = existing.created_at;
            updated.version = existing.version;
            // The vault changed, so the knowledge changed; but the *actor*
            // is this deterministic pass, not whoever wrote the page.
            store.update_node_attributed(
                updated,
                Some("vault re-index".to_string()),
                Provenance::deterministic(PAGE_PROVENANCE),
            )?;
            report.nodes_updated += 1;
        }
        Some(_) => {}
    }
    Ok(())
}

/// Compare the fields the vault dictates. Timestamps and version are
/// bookkeeping the vault says nothing about, so they never count as a
/// change — otherwise every pass would rewrite every node.
fn node_changed(existing: &KnowledgeNode, desired: &KnowledgeNode) -> bool {
    existing.node_type != desired.node_type
        || existing.title != desired.title
        || existing.summary != desired.summary
        || existing.content != desired.content
        || existing.scope != desired.scope
        || existing.authority != desired.authority
        || existing.status != desired.status
        || existing.provenance != desired.provenance
}

/// Delete the nodes for pages and sources that are no longer in the vault.
///
/// Deletion, not a stale marking: the index is a function of the vault, and
/// a page that is gone has no node. Anything worth keeping was in the
/// markdown, which is the layer that keeps things.
fn remove_departed(
    store: &mut KnowledgeStore,
    owned: &HashSet<NodeId>,
    report: &mut VaultIndexReport,
) -> Result<()> {
    let departed = store
        .list_nodes(&NodeFilter::all())?
        .into_iter()
        .filter(|node| is_vault_owned(node) && !owned.contains(&node.id))
        .map(|node| node.id)
        .collect::<Vec<_>>();
    for node in departed {
        store.delete_node(&node)?;
        report.nodes_removed += 1;
    }
    Ok(())
}

fn is_vault_owned(node: &KnowledgeNode) -> bool {
    node.provenance.source == ProvenanceSource::Deterministic
        && node.provenance.detail.as_deref().is_some_and(|detail| {
            detail.starts_with(PAGE_PROVENANCE) || detail.starts_with(SOURCE_PROVENANCE)
        })
}

fn reconcile_edges(
    store: &mut KnowledgeStore,
    from: &NodeId,
    desired: &DesiredEdges,
    report: &mut VaultIndexReport,
) -> Result<()> {
    let obsolete = store
        .edges_of(from)?
        .into_iter()
        .filter(|edge| {
            edge.from == *from
                && is_vault_edge(edge)
                && !desired.contains_key(&(edge.to.clone(), edge.edge_type.as_str()))
        })
        .map(|edge| edge.id)
        .collect::<Vec<_>>();
    for edge in obsolete {
        store.remove_edge(&edge)?;
        report.edges_removed += 1;
    }

    for ((to, _), edge) in desired {
        if store.find_edge_between(from, to, edge.edge_type)?.is_none() {
            store.connect(KnowledgeEdge::deterministic(
                from.clone(),
                edge.edge_type,
                to.clone(),
                edge.detail,
            ))?;
            report.edges_created += 1;
        }
    }
    Ok(())
}

fn is_vault_edge(edge: &KnowledgeEdge) -> bool {
    edge.provenance.source == ProvenanceSource::Deterministic
        && edge
            .provenance
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with(EDGE_PREFIX))
}

fn reconcile_evidence(
    store: &mut KnowledgeStore,
    node: &NodeId,
    desired: &DesiredEvidence,
    report: &mut VaultIndexReport,
) -> Result<()> {
    let existing = store.evidence_for(node)?;

    for evidence in &existing {
        if !evidence.id.as_str().starts_with(EVIDENCE_PREFIX) {
            continue;
        }
        match desired.get(&evidence.id) {
            None => {
                store.remove_evidence(&evidence.id)?;
                report.evidence_removed += 1;
            }
            Some(want) if want.anchor != evidence.anchor => {
                if let Some(anchor) = want.anchor.clone() {
                    store.update_evidence_anchor(&evidence.id, anchor)?;
                    report.evidence_updated += 1;
                }
            }
            Some(_) => {}
        }
    }

    let present = existing
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<HashSet<_>>();
    for (id, evidence) in desired {
        if !present.contains(id) {
            store.attach_evidence_attributed(
                evidence.clone(),
                Provenance::deterministic(PAGE_PROVENANCE),
            )?;
            report.evidence_attached += 1;
        }
    }
    Ok(())
}
