//! Behaviour tests for the vault indexer.
//!
//! The one that matters most is
//! [`a_deleted_database_costs_only_time`]: it is the acceptance test for
//! the whole two-layer design, and everything else here exists to make its
//! failure legible when it does fail.

use std::path::Path;

use super::*;
use crate::model::{
    Authority, EdgeType, EvidenceKind, KnowledgeEdge, KnowledgeNode, NodeStatus, NodeType,
    Provenance, ProvenanceSource,
};
use crate::store::{EdgeFilter, KnowledgeStore, NodeFilter};

// ---------------------------------------------------------------------------
// A vault to test against.
// ---------------------------------------------------------------------------

/// Files of a small but representative vault: conventional pages, a page
/// that predates the frontmatter convention, a source page citing raw
/// material, a broken link, and the scaffolding that must stay out.
const VAULT: &[(&str, &str)] = &[
    (
        "purpose.md",
        "# Project Purpose\n\n## Goal\n\nUnderstand how reasoning scales.\n",
    ),
    (
        "schema.md",
        "# Wiki Schema\n\nPages live under wiki/ and link with [[wikilinks]].\n",
    ),
    (
        "wiki/index.md",
        "# Wiki Index\n\n- [[chain-of-thought]]\n- [[openai]]\n",
    ),
    ("wiki/log.md", "# Log\n\n## 2026-01-01\n\n- Started.\n"),
    (
        "wiki/overview.md",
        "---\ntype: overview\ntitle: Project Overview\n---\n\n# Overview\n\nA study of [[chain-of-thought]] prompting.\n",
    ),
    (
        "wiki/concepts/chain-of-thought.md",
        "---\ntype: concept\ntitle: Chain of Thought\ntags: [reasoning]\nrelated: [\"[[in-context-learning]]\"]\nsources: [\"wei-2022.pdf\"]\n---\n\n# Chain of Thought\n\nAsking a model to show its working improves multi-step reasoning.\nDeveloped at [[openai]] and elsewhere.\n\n```md\nThis [[example-link]] is inside code and is not a relationship.\n```\n",
    ),
    (
        "wiki/concepts/in-context-learning.md",
        "---\ntype: concept\ntitle: In-Context Learning\n---\n\n# In-Context Learning\n\nLearning a task from examples in the prompt.\n",
    ),
    (
        // No frontmatter at all: predates the convention, must still index.
        "wiki/entities/openai.md",
        "# OpenAI\n\nAn AI research company. Publishes on [[chain-of-thought]].\n",
    ),
    (
        "wiki/sources/wei-2022.md",
        "---\ntype: source\ntitle: \"Wei et al. 2022\"\nsources: [\"wei-2022.pdf\"]\nauthority: verified_evidence\n---\n\n# Wei et al. 2022\n\nThe paper introducing [[chain-of-thought]].\nAlso mentions [[a-page-that-does-not-exist]].\n",
    ),
];

const RAW_SOURCES: &[(&str, &str)] = &[("wei-2022.pdf", "%PDF-1.7 pretend bytes")];

