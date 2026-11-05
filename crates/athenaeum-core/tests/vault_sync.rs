//! Keeping an index current with a vault an agent is editing.
//!
//! An agent changes the vault with ordinary file tools, and a headless session has nothing
//! watching it. `sync_if_changed` is how a read finds out, cheaply, whether the index is behind.

use std::path::Path;

use athenaeum_core::vault::{VaultIndexer, VaultScanOptions, VaultSync};
use athenaeum_core::wiki::WikiService;
use athenaeum_core::workspace::Workspace;
use athenaeum_core::{store::KnowledgeStore, Result};

const PAGE: &str =
    "---\ntype: concept\ntitle: OAuthCallback\n---\n\n# OAuthCallback\n\nExchanges the code.\n";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn sync(store: &mut KnowledgeStore, root: &Path) -> Result<VaultSync> {
    VaultIndexer::sync_if_changed(store, root, VaultScanOptions::default())
}

fn index(store: &mut KnowledgeStore, root: &Path) {
    VaultIndexer::scan_and_index(store, root, VaultScanOptions::default()).unwrap();
}

fn project() -> (tempfile::TempDir, KnowledgeStore) {
    let project = tempfile::tempdir().unwrap();
    write(project.path(), "wiki/concepts/oauthcallback.md", PAGE);
    let store = Workspace::init(project.path()).unwrap().open().unwrap();
    (project, store)
}

#[test]
fn a_read_never_builds_the_first_index() {
    // A first index of a large vault outlasts what a caller waiting on an answer should wait for,
    // so it stays an explicit act. Nothing was indexed, so there is nothing to keep current.
    let (project, mut store) = project();

    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::NotIndexed
    );

    assert_eq!(store.stats().unwrap().nodes, 0);
}

#[test]
fn an_unchanged_vault_is_not_read_again() {
    let (project, mut store) = project();
    index(&mut store, project.path());

    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::Unchanged
    );
    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::Unchanged
    );
}

#[test]
fn an_edited_page_is_what_the_next_read_sees() {
    let (project, mut store) = project();
    index(&mut store, project.path());

    write(
        project.path(),
        "wiki/concepts/oauthcallback.md",
        &format!("{PAGE}\nAlso validates the state parameter.\n"),
    );
    let outcome = sync(&mut store, project.path()).unwrap();

    let VaultSync::Reconciled(report) = outcome else {
        panic!("an edited page must be reconciled, got {outcome:?}");
    };
    assert_eq!(report.nodes_updated, 1);
    let detail = WikiService::new(&mut store).open("OAuthCallback").unwrap();
    assert!(detail
        .node
        .content
        .contains("validates the state parameter"));
    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::Unchanged,
        "once reconciled it is current again"
    );
}

#[test]
fn a_page_the_agent_created_becomes_searchable() {
    let (project, mut store) = project();
    index(&mut store, project.path());
    assert!(store.search("session fixation", 5).unwrap().is_empty());

    write(
        project.path(),
        "wiki/gotchas/session-fixation.md",
        "---\ntype: gotcha\ntitle: Session fixation\n---\n\n# Session fixation\n\nRotate the session id on login.\n",
    );
    let outcome = sync(&mut store, project.path()).unwrap();

    assert!(matches!(outcome, VaultSync::Reconciled(_)), "{outcome:?}");
    assert_eq!(store.search("session fixation", 5).unwrap().len(), 1);
}

#[test]
fn a_page_the_agent_deleted_leaves_the_index() {
    let (project, mut store) = project();
    index(&mut store, project.path());
    assert_eq!(store.search("OAuthCallback", 5).unwrap().len(), 1);

    std::fs::remove_file(project.path().join("wiki/concepts/oauthcallback.md")).unwrap();
    let outcome = sync(&mut store, project.path()).unwrap();

    assert!(matches!(outcome, VaultSync::Reconciled(_)), "{outcome:?}");
    assert!(store.search("OAuthCallback", 5).unwrap().is_empty());
}

#[test]
fn a_new_raw_source_is_noticed() {
    let (project, mut store) = project();
    index(&mut store, project.path());

    write(
        project.path(),
        "raw/sources/rfc6749.md",
        "OAuth 2.0 framework",
    );
    let outcome = sync(&mut store, project.path()).unwrap();

    let VaultSync::Reconciled(report) = outcome else {
        panic!("a new source must be reconciled, got {outcome:?}");
    };
    assert_eq!(report.sources, 1);
}

#[test]
fn files_the_indexer_ignores_do_not_trigger_a_reconcile() {
    // Scaffolding, hidden directories and non-markdown files under wiki/ are not knowledge, so
    // touching them must not make every later read pay for a re-read of the vault.
    let (project, mut store) = project();
    index(&mut store, project.path());

    write(
        project.path(),
        "wiki/index.md",
        "# Index\n\n- [[oauthcallback]]\n",
    );
    write(project.path(), "wiki/log.md", "# Log\n\n- did a thing\n");
    write(project.path(), "wiki/.obsidian/workspace.md", "ui state");
    write(project.path(), "wiki/diagram.png", "not markdown");
    write(project.path(), "notes-outside-the-vault.md", "# Not a page");

    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::Unchanged
    );
}

#[test]
fn an_explicit_index_records_what_it_saw_so_the_next_read_is_not_a_redo() {
    // The CLI's `index-vault` and the app both go through scan_and_index, so either one leaves the
    // index in a state a later read recognises as current.
    let (project, mut store) = project();

    index(&mut store, project.path());
    write(project.path(), "wiki/concepts/new.md", "# New\n\nBody.\n");
    index(&mut store, project.path());

    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::Unchanged
    );
}

#[test]
fn deleting_the_database_forgets_that_a_vault_was_indexed() {
    // The fingerprint lives beside the index, so the index being derived data stays true: lose the
    // database and the vault is simply not indexed yet.
    let (project, mut store) = project();
    index(&mut store, project.path());
    drop(store);
    std::fs::remove_file(project.path().join(".athenaeum/knowledge.db")).unwrap();
    let _ = std::fs::remove_file(project.path().join(".athenaeum/knowledge.db-wal"));
    let _ = std::fs::remove_file(project.path().join(".athenaeum/knowledge.db-shm"));

    let mut store = Workspace::at(project.path().join(".athenaeum"))
        .open()
        .unwrap();

    assert_eq!(
        sync(&mut store, project.path()).unwrap(),
        VaultSync::NotIndexed
    );
}
