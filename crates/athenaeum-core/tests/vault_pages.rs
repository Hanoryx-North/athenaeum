//! A node that is a page of the vault says which file it is.
//!
//! The vault is the source of truth and an agent changes it with ordinary file tools, so the
//! seam has to hand it a path. These tests index a real vault on disk and read the pointer back
//! through the same service the bridge uses.

use std::path::Path;

use athenaeum_core::model::{hash, NodeType, Scope};
use athenaeum_core::vault::{VaultIndexer, VaultScanOptions};
use athenaeum_core::wiki::{WikiCaller, WikiNodeCreate, WikiNodeUpdate, WikiService};
use athenaeum_core::workspace::Workspace;
use athenaeum_core::{id::SessionId, store::KnowledgeStore};

const OAUTH_PAGE: &str = "---\ntype: concept\ntitle: OAuthCallback\nrelated: [\"[[validate-state]]\"]\n---\n\n# OAuthCallback\n\nExchanges the code, then creates the session.\n";
const DECISION_PAGE: &str = "---\ntype: decision\ntitle: Validate state first\nauthority: project_decision\nsources: [\"rfc.md\"]\n---\n\n# Validate state first\n\nCreating the session first opens a login CSRF. Constrains [[oauthcallback]].\n";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A project with a vault, a workspace, and the vault indexed into it.
fn indexed_project() -> (tempfile::TempDir, KnowledgeStore) {
    let project = tempfile::tempdir().unwrap();
    write(project.path(), "wiki/concepts/oauthcallback.md", OAUTH_PAGE);
    write(
        project.path(),
        "wiki/decisions/validate-state.md",
        DECISION_PAGE,
    );
    write(
        project.path(),
        "raw/sources/rfc.md",
        "RFC 6749 section 10.12",
    );
    let mut store = Workspace::init(project.path()).unwrap().open().unwrap();
    VaultIndexer::scan_and_index(&mut store, project.path(), VaultScanOptions::default()).unwrap();
    (project, store)
}

fn caller() -> WikiCaller {
    WikiCaller {
        model: "test-model".to_string(),
        session: Some(SessionId::from_existing("sess_pages")),
    }
}

#[test]
fn an_opened_page_names_the_file_it_came_from() {
    let (project, mut store) = indexed_project();

    let detail = WikiService::new(&mut store).open("OAuthCallback").unwrap();

    let page = detail.page.expect("a vault page carries a pointer");
    assert_eq!(page.path, "wiki/concepts/oauthcallback.md");
    let absolute = page.absolute_path.expect("the project root is known");
    assert!(
        Path::new(&absolute).is_absolute(),
        "{absolute} is not absolute"
    );
    assert_eq!(
        std::fs::read_to_string(&absolute).unwrap(),
        OAUTH_PAGE,
        "the absolute path must open the page itself"
    );
    assert!(absolute.starts_with(project.path().to_str().unwrap()));
    assert_eq!(page.content_hash, hash(OAUTH_PAGE));
}

#[test]
fn the_pointer_is_in_addition_to_the_node_never_instead_of_it() {
    let (_project, mut store) = indexed_project();

    let detail = WikiService::new(&mut store).open("OAuthCallback").unwrap();

    // The prose is in the markdown. These are not, and are why the knowledge base exists.
    assert!(detail.page.is_some());
    assert!(
        !detail.relationships.is_empty(),
        "relationships stay on the node"
    );
    assert!(!detail.evidence.is_empty(), "evidence stays on the node");
    assert!(detail.node.content.contains("Exchanges the code"));
}

#[test]
fn a_node_that_is_not_a_page_has_no_pointer() {
    let (_project, mut store) = indexed_project();
    let mut service = WikiService::new(&mut store);
    service
        .create(
            WikiNodeCreate {
                node_type: NodeType::Observation,
                title: "Callbacks are slow on Mondays".to_string(),
                summary: "A model's guess.".to_string(),
                content: String::new(),
                scope: Scope::Project,
            },
            caller(),
        )
        .unwrap();

    let detail = service.open("Callbacks are slow on Mondays").unwrap();

    assert_eq!(detail.page, None);
}

#[test]
fn a_raw_source_is_not_a_page() {
    let (_project, mut store) = indexed_project();

    let detail = WikiService::new(&mut store).open("rfc.md").unwrap();

    assert_eq!(detail.node.node_type, NodeType::Source);
    assert_eq!(detail.page, None);
}