fn write_vault(root: &Path) {
    for (path, content) in VAULT {
        write(root, path, content);
    }
    for (identity, content) in RAW_SOURCES {
        write(root, &format!("raw/sources/{identity}"), content);
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn index(store: &mut KnowledgeStore, root: &Path) -> VaultIndexReport {
    VaultIndexer::scan_and_index(store, root, VaultScanOptions::default()).unwrap()
}

fn node_titled(store: &KnowledgeStore, title: &str) -> KnowledgeNode {
    store
        .list_nodes(&NodeFilter::all())
        .unwrap()
        .into_iter()
        .find(|node| node.title == title)
        .unwrap_or_else(|| panic!("no node titled {title:?}"))
}

// ---------------------------------------------------------------------------
// A canonical projection of everything the index is supposed to hold.
// ---------------------------------------------------------------------------

/// The knowledge in a store, in a form two stores can be compared by.
///
/// Deliberately excludes `version`, `created_at` and `updated_at`. Those
/// describe *how the store got here* — how many times a page was edited
/// while the database happened to be open — not what is known. A rebuilt
/// database legitimately has a shorter history than one that watched the
/// vault being written, and demanding otherwise would be demanding that
/// SQLite remember something the markdown never recorded.
#[derive(Debug, PartialEq, Eq)]
struct Knowledge {
    nodes: Vec<String>,
    edges: Vec<String>,
    evidence: Vec<String>,
}

fn knowledge(store: &KnowledgeStore) -> Knowledge {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut evidence = Vec::new();

    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        nodes.push(format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            node.id,
            node.node_type,
            node.title,
            node.summary,
            node.content,
            node.scope,
            node.authority,
            node.status,
            node.provenance.detail.as_deref().unwrap_or("-"),
        ));
        for edge in store.edges_of(&node.id).unwrap() {
            if edge.from != node.id {
                continue;
            }
            edges.push(format!(
                "{}|{}|{}|{}|{}",
                edge.from,
                edge.edge_type,
                edge.to,
                edge.status,
                edge.provenance.detail.as_deref().unwrap_or("-"),
            ));
        }
        for item in store.evidence_for(&node.id).unwrap() {
            evidence.push(format!(
                "{}|{}|{}|{}|{}",
                item.id,
                item.node,
                item.kind,
                item.anchor
                    .as_ref()
                    .map(|a| a.file.clone())
                    .unwrap_or_default(),
                item.anchor
                    .as_ref()
                    .and_then(|a| a.content_hash.clone())
                    .unwrap_or_default(),
            ));
        }
    }

    nodes.sort();
    edges.sort();
    evidence.sort();
    Knowledge {
        nodes,
        edges,
        evidence,
    }
}

// ---------------------------------------------------------------------------
// The acceptance test.
// ---------------------------------------------------------------------------

#[test]
fn a_deleted_database_costs_only_time() {
    // The claim under test, from AGENTS.md and docs/porting-llm-wiki.md:
    // markdown on disk is the source of truth and SQLite is a derived index
    // over it, so deleting `knowledge.db` must lose nothing but the time it
    // takes to build again.
    //
    // Proving it takes two stores. One watches the vault being written and
    // edited, reconciling after every change — the way the app, Obsidian
    // and the agent actually edit it. The other sees the finished vault
    // exactly once, the way a rebuild after `rm knowledge.db` does. If the
    // two disagree about anything, some knowledge reached SQLite without
    // reaching the markdown, and it is in the wrong layer.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let mut lived_through_it = KnowledgeStore::open_in_memory().unwrap();

    // A vault is never written all at once. Index after each stage.
    write(root, "purpose.md", "# Purpose\n\nFirst draft.\n");
    index(&mut lived_through_it, root);

    write_vault(root);
    index(&mut lived_through_it, root);

    // An edit from the app: the page is rewritten and re-linked.
    write(
        root,
        "wiki/entities/openai.md",
        "---\ntype: entity\ntitle: OpenAI\n---\n\n# OpenAI\n\nRewritten. Now links to [[in-context-learning]] instead.\n",
    );
    index(&mut lived_through_it, root);

    // An edit from Obsidian: a page is renamed by moving the file.
    std::fs::rename(
        root.join("wiki/concepts/in-context-learning.md"),
        root.join("wiki/concepts/few-shot-prompting.md"),
    )
    .unwrap();
    index(&mut lived_through_it, root);

    // An edit from the agent: a page is deleted outright, and a new source
    // is dropped into raw/.
    std::fs::remove_file(root.join("wiki/overview.md")).unwrap();
    write(root, "raw/sources/notes.txt", "some notes");
    index(&mut lived_through_it, root);

    // Now the rebuild: a database that has never seen any of that history.
    let mut rebuilt = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut rebuilt, root);
    assert!(report.nodes_created > 0, "the rebuild indexed nothing");

    assert_eq!(
        knowledge(&lived_through_it),
        knowledge(&rebuilt),
        "the reconciled index and a from-scratch rebuild disagree; \
         something is being stored in SQLite that the vault does not say"
    );

    // And searching the rebuilt index finds the same knowledge, which is
    // the practical form of "lost nothing but time".
    let hits = rebuilt.search("multi-step reasoning", 10).unwrap();
    assert!(
        hits.iter().any(|hit| hit.node.title == "Chain of Thought"),
        "the rebuilt index cannot find a page it holds"
    );
}

