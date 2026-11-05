//! The memory lifecycle: proposal, acceptance, rejection.
//!
//! A model proposes knowledge through the bridge, and a proposal is a node in `knowledge.db` and
//! nothing more. It is not durable: delete the database and it is gone, which is the right thing
//! for something no person has looked at. **Accepting** is the act that makes it permanent, and it
//! does so by writing the node as a page of the vault, so that it is text, syncs, exports, and
//! survives losing every index. The node keeps its id, which is declared in the page, so its edges,
//! evidence and utility history carry over to the page that now backs it.
//!
//! Accepting is not verifying. The page records the authority the node already had, and the
//! authority of a model's proposal is `unverified_observation`; raising it is `verify`'s job and
//! needs evidence.
//!
//! Conversation transcripts never come through here. Nothing is proposed by the act of talking.

use std::collections::HashSet;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::lock::RepoLock;
use super::{portable_error, write_atomic};
use crate::error::{Error, Result};
use crate::id::NodeId;
use crate::journal::{EventKind, NewEvent};
use crate::model::{
    EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, NodeStatus, ProvenanceSource,
};
use crate::store::{KnowledgeStore, NodeFilter};
use crate::vault::{
    is_valid_declared_id, is_vault_owned, set_fields, DeclaredEvidence, FieldValue, PageRef,
    VaultIndexer, VaultPage, VaultScanOptions, VaultSync,
};

/// What accepting a node produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub node: KnowledgeNode,
    pub page: PageRef,
    /// True when the node was already a page of the vault and nothing was written.
    pub already_accepted: bool,
    /// Relationships written into the page, so they travel with it.
    pub relationships_written: usize,
    /// Relationships that stay local because their other end is not a page yet.
    pub relationships_left_local: Vec<String>,
}

/// Model proposals nobody has accepted or rejected.
///
/// A proposal is a live node whose provenance is a model and that the vault does not own.
pub fn proposals(store: &KnowledgeStore, limit: Option<u32>) -> Result<Vec<KnowledgeNode>> {
    let mut found: Vec<KnowledgeNode> = store
        .list_nodes(&NodeFilter::live())?
        .into_iter()
        .filter(is_proposal)
        .collect();
    if let Some(limit) = limit {
        found.truncate(limit as usize);
    }
    Ok(found)
}

/// True for a node a model proposed and nothing has yet decided.
pub fn is_proposal(node: &KnowledgeNode) -> bool {
    node.provenance.source == ProvenanceSource::Model && !is_vault_owned(node) && node.is_live()
}

/// Accept a model's proposal: write it to the vault as a page.
pub fn accept(store: &mut KnowledgeStore, id: &NodeId, accepted_by: &str) -> Result<Accepted> {
    accept_impl(store, id, accepted_by, true)
}

pub(crate) fn accept_deferred(
    store: &mut KnowledgeStore,
    id: &NodeId,
    accepted_by: &str,
) -> Result<Accepted> {
    accept_impl(store, id, accepted_by, false)
}

fn accept_impl(
    store: &mut KnowledgeStore,
    id: &NodeId,
    accepted_by: &str,
    reconcile: bool,
) -> Result<Accepted> {
    let node = store.get_node(id)?;
    if is_vault_owned(&node) {
        return already_accepted(store, node);
    }
    if node.provenance.source != ProvenanceSource::Model {
        return Err(Error::Invalid {
            id: node.id,
            reason: "it is not a model proposal; user-stated knowledge is written to the vault \
                     with `vault materialize`"
                .into(),
        });
    }
    if node.status == NodeStatus::Archived {
        return Err(Error::Invalid {
            id: node.id,
            reason: "it was rejected; propose it again if it should be reconsidered".into(),
        });
    }
    write_node_as_page_impl(store, node, accepted_by, "agent-proposal", reconcile)
}

/// Write a node a person stated to the vault as a page. A no-op for anything the vault owns.
pub fn materialize_stated(store: &mut KnowledgeStore, id: &NodeId) -> Result<Option<Accepted>> {
    let node = store.get_node(id)?;
    if is_vault_owned(&node) || node.provenance.source != ProvenanceSource::User {
        return Ok(None);
    }
    write_node_as_page(store, node, "user", "user-stated").map(Some)
}

