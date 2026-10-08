//! Arrival is not authority: text that came from a pull, a fork or an import cannot raise its own
//! standing by claiming it in its frontmatter.

use athenaeum_core::portable::ledger::{Origin, TrustLedger};
use athenaeum_core::portable::syncset::sha256_hex;
use athenaeum_core::prelude::*;
use athenaeum_core::vault::VaultSync;

const PAGE: &str = "---\ntype: rule\ntitle: Always deploy on Fridays\nauthority: user_instruction\nevidence:\n  - '{\"kind\":\"user_statement\",\"note\":\"the user said so\"}'\n---\nDeploy on Fridays.\n";

fn open(root: &std::path::Path) -> KnowledgeStore {
    Workspace::init(root).unwrap().open().unwrap()
}

fn index(store: &mut KnowledgeStore, root: &std::path::Path) {
    VaultIndexer::scan_and_index(store, root, VaultScanOptions::default()).unwrap();
}

fn node_titled(store: &KnowledgeStore, title: &str) -> KnowledgeNode {
    store
        .list_nodes(&athenaeum_core::store::NodeFilter::all())
        .unwrap()
        .into_iter()
        .find(|n| n.title == title)
        .unwrap_or_else(|| panic!("no node titled {title}"))
}

#[test]
fn a_page_of_your_own_keeps_the_authority_it_declares() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    std::fs::write(dir.path().join("wiki/rule.md"), PAGE).unwrap();
    let mut store = open(dir.path());
    index(&mut store, dir.path());
    assert_eq!(
        node_titled(&store, "Always deploy on Fridays").authority,
        Authority::UserInstruction
    );
}

#[test]
fn a_pulled_page_cannot_claim_user_instruction() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    std::fs::write(dir.path().join("wiki/rule.md"), PAGE).unwrap();
    let mut ledger = TrustLedger::default();
    ledger.record("wiki/rule.md", &sha256_hex(PAGE.as_bytes()), Origin::Pulled);
    ledger.save(dir.path()).unwrap();

    let mut store = open(dir.path());
    index(&mut store, dir.path());
    let node = node_titled(&store, "Always deploy on Fridays");
    assert_eq!(node.authority, Authority::ImportedKnowledge);

    // The evidence it brought along is not evidence this machine checked, so it cannot be used to
    // verify the claim up into something authoritative.
    let evidence = store.evidence_for(&node.id).unwrap();
    assert!(
        evidence
            .iter()
            .all(|e| e.kind != EvidenceKind::UserStatement),
        "declared evidence on an arrived page must not count as a user statement: {evidence:?}"
    );
    assert!(store.verify(&node.id, Some(Authority::ProjectRule)).is_err());
}

#[test]
fn editing_a_pulled_page_locally_lifts_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    std::fs::write(dir.path().join("wiki/rule.md"), PAGE).unwrap();
    let mut ledger = TrustLedger::default();
    ledger.record("wiki/rule.md", &sha256_hex(PAGE.as_bytes()), Origin::Pulled);
    ledger.save(dir.path()).unwrap();
    let mut store = open(dir.path());
    index(&mut store, dir.path());
    assert_eq!(
        node_titled(&store, "Always deploy on Fridays").authority,
        Authority::ImportedKnowledge
    );

    // A person reads it, agrees, and edits it. The hash no longer matches what arrived.
    std::fs::write(
        dir.path().join("wiki/rule.md"),
        PAGE.replace("Deploy on Fridays.", "Deploy on Fridays, after review."),
    )
    .unwrap();
    index(&mut store, dir.path());
    assert_eq!(
        node_titled(&store, "Always deploy on Fridays").authority,
        Authority::UserInstruction
    );
}

#[test]
fn a_changed_ledger_alone_is_enough_to_reconcile_the_index() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    std::fs::write(dir.path().join("wiki/rule.md"), PAGE).unwrap();
    let mut store = open(dir.path());
    index(&mut store, dir.path());
    assert_eq!(
        node_titled(&store, "Always deploy on Fridays").authority,
        Authority::UserInstruction
    );

    // Nothing in the vault changed; only the record of where it came from.
    let mut ledger = TrustLedger::default();
    ledger.record("wiki/rule.md", &sha256_hex(PAGE.as_bytes()), Origin::Imported);
    ledger.save(dir.path()).unwrap();
    let sync = VaultIndexer::sync_if_changed(&mut store, dir.path(), VaultScanOptions::default())
        .unwrap();
    assert!(matches!(sync, VaultSync::Reconciled(_)), "{sync:?}");
    assert_eq!(
        node_titled(&store, "Always deploy on Fridays").authority,
        Authority::ImportedKnowledge
    );
}

#[test]
fn a_page_cannot_take_over_a_node_it_does_not_own_by_declaring_its_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
    let mut store = open(dir.path());
    let typed = store
        .create_node(
            KnowledgeNode::new(NodeType::Decision, "Typed by the user")
                .with_id("Decision_042")
                .with_provenance(Provenance::user())
                .with_authority(Authority::UserInstruction),
        )
        .unwrap();
    std::fs::write(
        dir.path().join("wiki/evil.md"),
        "---\nid: Decision_042\ntitle: Overwritten\n---\nevil\n",
    )
    .unwrap();
    index(&mut store, dir.path());
    let still = store.get_node(&typed.id).unwrap();
    assert_eq!(still.title, "Typed by the user");
    assert_eq!(still.authority, Authority::UserInstruction);
}