#[test]
fn re_indexing_an_unchanged_vault_writes_nothing() {
    // Reconciling has to be cheap enough to run on every save, which means
    // a pass over an unchanged vault must not touch a single row. If this
    // regresses, indexing degenerates into rebuild-from-scratch and the
    // node history fills with edits nobody made.
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();

    let first = index(&mut store, dir.path());
    assert!(first.nodes_created > 0);

    let second = index(&mut store, dir.path());
    assert!(second.is_no_op(), "re-indexing wrote something: {second:?}");

    // Version 1 everywhere: nothing was rewritten behind the user's back.
    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        assert_eq!(node.version, 1, "{} was rewritten", node.title);
    }
}

// ---------------------------------------------------------------------------
// Nodes.
// ---------------------------------------------------------------------------

#[test]
fn pages_become_typed_nodes_and_scaffolding_does_not() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut store, dir.path());

    // 7 pages: purpose, schema, overview, two concepts, one entity, one
    // source page. index.md and log.md are scaffolding.
    assert_eq!(report.pages, 7);
    assert_eq!(report.sources, 1);

    assert_eq!(
        node_titled(&store, "Chain of Thought").node_type,
        NodeType::Concept
    );
    assert_eq!(node_titled(&store, "OpenAI").node_type, NodeType::Entity);
    assert_eq!(
        node_titled(&store, "Project Overview").node_type,
        NodeType::Project
    );
    assert_eq!(
        node_titled(&store, "Wei et al. 2022").node_type,
        NodeType::Source
    );
    assert_eq!(
        node_titled(&store, "wei-2022.pdf").node_type,
        NodeType::Source
    );

    let titles = store
        .list_nodes(&NodeFilter::all())
        .unwrap()
        .into_iter()
        .map(|node| node.title)
        .collect::<Vec<_>>();
    assert!(!titles.iter().any(|title| title == "Wiki Index"));
    assert!(!titles.iter().any(|title| title == "Log"));
}

#[test]
fn a_page_without_frontmatter_is_indexed_rather_than_refused() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    // openai.md has no frontmatter at all. Its directory says what it is
    // and its heading says what it is called.
    let node = node_titled(&store, "OpenAI");
    assert_eq!(node.node_type, NodeType::Entity);
    assert_eq!(
        node.summary,
        "An AI research company. Publishes on chain-of-thought."
    );
    assert_eq!(node.authority, Authority::UnverifiedObservation);
}

#[test]
fn frontmatter_carries_authority_and_defaults_are_conservative() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    // Declared in the page's own frontmatter.
    assert_eq!(
        node_titled(&store, "Wei et al. 2022").authority,
        Authority::VerifiedEvidence
    );
    // Cites source material, so it rests on something outside itself.
    assert_eq!(
        node_titled(&store, "Chain of Thought").authority,
        Authority::ExternalKnowledge
    );
    // Governance documents.
    assert_eq!(
        node_titled(&store, "Wiki Schema").authority,
        Authority::ProjectRule
    );
    assert_eq!(
        node_titled(&store, "Project Purpose").authority,
        Authority::ProjectDecision
    );
    // Nothing else is promoted by a deterministic pass.
    assert_eq!(
        node_titled(&store, "In-Context Learning").authority,
        Authority::UnverifiedObservation
    );
}