/// Reject a proposal so it is not offered again. Touches no file: a proposal was never durable.
pub fn reject(
    store: &mut KnowledgeStore,
    id: &NodeId,
    reason: Option<&str>,
    rejected_by: &str,
) -> Result<KnowledgeNode> {
    let node = store.get_node(id)?;
    if is_vault_owned(&node) {
        return Err(Error::Invalid {
            id: node.id,
            reason: "it is already a page of the vault; delete or edit the page instead".into(),
        });
    }
    if node.provenance.source != ProvenanceSource::Model {
        return Err(Error::Invalid {
            id: node.id,
            reason: "it is not a model proposal".into(),
        });
    }
    if node.status == NodeStatus::Archived {
        return Ok(node);
    }
    let node = store.set_node_status(id, NodeStatus::Archived)?;
    store.record(
        NewEvent::about(EventKind::NodeRejected, &node.id)
            .with_detail(serde_json::json!({ "reason": reason, "by": rejected_by })),
    )?;
    Ok(node)
}

/// One node `materialize` wrote, or would write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Materialized {
    pub node: NodeId,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftAlone {
    pub node: NodeId,
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeReport {
    pub dry_run: bool,
    pub written: Vec<Materialized>,
    pub left_alone: Vec<LeftAlone>,
}

/// Write pages for existing nodes that are user-stated or verified and not yet vault-owned.
///
/// This is the migration for a database that predates "accepted knowledge is a page". It defaults
/// to a dry run in the CLI, never deletes a row, never overwrites an existing file (a name
/// collision gets a numeric suffix), and reports every node it left alone and why. Model proposals
/// nobody verified stay proposals.
pub fn materialize(store: &mut KnowledgeStore, apply: bool) -> Result<MaterializeReport> {
    let mut report = MaterializeReport {
        dry_run: !apply,
        ..Default::default()
    };
    let project = require_project(store)?;
    let mut taken: HashSet<String> = HashSet::new();

    let nodes = store.list_nodes(&NodeFilter::all())?;
    for node in nodes {
        let reason =
            if is_vault_owned(&node) || node.provenance.source == ProvenanceSource::Deterministic {
                continue;
            } else if !node.is_live() {
                Some("it is not live knowledge")
            } else if node.provenance.source == ProvenanceSource::Model
                && node.last_verified_at.is_none()
            {
                Some("it is a model proposal nobody has verified or accepted")
            } else if !is_valid_declared_id(node.id.as_str()) {
                Some("its id cannot be declared in a page")
            } else {
                None
            };
        if let Some(reason) = reason {
            report.left_alone.push(LeftAlone {
                node: node.id,
                title: node.title,
                reason: reason.into(),
            });
            continue;
        }

        if apply {
            let origin = origin_of(&node);
            let accepted = write_node_as_page(store, node.clone(), "materialize", origin)?;
            report.written.push(Materialized {
                node: node.id,
                title: node.title,
                path: accepted.page.path,
            });
        } else {
            let path = unique_page_path(&project, &node, &taken);
            taken.insert(path.clone());
            report.written.push(Materialized {
                node: node.id,
                title: node.title,
                path,
            });
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------

fn already_accepted(store: &KnowledgeStore, node: KnowledgeNode) -> Result<Accepted> {
    let evidence = store.evidence_for(&node.id)?;
    let project = store.project_root();
    let page = PageRef::of(&node, &evidence, project.as_deref()).ok_or_else(|| Error::Invalid {
        id: node.id.clone(),
        reason: "it is owned by the vault but its page is not indexed yet; run `index-vault`"
            .into(),
    })?;
    Ok(Accepted {
        node,
        page,
        already_accepted: true,
        relationships_written: 0,
        relationships_left_local: Vec::new(),
    })
}

fn require_project(store: &KnowledgeStore) -> Result<std::path::PathBuf> {
    store.project_root().ok_or_else(|| {
        portable_error(
            "invalid_request",
            "this knowledge base is not in a project, so there is no vault to write a page to",
        )
    })
}

fn origin_of(node: &KnowledgeNode) -> &'static str {
    match node.provenance.source {
        ProvenanceSource::User => "user-stated",
        _ => "agent-proposal",
    }
}

fn write_node_as_page(
    store: &mut KnowledgeStore,
    node: KnowledgeNode,
    accepted_by: &str,
    origin: &str,
) -> Result<Accepted> {
    write_node_as_page_impl(store, node, accepted_by, origin, true)
}

fn write_node_as_page_impl(
    store: &mut KnowledgeStore,
    node: KnowledgeNode,
    accepted_by: &str,
    origin: &str,
    reconcile: bool,
) -> Result<Accepted> {
    if !is_valid_declared_id(node.id.as_str()) {
        return Err(Error::Invalid {
            id: node.id,
            reason: "its id cannot be declared in a page (letters, digits, `.`, `_`, `-`, and not \
                     starting with `vault_`)"
                .into(),
        });
    }
    let project = require_project(store)?;
    let _lock = RepoLock::acquire(store.root(), "vault")?;

    // Re-read under the lock: another process may have accepted it while this one waited.
    let node = store.get_node(&node.id)?;
    if is_vault_owned(&node) {
        return already_accepted(store, node);
    }

    let path = unique_page_path(&project, &node, &HashSet::new());
    let (related, relations, left_local, edges) = relationships_for(store, &node)?;
    let evidence = declared_evidence(store, &node)?;
    let content = render_page(&node, accepted_by, origin, &related, &relations, &evidence);

    write_atomic(&project.join(&path), content.as_bytes())?;

    let page = VaultPage::parse(&path, &content);
    VaultIndexer::adopt_page(store, &page)?;
    // The page now states these relationships, so they stop being a model's guesses.
    for edge in &edges {
        if !edge.is_traversable() {
            store.promote_edge(&edge.id)?;
        }
    }
    store.record(
        NewEvent::about(EventKind::NodeAccepted, &node.id)
            .with_detail(serde_json::json!({ "path": path, "by": accepted_by, "origin": origin })),
    )?;

    // Bring the edges and evidence the page declares into the index now rather than at the next
    // read. Accepting is a write the person asked for, so unlike a read it may build the first
    // index: without one the page could not say which file backs the node.
    if reconcile {
        match VaultIndexer::sync_if_changed(store, &project, VaultScanOptions::default())? {
            VaultSync::NotIndexed => {
                VaultIndexer::scan_and_index(store, &project, VaultScanOptions::default())?;
            }
            VaultSync::Unchanged | VaultSync::Reconciled(_) => {}
        }
    }

    let node = store.get_node(&node.id)?;
    let evidence = store.evidence_for(&node.id)?;
    let page = PageRef::of(&node, &evidence, Some(&project)).unwrap_or(PageRef {
        absolute_path: Some(project.join(&path).to_string_lossy().into_owned()),
        content_hash: page.content_hash.clone(),
        path: path.clone(),
    });
    Ok(Accepted {
        node,
        page,
        already_accepted: false,
        relationships_written: related.len() + relations.len(),
        relationships_left_local: left_local,
    })
}

type Rels = (Vec<String>, Vec<String>, Vec<String>, Vec<KnowledgeEdge>);

/// Disconnect a relationship in both the derived index and its owning Markdown.
pub fn unlink(store: &mut KnowledgeStore, id: &crate::id::EdgeId) -> Result<()> {
    let edge = store.get_edge(id)?;
    let from = store.get_node(&edge.from)?;
    let Some(project) = store.project_root() else {
        return store.remove_edge(id);
    };
    let Some(page) = PageRef::of(&from, &store.evidence_for(&from.id)?, Some(&project)) else {
        return store.remove_edge(id);
    };
    let _lock = RepoLock::acquire(store.root(), "vault")?;
    crate::vault::unlink_page_edge(store, &project, &edge, &page)
}

/// Explicit page-to-page links are canonical Markdown, including links made by
/// connected agents. Re-indexing, export and sync must all retain them.
pub fn link_pages(store: &mut KnowledgeStore, edge: KnowledgeEdge) -> Result<KnowledgeEdge> {
    if edge.from == edge.to {
        return Err(portable_error(
            "invalid_request",
            "A saved page cannot link to itself",
        ));
    }
    let project = require_project(store)?;
    let _lock = RepoLock::acquire(store.root(), "vault")?;
    let from = store.get_node(&edge.from)?;
    let to = store.get_node(&edge.to)?;
    let source = PageRef::of(&from, &store.evidence_for(&from.id)?, Some(&project))
        .ok_or_else(|| portable_error("invalid_request", "The source has no saved page"))?;
    let target = PageRef::of(&to, &store.evidence_for(&to.id)?, Some(&project))
        .ok_or_else(|| portable_error("invalid_request", "The target has no saved page"))?;
    let path = project.join(&source.path);
    let content = std::fs::read_to_string(&path)?;
    let page = VaultPage::parse(&source.path, &content);
    let mut relations = page
        .relations
        .iter()
        .map(|(kind, target)| format!("{} [[{}]]", kind.as_str(), target))
        .collect::<Vec<_>>();
    let reference = format!(
        "{} [[{}]]",
        edge.edge_type.as_str(),
        target.path.trim_end_matches(".md")
    );
    if !relations.contains(&reference) {
        relations.push(reference);
    }
    let updated = set_fields(&content, &[("relations", FieldValue::List(relations))]);
    write_atomic(&path, updated.as_bytes())?;
    VaultIndexer::scan_and_index(store, &project, VaultScanOptions::default())?;
    store
        .edges_of(&from.id)?
        .into_iter()
        .find(|found| {
            found.from == edge.from && found.to == edge.to && found.edge_type == edge.edge_type
        })
        .ok_or_else(|| portable_error("invalid_request", "The page link could not be indexed"))
}

/// The relationships a node has that can be written into its page: outgoing edges to nodes that
/// are themselves pages. Returns `related:` entries, typed `relations:` entries, descriptions of
/// the edges that must stay local, and the edges that were written.
fn relationships_for(store: &KnowledgeStore, node: &KnowledgeNode) -> Result<Rels> {
    let (mut related, mut relations, mut left, mut written) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let project = store.project_root();
    for edge in store.edges_of(&node.id)? {
        if edge.from != node.id {
            continue;
        }
        let is_vault_edge = edge
            .provenance
            .detail
            .as_deref()
            .is_some_and(|d| d.starts_with("vault_index:"));
        if is_vault_edge {
            continue;
        }
        let target = store.get_node(&edge.to)?;
        let target_evidence = store.evidence_for(&target.id)?;
        let reference =
            if let Some(page) = PageRef::of(&target, &target_evidence, project.as_deref()) {
                format!("[[{}]]", page.path.trim_end_matches(".md"))
            } else if (is_proposal(&target) || is_vault_owned(&target))
                && is_valid_declared_id(target.id.as_str())
            {
                // A declared id resolves as soon as the target becomes a page. Keep the
                // reference in Markdown now, regardless of acceptance order.
                format!("[[{}]]", target.id)
            } else {
                left.push(format!("{} {}", edge.edge_type.as_str(), target.title));
                continue;
            };
        if edge.edge_type == EdgeType::RelatedTo {
            related.push(reference);
        } else {
            relations.push(format!("{} {reference}", edge.edge_type.as_str()));
        }
        written.push(edge);
    }
    related.sort();
    related.dedup();
    relations.sort();
    relations.dedup();
    Ok((related, relations, left, written))
}

fn declared_evidence(store: &KnowledgeStore, node: &KnowledgeNode) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for evidence in store.evidence_for(&node.id)? {
        if evidence.id.as_str().starts_with("ev_vault_") {
            continue;
        }
        out.push(evidence_json(&evidence));
        if out.len() >= 50 {
            break;
        }
    }
    Ok(out)
}

fn evidence_json(evidence: &Evidence) -> String {
    let declared = DeclaredEvidence {
        kind: evidence.kind,
        file: evidence.anchor.as_ref().map(|a| a.file.clone()),
        symbol: evidence.anchor.as_ref().and_then(|a| a.symbol.clone()),
        excerpt: evidence
            .excerpt
            .as_ref()
            .map(|e| e.chars().take(2000).collect()),
        note: evidence.note.clone(),
    };
    serde_json::to_string(&declared).unwrap_or_default()
}

fn render_page(
    node: &KnowledgeNode,
    accepted_by: &str,
    origin: &str,
    related: &[String],
    relations: &[String],
    evidence: &[String],
) -> String {
    use FieldValue::{List, Scalar};
    let mut fields: Vec<(&str, FieldValue)> = vec![
        ("id", Scalar(node.id.as_str().into())),
        ("type", Scalar(node.node_type.as_str().into())),
        ("title", Scalar(node.title.clone())),
    ];
    if !node.summary.trim().is_empty() {
        fields.push(("summary", Scalar(node.summary.trim().into())));
    }
    fields.push(("authority", Scalar(node.authority.as_str().into())));
    fields.push(("scope", Scalar(node.scope.as_str().into())));
    fields.push(("origin", Scalar(origin.into())));
    if let Some(detail) = node
        .provenance
        .detail
        .as_deref()
        .filter(|_| node.provenance.source == ProvenanceSource::Model)
    {
        fields.push(("proposed_by", Scalar(detail.into())));
    }
    if let Some(session) = &node.provenance.session {
        fields.push(("session", Scalar(session.as_str().into())));
    }
    fields.push((
        "accepted",
        Scalar(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
    ));
    fields.push(("accepted_by", Scalar(accepted_by.into())));
    if !related.is_empty() {
        fields.push(("related", List(related.to_vec())));
    }
    if !relations.is_empty() {
        fields.push(("relations", List(relations.to_vec())));
    }
    if !evidence.is_empty() {
        fields.push(("evidence", List(evidence.to_vec())));
    }

    let body = if node.content.trim().is_empty() {
        node.summary.trim()
    } else {
        node.content.trim()
    };
    let mut page = set_fields("", &fields);
    page.push('\n');
    page.push_str(body);
    page.push('\n');
    page
}

/// `wiki/<types>/<title-slug>.md`, made unique against the vault and against `taken`.
fn unique_page_path(project: &Path, node: &KnowledgeNode, taken: &HashSet<String>) -> String {
    let directory = plural(node.node_type.as_str());
    let mut slug = slugify(&node.title);
    if slug.is_empty() {
        slug = slugify(node.id.as_str());
    }
    let mut candidate = format!("wiki/{directory}/{slug}.md");
    let mut suffix = 2;
    while taken.contains(&candidate) || project.join(&candidate).exists() {
        candidate = format!("wiki/{directory}/{slug}-{suffix}.md");
        suffix += 1;
    }
    candidate
}

fn plural(word: &str) -> String {
    match word.strip_suffix('y') {
        Some(stem) => format!("{stem}ies"),
        None => format!("{word}s"),
    }
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
        if slug.len() >= 60 {
            break;
        }
    }
    slug
}

// ---------------------------------------------------------------------------
// Drafting from a source
// ---------------------------------------------------------------------------

/// The most of a source file a draft will read. A draft quotes one line; this bounds the I/O.
const DRAFT_SOURCE_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Who a deterministic draft is attributed to. A draft is a proposal like any other: it waits for a
/// person, and `proposals` lists it.
pub const DRAFT_PROPOSER: &str = "athenaeum-draft-source";

/// What drafting a proposal from a source produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drafted {
    pub node: KnowledgeNode,
    pub evidence: Evidence,
}

/// Propose an observation drawn from one file under `raw/sources/`, without a model.
///
/// The draft quotes the source's first non-empty line and cites the file with its content hash, so
/// a later edit to the source makes the evidence stale and visible. It is a proposal: nothing is
/// written to the vault until it is accepted.
pub fn draft_from_source(store: &mut KnowledgeStore, source: &str, title: &str) -> Result<Drafted> {
    use crate::model::{NodeType, Scope, SourceAnchor};
    use crate::wiki::{WikiCaller, WikiEvidenceCreate, WikiNodeCreate, WikiService};

    let project = require_project(store)?;
    let source = source.trim().replace('\\', "/");
    if !source.starts_with("raw/sources/") || super::syncset::validate_path(&source).is_err() {
        return Err(portable_error(
            "invalid_request",
            format!("{source:?} is not a vault-relative path under raw/sources/"),
        ));
    }
    // Refuse a symlink anywhere on the way, so a link inside the vault cannot pull in a file from
    // outside it.
    let mut walked = project.clone();
    for segment in source.split('/') {
        walked.push(segment);
        let metadata = std::fs::symlink_metadata(&walked).map_err(|_| Error::Invalid {
            id: NodeId::from_existing(&source),
            reason: "the source does not exist".into(),
        })?;
        if metadata.file_type().is_symlink() {
            return Err(portable_error(
                "invalid_request",
                format!("{source} passes through a symbolic link, which a draft will not follow"),
            ));
        }
    }
    let metadata = std::fs::metadata(&walked)?;
    if !metadata.is_file() || metadata.len() > DRAFT_SOURCE_MAX_BYTES {
        return Err(portable_error(
            "invalid_request",
            format!(
                "{source} is not a regular text file of at most {DRAFT_SOURCE_MAX_BYTES} bytes"
            ),
        ));
    }
    let bytes = std::fs::read(&walked)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        portable_error(
            "invalid_request",
            "the source is not UTF-8 text; extract it to text first",
        )
    })?;
    let excerpt = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");

    let caller = WikiCaller {
        model: DRAFT_PROPOSER.into(),
        session: None,
    };
    let mut service = WikiService::new(store);
    let node = service.create(
        WikiNodeCreate {
            node_type: NodeType::Observation,
            title: title.trim().into(),
            summary: excerpt.chars().take(300).collect(),
            content: format!(
                "# {}\n\nSource: `{source}`\n\n> {}\n",
                title.trim(),
                excerpt.chars().take(500).collect::<String>()
            ),
            scope: Scope::default(),
        },
        caller.clone(),
    )?;
    let evidence = service.attach_evidence(
        WikiEvidenceCreate {
            node: node.id.as_str().into(),
            kind: EvidenceKind::SourceFile,
            anchor: Some(SourceAnchor::file(source.clone()).fingerprint(excerpt, &text)),
            excerpt: Some(excerpt.chars().take(500).collect()),
            note: None,
        },
        caller,
    )?;
    Ok(Drafted { node, evidence })
}

