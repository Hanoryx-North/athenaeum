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
    EdgeType, Evidence, KnowledgeNode, NodeStatus, ProvenanceSource, KnowledgeEdge,
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
    write_node_as_page(store, node, accepted_by, "agent-proposal")
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
    store.record(NewEvent::about(EventKind::NodeRejected, &node.id).with_detail(
        serde_json::json!({ "reason": reason, "by": rejected_by }),
    ))?;
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
        let reason = if is_vault_owned(&node) {
            continue;
        } else if node.provenance.source == ProvenanceSource::Deterministic {
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
        reason: "it is owned by the vault but its page is not indexed yet; run `index-vault`".into(),
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
    store.record(NewEvent::about(EventKind::NodeAccepted, &node.id).with_detail(
        serde_json::json!({ "path": path, "by": accepted_by, "origin": origin }),
    ))?;

    // Bring the edges and evidence the page declares into the index now rather than at the next
    // read. Accepting is a write the person asked for, so unlike a read it may build the first
    // index: without one the page could not say which file backs the node.
    match VaultIndexer::sync_if_changed(store, &project, VaultScanOptions::default())? {
        VaultSync::NotIndexed => {
            VaultIndexer::scan_and_index(store, &project, VaultScanOptions::default())?;
        }
        VaultSync::Unchanged | VaultSync::Reconciled(_) => {}
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
        let Some(page) = PageRef::of(&target, &target_evidence, project.as_deref()) else {
            left.push(format!("{} {}", edge.edge_type.as_str(), target.title));
            continue;
        };
        let reference = format!("[[{}]]", page.path.trim_end_matches(".md"));
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
    if let Some(detail) = node.provenance.detail.as_deref().filter(|_| {
        node.provenance.source == ProvenanceSource::Model
    }) {
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