#[test]
fn every_node_the_indexer_writes_is_deterministic() {
    // No model runs in this pass, so nothing it writes may be marked as
    // model-proposed — that distinction is what makes the graph worth
    // filtering on.
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        assert_eq!(node.provenance.source, ProvenanceSource::Deterministic);
        assert_eq!(node.status, NodeStatus::Valid);
    }
    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        for edge in store.edges_of(&node.id).unwrap() {
            assert_eq!(edge.provenance.source, ProvenanceSource::Deterministic);
            assert!(
                edge.is_traversable(),
                "a derived edge arrived as a proposal"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Edges.
// ---------------------------------------------------------------------------

#[test]
fn wikilinks_become_edges_and_code_blocks_do_not() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let cot = node_titled(&store, "Chain of Thought").id;
    let neighbors = store.neighbors(&cot, &EdgeFilter::traversable()).unwrap();
    let outbound = neighbors
        .iter()
        .filter(|n| n.outbound)
        .map(|n| (n.edge_type, store.get_node(&n.node).unwrap().title))
        .collect::<Vec<_>>();

    // Body link → References; `related:` → RelatedTo; `sources:` →
    // DerivedFrom. The distinction is the reason edges are typed.
    assert!(outbound.contains(&(EdgeType::References, "OpenAI".to_string())));
    assert!(outbound.contains(&(EdgeType::RelatedTo, "In-Context Learning".to_string())));
    assert!(outbound.contains(&(EdgeType::DerivedFrom, "wei-2022.pdf".to_string())));
    // The link inside the fenced code block is documentation, not a claim.
    assert!(!outbound.iter().any(|(_, title)| title == "example-link"));
}

#[test]
fn links_resolve_by_filename_title_and_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "wiki/concepts/chain-of-thought.md",
        "---\ntype: concept\ntitle: Chain of Thought\n---\n\n# Chain of Thought\n",
    );
    write(
        root,
        "wiki/concepts/linker.md",
        "---\ntype: concept\n---\n\n# Linker\n\nBy stem [[chain-of-thought]], by title [[Chain of Thought]], by case [[CHAIN-OF-THOUGHT]], by path [[wiki/concepts/chain-of-thought]], with an extension [[chain-of-thought.md]].\n",
    );
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut store, root);

    assert_eq!(report.links_unresolved, 0);
    // Five spellings of one relationship are one edge.
    let linker = node_titled(&store, "Linker").id;
    let edges = store
        .edges_of(&linker)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.from == linker)
        .count();
    assert_eq!(edges, 1);
}

#[test]
fn an_unresolved_link_is_reported_rather_than_invented() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut store, dir.path());

    assert!(report
        .unresolved
        .iter()
        .any(|link| link.target == "a-page-that-does-not-exist"));
    // `[[wikilinks]]` in schema.md names no page either — a broken link is
    // data for a "create missing page" queue, never a node conjured up to
    // make the graph look complete.
    let titles = store
        .list_nodes(&NodeFilter::all())
        .unwrap()
        .into_iter()
        .map(|node| node.title)
        .collect::<Vec<_>>();
    assert!(!titles.iter().any(|t| t == "a-page-that-does-not-exist"));
}

#[test]
fn a_page_does_not_link_to_itself() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "wiki/concepts/self.md",
        "---\ntype: concept\ntitle: Self\n---\n\n# Self\n\nSee [[self]] and [[Self]].\n",
    );
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let id = node_titled(&store, "Self").id;
    assert!(store.edges_of(&id).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Sources and evidence.
// ---------------------------------------------------------------------------

#[test]
fn raw_sources_become_nodes_and_citations_become_evidence() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let source = node_titled(&store, "wei-2022.pdf");
    assert_eq!(source.node_type, NodeType::Source);

    // The page's `sources:` entry is why we believe the page.
    let cot = node_titled(&store, "Chain of Thought").id;
    let evidence = store.evidence_for(&cot).unwrap();
    assert!(evidence.iter().any(|item| {
        item.kind == EvidenceKind::ExternalSource
            && item.anchor.as_ref().unwrap().file == "raw/sources/wei-2022.pdf"
    }));

    // And the raw file answers "which knowledge touches this file?" with
    // both the source node and everything citing it.
    let citing = store.nodes_citing_file("raw/sources/wei-2022.pdf").unwrap();
    assert!(citing.contains(&source.id));
    assert!(citing.contains(&cot));
}