// ---------------------------------------------------------------------------
// Leaving the retired `athenaeum-core` index
// ---------------------------------------------------------------------------

/// The index the retired second engine kept inside `.athenaeum/`.
pub const LEGACY_CORE_DATABASE: &str = "local-core.db";

/// One proposal carried over from the retired index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImported {
    pub legacy_id: String,
    pub node: Option<NodeId>,
    pub title: String,
    pub evidence: usize,
    pub relationships: usize,
}

/// What importing the retired index did, or in a dry run would do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportReport {
    /// Whether a retired index exists in this workspace at all.
    pub found: bool,
    pub dry_run: bool,
    pub imported: Vec<LegacyImported>,
    pub left_alone: Vec<String>,
}

/// Carry the pending proposals of a retired `athenaeum-core` index into this knowledge base.
///
/// Everything the old engine had *accepted* is already Markdown in the vault and needs nothing: the
/// index here is rebuilt from it. What the old index alone holds is the proposals nobody has
/// reviewed yet, with their evidence and relationships, and that is all this moves. The old
/// database is opened read-only and is never changed or deleted. Running it twice is safe: a
/// proposal whose title is already known is reported and skipped rather than duplicated.
pub fn import_legacy_core(store: &mut KnowledgeStore, apply: bool) -> Result<LegacyImportReport> {
    use crate::model::{NodeType, Scope, SourceAnchor};
    use crate::wiki::{
        WikiCaller, WikiEvidenceCreate, WikiLinkCreate, WikiNodeCreate, WikiService,
    };
    use rusqlite::{Connection, OpenFlags};
    use std::collections::HashMap;

    let project = require_project(store)?;
    let path = project.join(".athenaeum").join(LEGACY_CORE_DATABASE);
    let mut report = LegacyImportReport {
        dry_run: !apply,
        ..Default::default()
    };
    if !path.is_file() {
        return Ok(report);
    }
    report.found = true;
    let old =
        Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| {
            portable_error("io", format!("cannot open {}: {error}", path.display()))
        })?;

    let bodies = |sql: &str| -> Vec<(String, serde_json::Value)> {
        // A table the old engine never created is simply empty.
        let Ok(mut statement) = old.prepare(sql) else {
            return Vec::new();
        };
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });
        let Ok(rows) = rows else { return Vec::new() };
        rows.filter_map(|row| row.ok())
            .filter_map(|(state, body)| {
                serde_json::from_str(&body).ok().map(|value| (state, value))
            })
            .collect()
    };
    let text = |value: &serde_json::Value, key: &str| -> String {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    let proposals = bodies("SELECT state, body FROM proposals ORDER BY created_at, id");
    let evidence = bodies("SELECT 'evidence', body FROM proposal_evidence ORDER BY id");
    let edges = bodies("SELECT 'edge', body FROM proposal_edges ORDER BY id");

    let mut mapped: HashMap<String, NodeId> = HashMap::new();
    let mut staged: Vec<(String, String, usize, usize)> = Vec::new();
    for (state, node) in &proposals {
        let legacy_id = text(node, "id");
        let title = text(node, "title");
        if state != "pending" {
            report.left_alone.push(format!(
                "{legacy_id} ({title}): {state} in the old index; accepted knowledge is already in the vault, rejected knowledge stays rejected"
            ));
            continue;
        }
        let Some(node_type) = NodeType::parse_opt(&text(node, "type")) else {
            report.left_alone.push(format!(
                "{legacy_id} ({title}): unknown type {:?}",
                text(node, "type")
            ));
            continue;
        };
        let scope = Scope::parse_opt(&text(node, "scope")).unwrap_or_default();
        let provenance = text(node, "provenance");
        let proposer = provenance
            .strip_prefix("model:")
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "athenaeum-core-import".to_string());
        let own_evidence = evidence
            .iter()
            .filter(|(_, e)| text(e, "node") == legacy_id)
            .count();
        let own_edges = edges
            .iter()
            .filter(|(_, e)| text(e, "from") == legacy_id || text(e, "to") == legacy_id)
            .count();

        if !apply {
            staged.push((legacy_id, title, own_evidence, own_edges));
            continue;
        }
        let created = WikiService::new(store).create(
            WikiNodeCreate {
                node_type,
                title: title.clone(),
                summary: text(node, "summary"),
                content: text(node, "content"),
                scope,
            },
            WikiCaller {
                model: proposer.clone(),
                session: None,
            },
        );
        match created {
            Ok(created) => {
                mapped.insert(legacy_id.clone(), created.id.clone());
                staged.push((legacy_id, title, own_evidence, own_edges));
            }
            Err(error) => report
                .left_alone
                .push(format!("{legacy_id} ({title}): {error}")),
        }
    }

    if apply {
        for (_, item) in &evidence {
            let Some(new_id) = mapped.get(&text(item, "node")) else {
                continue;
            };
            let file = item
                .get("anchor")
                .and_then(|anchor| anchor.get("file"))
                .and_then(|file| file.as_str())
                .map(str::to_string)
                .filter(|file| !file.is_empty())
                .or_else(|| Some(text(item, "path")).filter(|file| !file.is_empty()));
            let anchor = file.map(|file| {
                let mut anchor = SourceAnchor::file(file);
                anchor.content_hash = item
                    .get("sha256")
                    .and_then(|h| h.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        item.get("anchor")
                            .and_then(|a| a.get("contentHash"))
                            .and_then(|h| h.as_str())
                            .map(str::to_string)
                    });
                anchor
            });
            let result = WikiService::new(store).attach_evidence(
                WikiEvidenceCreate {
                    node: new_id.as_str().to_string(),
                    kind: EvidenceKind::parse_opt(&text(item, "kind")).unwrap_or_default(),
                    anchor,
                    excerpt: item
                        .get("excerpt")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    note: item
                        .get("note")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                },
                WikiCaller {
                    model: "athenaeum-core-import".into(),
                    session: None,
                },
            );
            if let Err(error) = result {
                report
                    .left_alone
                    .push(format!("evidence {}: {error}", text(item, "id")));
            }
        }
        for (_, edge) in &edges {
            let (from, to) = (text(edge, "from"), text(edge, "to"));
            // An endpoint is either another imported proposal or knowledge the new index already
            // holds under the same id (a page that declared its id).
            let resolve = |old: &str| {
                mapped
                    .get(old)
                    .map(|id| id.as_str().to_string())
                    .or_else(|| {
                        store
                            .find_node(&NodeId::from_existing(old))
                            .ok()
                            .flatten()
                            .map(|node| node.id.as_str().to_string())
                    })
            };
            let (Some(from_id), Some(to_id)) = (resolve(&from), resolve(&to)) else {
                report.left_alone.push(format!(
                    "relationship {from} → {to}: an end is not known to this index"
                ));
                continue;
            };
            let Some(edge_type) = EdgeType::parse_opt(&text(edge, "type")) else {
                report.left_alone.push(format!(
                    "relationship {from} → {to}: unknown type {:?}",
                    text(edge, "type")
                ));
                continue;
            };
            let result = WikiService::new(store).link(
                WikiLinkCreate {
                    from: from_id,
                    edge_type,
                    to: to_id,
                    note: None,
                    confidence: None,
                },
                WikiCaller {
                    model: "athenaeum-core-import".into(),
                    session: None,
                },
            );
            if let Err(error) = result {
                report
                    .left_alone
                    .push(format!("relationship {from} → {to}: {error}"));
            }
        }
    }

    for (legacy_id, title, evidence, relationships) in staged {
        report.imported.push(LegacyImported {
            node: mapped.get(&legacy_id).cloned(),
            legacy_id,
            title,
            evidence,
            relationships,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Authority, NodeType, Provenance};
    use crate::store::EdgeFilter;
    use crate::vault::VaultIndexer;

    struct Project {
        _dir: tempfile::TempDir,
        root: std::path::PathBuf,
        store: KnowledgeStore,
    }

    fn project() -> Project {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let workspace = crate::workspace::Workspace::init(&root).unwrap();
        let store = workspace.open().unwrap();
        Project {
            _dir: dir,
            root,
            store,
        }
    }

    fn propose(p: &mut Project, title: &str) -> KnowledgeNode {
        p.store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, title)
                    .with_summary("Retries are capped at three.")
                    .with_content("Retries are capped at three because the queue backs off.")
                    .with_provenance(Provenance::model("curator").with_session("sess_1".into())),
            )
            .unwrap()
    }

    #[test]
    fn an_accepted_proposal_survives_deleting_the_database() {
        let mut p = project();
        let node = propose(&mut p, "Retry budget is three");
        assert_eq!(proposals(&p.store, None).unwrap().len(), 1);

        let accepted = accept(&mut p.store, &node.id, "user").unwrap();
        assert!(accepted
            .page
            .path
            .starts_with("wiki/decisions/retry-budget-is-three"));
        assert!(proposals(&p.store, None).unwrap().is_empty());

        // The text is on disk with the node's id declared and its authority unchanged.
        let text = std::fs::read_to_string(p.root.join(&accepted.page.path)).unwrap();
        assert!(text.contains(&format!("id: {}", node.id)), "{text}");
        assert!(text.contains("authority: unverified_observation"), "{text}");
        assert!(text.contains("origin: agent-proposal"), "{text}");

        // Delete the index entirely and rebuild it from the vault.
        drop(p.store);
        std::fs::remove_file(p.root.join(".athenaeum/knowledge.db")).unwrap();
        let mut rebuilt = crate::workspace::Workspace::at(p.root.join(".athenaeum"))
            .open()
            .unwrap();
        VaultIndexer::scan_and_index(&mut rebuilt, &p.root, VaultScanOptions::default()).unwrap();
        let back = rebuilt.get_node(&node.id).unwrap();
        assert_eq!(back.title, "Retry budget is three");
        assert_eq!(back.authority, Authority::UnverifiedObservation);
        assert_eq!(back.node_type, NodeType::Decision);
    }

    #[test]
    fn a_rejected_proposal_is_not_offered_again_and_writes_nothing() {
        let mut p = project();
        let node = propose(&mut p, "Probably nothing");
        reject(&mut p.store, &node.id, Some("noise"), "user").unwrap();
        assert!(proposals(&p.store, None).unwrap().is_empty());
        assert!(!p.root.join("wiki").exists());
        let error = accept(&mut p.store, &node.id, "user").unwrap_err();
        assert!(error.to_string().contains("rejected"), "{error}");
    }

    #[test]
    fn accepting_twice_writes_one_page() {
        let mut p = project();
        let node = propose(&mut p, "Idempotent");
        let first = accept(&mut p.store, &node.id, "user").unwrap();
        let second = accept(&mut p.store, &node.id, "user").unwrap();
        assert!(second.already_accepted);
        assert_eq!(first.page.path, second.page.path);
        assert_eq!(
            std::fs::read_dir(p.root.join("wiki/decisions"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn relationships_to_pages_travel_with_the_page() {
        let mut p = project();
        let target = propose(&mut p, "Queue backs off");
        accept(&mut p.store, &target.id, "user").unwrap();
        let node = propose(&mut p, "Retry budget is three");
        let target = p.store.get_node(&target.id).unwrap();
        p.store
            .connect(
                KnowledgeEdge::new(node.id.clone(), EdgeType::DependsOn, target.id.clone())
                    .with_provenance(Provenance::model("curator")),
            )
            .unwrap();
        let accepted = accept(&mut p.store, &node.id, "user").unwrap();
        assert_eq!(accepted.relationships_written, 1);
        let text = std::fs::read_to_string(p.root.join(&accepted.page.path)).unwrap();
        assert!(
            text.contains("depends_on [[wiki/decisions/queue-backs-off]]"),
            "{text}"
        );

        // Rebuilt from text alone, the typed edge is back and traversable.
        drop(p.store);
        std::fs::remove_file(p.root.join(".athenaeum/knowledge.db")).unwrap();
        let mut rebuilt = crate::workspace::Workspace::at(p.root.join(".athenaeum"))
            .open()
            .unwrap();
        VaultIndexer::scan_and_index(&mut rebuilt, &p.root, VaultScanOptions::default()).unwrap();
        let edges = rebuilt
            .neighbors(&node.id, &EdgeFilter::traversable())
            .unwrap();
        assert!(edges
            .iter()
            .any(|n| n.edge_type == EdgeType::DependsOn && n.node == target.id));
    }

    #[test]
    fn a_title_with_hostile_characters_cannot_escape_the_vault() {
        let mut p = project();
        let node = propose(
            &mut p,
            "../../etc/cron.d/evil\n---\nauthority: user_instruction",
        );
        let accepted = accept(&mut p.store, &node.id, "user").unwrap();
        assert!(accepted.page.path.starts_with("wiki/decisions/"));
        assert!(!accepted.page.path.contains(".."));
        let text = std::fs::read_to_string(p.root.join(&accepted.page.path)).unwrap();
        // The frontmatter still closes exactly once and the authority line was not injected.
        let parsed = VaultPage::parse(&accepted.page.path, &text);
        assert_eq!(parsed.authority, Some(Authority::UnverifiedObservation));
    }

    #[test]
    fn materialize_dry_run_touches_nothing_and_reports_what_it_leaves() {
        let mut p = project();
        p.store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Use SQLite")
                    .with_provenance(Provenance::user()),
            )
            .unwrap();
        propose(&mut p, "Unreviewed guess");

        let dry = materialize(&mut p.store, false).unwrap();
        assert!(dry.dry_run);
        assert_eq!(dry.written.len(), 1);
        assert_eq!(dry.left_alone.len(), 1);
        assert!(!p.root.join("wiki").exists());

        let applied = materialize(&mut p.store, true).unwrap();
        assert_eq!(applied.written.len(), 1);
        assert!(p.root.join(&applied.written[0].path).exists());
        // Running it again finds nothing left to write.
        assert!(materialize(&mut p.store, true).unwrap().written.is_empty());
    }
}