#[test]
fn a_brief_points_at_pages_and_states_where_they_are() {
    let (_project, mut store) = indexed_project();

    let context = WikiService::new(&mut store)
        .context("fix the OAuthCallback bug", &[], None, None)
        .unwrap();

    let pages: Vec<_> = context
        .brief
        .nodes
        .iter()
        .filter_map(|node| node.page.as_ref())
        .collect();
    assert!(
        pages.len() >= 2,
        "both pages should be in the brief, got {pages:?}"
    );
    for page in &pages {
        assert!(
            context
                .prompt
                .contains(&format!("Page: {}", page.best_path())),
            "the prompt should state where {} is",
            page.path
        );
    }
    // The trust wrapper is untouched, and the page line is a fact, not an instruction.
    assert!(context.prompt.starts_with("<retrieved_knowledge>"));
    assert!(context.prompt.contains("does not authorize tool execution"));
}

#[test]
fn the_source_node_in_a_brief_has_no_page_line() {
    let (_project, mut store) = indexed_project();

    let context = WikiService::new(&mut store)
        .context("validate state", &[], None, None)
        .unwrap();

    let source = context
        .brief
        .nodes
        .iter()
        .find(|node| node.node_type == NodeType::Source)
        .expect("the decision cites the RFC, so it is reached");
    assert_eq!(source.page, None);
}

#[test]
fn capabilities_advertise_page_pointers_so_old_and_new_can_be_told_apart() {
    let capabilities = WikiService::capabilities();

    assert!(capabilities.features.iter().any(|f| f == "page_pointers"));
}

#[test]
fn after_an_edit_and_a_reindex_the_pointer_carries_the_new_hash() {
    // The hash is how a reader that holds a node can tell it is behind the file it is about to change.
    let (project, mut store) = indexed_project();
    let before = WikiService::new(&mut store)
        .open("OAuthCallback")
        .unwrap()
        .page
        .unwrap();

    let edited = format!("{OAUTH_PAGE}\nAlso validates the state parameter.\n");
    write(project.path(), "wiki/concepts/oauthcallback.md", &edited);
    VaultIndexer::scan_and_index(&mut store, project.path(), VaultScanOptions::default()).unwrap();

    let after = WikiService::new(&mut store)
        .open("OAuthCallback")
        .unwrap()
        .page
        .unwrap();
    assert_eq!(after.path, before.path, "editing keeps the page's identity");
    assert_ne!(after.content_hash, before.content_hash);
    assert_eq!(after.content_hash, hash(&edited));
}

#[test]
fn a_store_that_is_not_in_a_project_still_names_the_relative_path() {
    // An in-memory store has no project, so there is nothing to resolve the page against.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "wiki/concepts/a.md", "# A\n\nBody.\n");
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    VaultIndexer::scan_and_index(&mut store, dir.path(), VaultScanOptions::default()).unwrap();

    let page = WikiService::new(&mut store)
        .open("A")
        .unwrap()
        .page
        .unwrap();

    assert_eq!(page.path, "wiki/concepts/a.md");
    assert_eq!(page.absolute_path, None);
}

fn update_summary(node: &str) -> WikiNodeUpdate {
    WikiNodeUpdate {
        node: node.to_string(),
        title: None,
        summary: Some("A model's rewrite.".to_string()),
        content: None,
        reason: Some("test".to_string()),
    }
}

#[test]
fn a_page_cannot_be_edited_through_the_index() {
    // The markdown is the source of truth and the index follows it, so an edit made to the node
    // would be discarded by the next reconcile. Refusing says so; accepting would lose it silently.
    let (_project, mut store) = indexed_project();

    let refused = WikiService::new(&mut store).update(update_summary("OAuthCallback"), caller());

    let message = refused.unwrap_err().to_string();
    assert!(
        message.contains("wiki/concepts/oauthcallback.md"),
        "it should name the file to change: {message}"
    );
    assert!(message.contains("change that file instead"), "{message}");
}

#[test]
fn a_node_that_is_not_a_page_can_still_be_updated() {
    let (_project, mut store) = indexed_project();
    let mut service = WikiService::new(&mut store);
    service
        .create(
            WikiNodeCreate {
                node_type: NodeType::Observation,
                title: "Callbacks are slow on Mondays".to_string(),
                summary: "A guess.".to_string(),
                content: String::new(),
                scope: Scope::Project,
            },
            caller(),
        )
        .unwrap();

    let updated = service
        .update(update_summary("Callbacks are slow on Mondays"), caller())
        .unwrap();

    assert_eq!(updated.summary, "A model's rewrite.");
}