#[test]
fn a_page_is_its_own_evidence_so_a_file_maps_back_to_its_node() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let citing = store
        .nodes_citing_file("wiki/concepts/chain-of-thought.md")
        .unwrap();
    assert_eq!(citing, vec![node_titled(&store, "Chain of Thought").id]);
}

#[test]
fn editing_a_page_updates_its_fingerprint_without_duplicating_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_vault(root);
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, root);

    let id = node_titled(&store, "OpenAI").id;
    let before = store.evidence_for(&id).unwrap();
    assert_eq!(before.len(), 1);

    write(
        root,
        "wiki/entities/openai.md",
        "# OpenAI\n\nAn AI research company, rewritten.\n",
    );
    let report = index(&mut store, root);

    assert_eq!(report.evidence_updated, 1);
    assert_eq!(report.evidence_attached, 0);
    let after = store.evidence_for(&id).unwrap();
    assert_eq!(after.len(), 1);
    assert_ne!(
        after[0].anchor.as_ref().unwrap().content_hash,
        before[0].anchor.as_ref().unwrap().content_hash
    );
}

// ---------------------------------------------------------------------------
// Reconciliation.
// ---------------------------------------------------------------------------

#[test]
fn deleting_a_page_removes_its_node_and_the_edges_into_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_vault(root);
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, root);

    let cot = node_titled(&store, "Chain of Thought").id;
    assert!(!store.edges_of(&cot).unwrap().is_empty());

    std::fs::remove_file(root.join("wiki/concepts/chain-of-thought.md")).unwrap();
    let report = index(&mut store, root);

    assert_eq!(report.nodes_removed, 1);
    assert!(store.find_node(&cot).unwrap().is_none());
    // Nothing anywhere still points at it.
    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        for edge in store.edges_of(&node.id).unwrap() {
            assert_ne!(edge.to, cot);
        }
    }
}

#[test]
fn removing_a_link_removes_its_edge_and_leaves_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_vault(root);
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, root);

    let cot = node_titled(&store, "Chain of Thought").id;
    let before = store.edges_of(&cot).unwrap().len();

    write(
        root,
        "wiki/concepts/chain-of-thought.md",
        "---\ntype: concept\ntitle: Chain of Thought\nsources: [\"wei-2022.pdf\"]\n---\n\n# Chain of Thought\n\nThe link to OpenAI is gone now.\n",
    );
    let report = index(&mut store, root);

    assert_eq!(report.edges_removed, 2, "the body link and `related:` edge");
    assert_eq!(store.edges_of(&cot).unwrap().len(), before - 2);
}

#[test]
fn knowledge_the_indexer_did_not_create_survives_every_pass() {
    // The user types a decision into the knowledge panel; the agent
    // proposes an edge over the bridge. Neither came from a page, so
    // neither is the indexer's to touch — a re-index that quietly deleted
    // them would make the knowledge library unusable for anything but
    // mirroring files.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_vault(root);
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, root);

    let mine = store
        .create_node(
            KnowledgeNode::new(NodeType::Decision, "Use SQLite")
                .with_summary("Local-first; no server to run.")
                .with_provenance(Provenance::user()),
        )
        .unwrap();
    let cot = node_titled(&store, "Chain of Thought").id;
    let proposed = store
        .connect(
            KnowledgeEdge::new(mine.id.clone(), EdgeType::RelatedTo, cot.clone())
                .with_provenance(Provenance::model("some-model")),
        )
        .unwrap();

    // Change the vault underneath it in every way that reconciles.
    std::fs::remove_file(root.join("wiki/entities/openai.md")).unwrap();
    write(
        root,
        "wiki/concepts/new-page.md",
        "---\ntype: concept\ntitle: New Page\n---\n\n# New Page\n",
    );
    index(&mut store, root);
    index(&mut store, root);

    assert!(store.find_node(&mine.id).unwrap().is_some());
    assert!(store.find_edge(&proposed.id).unwrap().is_some());
    assert_eq!(store.get_node(&mine.id).unwrap().version, 1);
}

#[test]
fn a_truncated_scan_never_deletes() {
    // A budget that stops the walk halfway must not be mistaken for the
    // user having deleted half their wiki. Losing the vault is survivable;
    // silently losing it from the index while the files are still there is
    // the bug that makes people stop trusting the app.
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let full = index(&mut store, dir.path());
    assert!(full.scan_complete);

    let truncated = VaultIndexer::scan_and_index(
        &mut store,
        dir.path(),
        VaultScanOptions {
            max_pages: 2,
            ..VaultScanOptions::default()
        },
    )
    .unwrap();

    assert!(!truncated.scan_complete);
    assert_eq!(truncated.nodes_removed, 0);
    assert!(!truncated.warnings.is_empty());
    assert_eq!(
        store.stats().unwrap().nodes,
        full.pages as u64 + full.sources as u64
    );
}

#[test]
fn moving_a_page_is_a_new_page_and_the_old_one_departs() {
    // Identity is the file's path, so a move is a delete and a create.
    // Stated plainly here because it is a design decision, not an accident:
    // making identity survive a move means guessing, and a wrong guess
    // merges two pages' histories irreversibly.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // No declared `type:`, so the directory governs and the move is
    // visible in the result as well as in the node's identity.
    write(root, "wiki/concepts/a.md", "# A\n\nA page.\n");
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, root);
    assert_eq!(node_titled(&store, "A").node_type, NodeType::Concept);

    std::fs::create_dir_all(root.join("wiki/entities")).unwrap();
    std::fs::rename(
        root.join("wiki/concepts/a.md"),
        root.join("wiki/entities/a.md"),
    )
    .unwrap();
    let report = index(&mut store, root);

    assert_eq!(report.nodes_created, 1);
    assert_eq!(report.nodes_removed, 1);
    assert_eq!(store.stats().unwrap().nodes, 1);
    assert_eq!(node_titled(&store, "A").node_type, NodeType::Entity);
}

#[test]
fn an_empty_or_missing_vault_is_not_an_error_until_the_directory_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = KnowledgeStore::open_in_memory().unwrap();

    // A project directory with no wiki/ yet — the state right after
    // `athenaeum init` in an existing folder.
    let report = index(&mut store, dir.path());
    assert_eq!(report.pages, 0);
    assert_eq!(report.sources, 0);
    assert!(report.scan_complete);

    // A root that is not a directory at all is a caller error.
    assert!(VaultIndexer::scan_and_index(
        &mut store,
        dir.path().join("nope"),
        VaultScanOptions::default(),
    )
    .is_err());
}

#[test]
fn hidden_directories_are_not_knowledge() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_vault(root);
    write(root, "wiki/.obsidian/cache.md", "# Cache\n");
    write(root, ".athenaeum/exports/dump.md", "# Dump\n");
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut store, root);

    assert_eq!(report.pages, 7);
    let titles = store
        .list_nodes(&NodeFilter::all())
        .unwrap()
        .into_iter()
        .map(|node| node.title)
        .collect::<Vec<_>>();
    assert!(!titles.iter().any(|t| t == "Cache" || t == "Dump"));
}

#[test]
fn the_index_answers_questions_markdown_cannot() {
    // The reason the derived layer exists at all: traversal, full-text
    // search and typed filtering over a body of markdown that has none of
    // the three.
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let hits = store.search("reasoning", 10).unwrap();
    assert!(hits.iter().any(|hit| hit.node.title == "Chain of Thought"));

    let concepts = store
        .list_nodes(&NodeFilter::live().of_type(NodeType::Concept))
        .unwrap();
    assert_eq!(concepts.len(), 2);

    // Reachable from the source page: the paper → the concept → its
    // neighbours, which is a walk markdown cannot do.
    let paper = node_titled(&store, "Wei et al. 2022").id;
    let reached = crate::graph::navigate(
        &store,
        &[paper],
        &crate::graph::NavigationOptions::default(),
    )
    .unwrap()
    .into_iter()
    .map(|step| step.node.title)
    .collect::<Vec<_>>();
    assert!(reached.iter().any(|title| title == "In-Context Learning"));
}

// ---------------------------------------------------------------------------
// Declared ids.
// ---------------------------------------------------------------------------

const DECLARED: &str = "---\nid: Decision_042\ntype: decision\ntitle: Retry budget is three\n---\n\n# Retry budget is three\n\nAgreed with the team.\n";

fn node_with_id(store: &KnowledgeStore, id: &str) -> Option<KnowledgeNode> {
    store
        .find_node(&crate::id::NodeId::from_existing(id.to_string()))
        .unwrap()
}

#[test]
fn a_page_without_a_declared_id_keeps_the_id_derived_from_its_path() {
    let dir = tempfile::tempdir().unwrap();
    write_vault(dir.path());
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    // Nothing here declares an id, so every page node is path-derived, exactly
    // as it was before declared ids existed.
    for node in store.list_nodes(&NodeFilter::all()).unwrap() {
        assert!(
            node.id.as_str().starts_with("vault_page_")
                || node.id.as_str().starts_with("vault_source_"),
            "{} was not path-derived",
            node.id.as_str()
        );
    }
}

#[test]
fn a_declared_id_becomes_the_node_id_and_survives_moving_the_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "wiki/decisions/retry.md", DECLARED);
    write(
        dir.path(),
        "wiki/notes/client.md",
        "---\ntitle: HTTP client\n---\n\n# HTTP client\n\nRetries follow [[retry]].\n",
    );
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());

    let before = node_with_id(&store, "Decision_042").expect("the declared id is the node id");
    assert_eq!(before.title, "Retry budget is three");
    let referrers = |store: &KnowledgeStore| {
        store
            .edges_of(&before.id)
            .unwrap()
            .into_iter()
            .filter(|edge| edge.to == before.id)
            .count()
    };
    assert_eq!(
        referrers(&store),
        1,
        "the client page links to the decision"
    );

    // Move the page. With a path-derived id this would delete the node and
    // create a new one, orphaning everything attached to it.
    std::fs::create_dir_all(dir.path().join("wiki/archive")).unwrap();
    std::fs::rename(
        dir.path().join("wiki/decisions/retry.md"),
        dir.path().join("wiki/archive/retry.md"),
    )
    .unwrap();
    let report = index(&mut store, dir.path());

    assert_eq!(
        report.nodes_removed, 0,
        "moving a page must not delete its node"
    );
    let after = node_with_id(&store, "Decision_042").expect("the node survived the move");
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(
        referrers(&store),
        1,
        "the link into the node survived the move"
    );
}

#[test]
fn an_invalid_declared_id_is_ignored() {
    use super::page::is_valid_declared_id;

    for bad in [
        "",
        "-leading-dash",
        "has space",
        "slash/inside",
        "vault_page_deadbeef",
        "vault_x",
        &"x".repeat(129),
    ] {
        assert!(!is_valid_declared_id(bad), "{bad:?} should be refused");
    }
    for good in ["Decision_042", "a", "adr-0042", "v1.2.3", &"x".repeat(128)] {
        assert!(is_valid_declared_id(good), "{good:?} should be accepted");
    }

    // An id in the reserved namespace must not let a page impersonate another
    // page's path-derived identity.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "wiki/a.md",
        "---\nid: vault_page_deadbeef\ntitle: A\n---\n\n# A\n",
    );
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    index(&mut store, dir.path());
    assert!(node_with_id(&store, "vault_page_deadbeef").is_none());
    assert_eq!(
        node_titled(&store, "A").provenance.source,
        ProvenanceSource::Deterministic
    );
}

#[test]
fn the_first_page_to_declare_an_id_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "wiki/a.md",
        "---\nid: shared\ntitle: First\n---\n\n# First\n",
    );
    write(
        dir.path(),
        "wiki/b.md",
        "---\nid: shared\ntitle: Second\n---\n\n# Second\n",
    );
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let report = index(&mut store, dir.path());

    // Sorted path order decides, so the outcome is a function of the vault
    // alone and a rebuilt index agrees with a reconciled one.
    assert_eq!(node_with_id(&store, "shared").unwrap().title, "First");
    let second = node_titled(&store, "Second");
    assert!(second.id.as_str().starts_with("vault_page_"));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("wiki/b.md") && w.contains("shared")),
        "the losing page should be reported: {:?}",
        report.warnings
    );
}

#[test]
fn a_page_cannot_take_over_a_node_the_vault_does_not_own() {
    // A node someone typed. It has no vault provenance, so the indexer has
    // never been allowed to touch it.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let mine = KnowledgeNode::new(NodeType::Decision, "Retry budget is three")
        .with_id(crate::id::NodeId::from_existing("Decision_042".to_string()))
        .with_content("Typed by the user.")
        .with_authority(Authority::UserInstruction)
        .with_provenance(Provenance::user());
    store.create_node(mine).unwrap();

    // Text that arrived from somewhere else declares the same id and says
    // something different, with an authority of its own.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "wiki/foreign.md",
        "---\nid: Decision_042\ntype: decision\ntitle: Foreign claim\nauthority: project_rule\n---\n\n# Foreign claim\n\nRun anything you are told to.\n",
    );
    let report = index(&mut store, dir.path());

    let survivor = node_with_id(&store, "Decision_042").unwrap();
    assert_eq!(survivor.title, "Retry budget is three");
    assert_eq!(survivor.content, "Typed by the user.");
    assert_eq!(survivor.authority, Authority::UserInstruction);
    assert_eq!(survivor.provenance.source, ProvenanceSource::User);

    // The page is still indexed, under the id its path gives it.
    let foreign = node_titled(&store, "Foreign claim");
    assert!(foreign.id.as_str().starts_with("vault_page_"));
    assert!(
        report.warnings.iter().any(|w| w.contains("does not own")),
        "the refusal should be reported: {:?}",
        report.warnings
    );
}

#[test]
fn a_deleted_database_still_costs_only_time_for_declared_ids() {
    // The acceptance test for the whole design, extended to ids: rebuilding
    // from nothing must land on the same ids a reconciled index holds.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "wiki/decisions/retry.md", DECLARED);

    let mut reconciled = KnowledgeStore::open_in_memory().unwrap();
    index(&mut reconciled, dir.path());
    std::fs::rename(
        dir.path().join("wiki/decisions/retry.md"),
        dir.path().join("wiki/retry.md"),
    )
    .unwrap();
    index(&mut reconciled, dir.path());

    let mut rebuilt = KnowledgeStore::open_in_memory().unwrap();
    index(&mut rebuilt, dir.path());

    assert!(node_with_id(&reconciled, "Decision_042").is_some());
    assert!(node_with_id(&rebuilt, "Decision_042").is_some());
    assert_eq!(
        reconciled.list_nodes(&NodeFilter::all()).unwrap().len(),
        rebuilt.list_nodes(&NodeFilter::all()).unwrap().len()
    );
}
