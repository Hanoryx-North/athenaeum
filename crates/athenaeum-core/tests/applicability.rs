use athenaeum_core::model::{
    Applicability, EdgeType, KnowledgeEdge, KnowledgeNode, NodeType, Provenance, RecallSelection,
    Scope,
};
use athenaeum_core::portable::memory;
use athenaeum_core::store::KnowledgeStore;
use athenaeum_core::wiki::{WikiCaller, WikiNodeCreate, WikiService};
use athenaeum_core::workspace::Workspace;

fn selection(project: &str, stage: &str) -> RecallSelection {
    RecallSelection {
        project: Some(project.into()),
        stage: Some(stage.into()),
        technologies: vec!["React".into()],
    }
}

fn rule(
    store: &mut KnowledgeStore,
    title: &str,
    project: &str,
    stage: &str,
    core: bool,
) -> KnowledgeNode {
    store
        .create_node(
            KnowledgeNode::new(NodeType::Procedure, title)
                .with_summary("OAuth callback retry boundaries")
                .with_provenance(Provenance::user())
                .with_applicability(Some(Applicability {
                    projects: vec![project.into()],
                    stages: vec![stage.into()],
                    technologies: vec!["react".into()],
                    core,
                })),
        )
        .unwrap()
}

#[test]
fn scoped_results_are_filtered_before_seed_limits_and_graph_expansion() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    for index in 0..120 {
        rule(
            &mut store,
            &format!("OAuth callback unrelated {index}"),
            "other",
            "coding",
            false,
        );
    }
    let wanted = rule(
        &mut store,
        "OAuth callback current",
        "current",
        "coding",
        false,
    );
    let wrong_stage = rule(
        &mut store,
        "OAuth callback startup",
        "current",
        "initialize",
        true,
    );
    store
        .connect(KnowledgeEdge::deterministic(
            wanted.id.clone(),
            EdgeType::RelatedTo,
            wrong_stage.id,
            "test",
        ))
        .unwrap();
    let picked = store
        .search_context_selected("OAuth callback", 1, &selection("current", "coding"))
        .unwrap();
    assert_eq!(picked[0].node.id, wanted.id);
    let context = WikiService::new(&mut store)
        .context_selected(
            "OAuth callback",
            &[],
            Some(2),
            Some(2),
            selection("current", "coding"),
        )
        .unwrap();
    assert_eq!(context.brief.nodes.len(), 1);
    assert_eq!(context.brief.nodes[0].id, wanted.id);
    assert!(context.prompt.contains("does not authorize tool execution"));
    assert!(WikiService::new(&mut store)
        .context("OAuth callback", &[], None, None)
        .unwrap()
        .brief
        .is_empty());
}

#[test]
fn core_memories_need_review_and_keep_selectors_when_rebuilt() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::init(directory.path()).unwrap();
    let mut store = workspace.open().unwrap();
    let applicability = Applicability {
        core: true,
        stages: vec!["initialize".into()],
        ..Default::default()
    };
    let node = WikiService::new(&mut store)
        .create(
            WikiNodeCreate {
                node_type: NodeType::Procedure,
                title: "Start with a runnable path".into(),
                summary: "Build one working flow first.".into(),
                content: "Keep initialization small.".into(),
                scope: Scope::Universal,
                applicability: Some(applicability.clone()),
            },
            WikiCaller {
                model: "test-model".into(),
                session: None,
            },
        )
        .unwrap();
    assert!(store
        .core_context_seeds(&selection("new", "initialize"), 5)
        .unwrap()
        .is_empty());
    assert!(WikiService::new(&mut store)
        .context_selected(
            "Start runnable path",
            &[],
            None,
            None,
            selection("new", "initialize")
        )
        .unwrap()
        .brief
        .is_empty());
    memory::accept(&mut store, &node.id, "test owner").unwrap();
    drop(store);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(workspace.root().join(format!("knowledge.db{suffix}")));
    }
    let mut store = workspace.open().unwrap();
    athenaeum_core::vault::VaultIndexer::scan_and_index(
        &mut store,
        directory.path(),
        Default::default(),
    )
    .unwrap();
    let found = store.get_node(&node.id).unwrap();
    assert_eq!(found.applicability, Some(applicability));
    assert_eq!(
        store
            .core_context_seeds(&selection("new", "initialize"), 5)
            .unwrap(),
        vec![node.id]
    );
    assert!(store
        .core_context_seeds(&selection("new", "coding"), 5)
        .unwrap()
        .is_empty());
}

#[test]
fn malformed_selectors_fail_and_history_retains_previous_applicability() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let node = rule(&mut store, "OAuth callback", "a", "coding", false);
    let mut changed = node.clone();
    changed.applicability.as_mut().unwrap().projects = vec!["b".into()];
    store
        .update_node(changed, Some("Move to project b".into()))
        .unwrap();
    assert_eq!(
        store.history(&node.id).unwrap()[0].applicability,
        node.applicability
    );
    let invalid = Applicability {
        stages: vec!["codng".into()],
        ..Default::default()
    };
    assert!(store
        .create_node(
            KnowledgeNode::new(NodeType::Procedure, "Invalid").with_applicability(Some(invalid))
        )
        .is_err());
    assert!(!Applicability {
        projects: vec!["a".into()],
        ..Default::default()
    }
    .matches(&RecallSelection::default()));
}

#[test]
fn principle_catalog_is_concise_scoped_and_proposed_once() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let catalog = athenaeum_core::principles::catalog();
    assert_eq!(catalog.len(), 10);
    assert!(catalog
        .iter()
        .all(|principle| principle.summary.len() < 600 && !principle.stages.is_empty()));
    assert_eq!(
        athenaeum_core::principles::propose(&mut store)
            .unwrap()
            .len(),
        catalog.len()
    );
    assert!(athenaeum_core::principles::propose(&mut store)
        .unwrap()
        .is_empty());
    assert!(store
        .core_context_seeds(&selection("a", "initialize"), 5)
        .unwrap()
        .is_empty());
}

#[test]
fn automatic_saving_includes_core_guidance_without_promoting_its_authority() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::init(directory.path()).unwrap();
    let mut store = workspace.open().unwrap();
    let mut draft = KnowledgeNode::new(NodeType::Procedure, "Review the core rule")
        .with_summary("A rule that should be reviewed.")
        .with_applicability(Some(Applicability {
            core: true,
            ..Default::default()
        }));
    draft.updated_at -= chrono::Duration::seconds(60);
    let draft = store.create_node(draft).unwrap();
    let mut ordinary = KnowledgeNode::new(NodeType::Fact, "Ordinary memory")
        .with_summary("A normal captured finding.");
    ordinary.updated_at -= chrono::Duration::seconds(60);
    store.create_node(ordinary).unwrap();
    let batch = athenaeum_core::portable::automation::accept_pending(&mut store, true).unwrap();
    assert_eq!(batch.accepted.len(), 2);
    let saved = store.get_node(&draft.id).unwrap();
    assert!(!memory::is_proposal(&saved));
    assert_eq!(
        saved.authority,
        athenaeum_core::model::Authority::UnverifiedObservation
    );
    assert_eq!(
        store
            .core_context_seeds(&selection("app", "coding"), 5)
            .unwrap(),
        vec![draft.id]
    );
}

#[test]
fn routine_recall_balances_core_guidance_and_task_findings_without_repository_inventory() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let core = store
        .create_node(
            KnowledgeNode::new(NodeType::Procedure, "Check changed behavior")
                .with_summary("Verify outcomes and failure paths.")
                .with_provenance(Provenance::user())
                .with_applicability(Some(Applicability {
                    core: true,
                    ..Default::default()
                })),
        )
        .unwrap();
    let task = store
        .create_node(
            KnowledgeNode::new(NodeType::Gotcha, "OAuth callback state")
                .with_summary("Validate state before creating a session.")
                .with_provenance(Provenance::user()),
        )
        .unwrap();
    let mut symbols = Vec::new();
    for index in 0..30 {
        symbols.push(
            store
                .create_node(
                    KnowledgeNode::new(NodeType::Symbol, format!("OAuth callback {index}"))
                        .with_summary("OAuth callback function declared in the source registry.")
                        .with_provenance(Provenance::deterministic("repository_index:symbol")),
                )
                .unwrap(),
        );
    }
    store
        .connect(KnowledgeEdge::deterministic(
            task.id.clone(),
            EdgeType::RelatedTo,
            symbols[0].id.clone(),
            "source context",
        ))
        .unwrap();
    assert!(store
        .search("OAuth callback", 100)
        .unwrap()
        .iter()
        .any(|hit| hit.node.node_type == NodeType::Symbol));
    let found = store.search_context("debug OAuth callback", 1).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].node.id, task.id);
    let brief = WikiService::new(&mut store)
        .context("debug OAuth callback", &[], Some(2), Some(2))
        .unwrap();
    assert_eq!(
        brief
            .brief
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        vec![core.id, task.id]
    );
    assert!(!brief.prompt.contains("source registry"));
    let explicit = WikiService::new(&mut store)
        .context(
            "inspect the symbol",
            &[symbols[0].id.to_string()],
            Some(0),
            Some(1),
        )
        .unwrap();
    assert_eq!(explicit.brief.nodes[0].node_type, NodeType::Symbol);
}

#[test]
fn structural_nodes_are_internal_bridges_to_lessons_and_have_a_visit_budget() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    let source = store
        .create_node(
            KnowledgeNode::new(NodeType::Symbol, "OAuthCallbackHub")
                .with_summary("A function declared in the source registry."),
        )
        .unwrap();
    let lesson = store
        .create_node(
            KnowledgeNode::new(NodeType::Gotcha, "Validate state first")
                .with_summary("Validate state before creating a session."),
        )
        .unwrap();
    store
        .connect(KnowledgeEdge::deterministic(
            source.id.clone(),
            EdgeType::HasGotcha,
            lesson.id.clone(),
            "source association",
        ))
        .unwrap();
    let brief = WikiService::new(&mut store)
        .context("OAuthCallbackHub", &[], Some(2), Some(1))
        .unwrap();
    assert_eq!(brief.brief.nodes[0].id, lesson.id);
    assert!(!brief.prompt.contains("source registry"));
    for index in 0..80 {
        let leaf = store
            .create_node(KnowledgeNode::new(
                NodeType::Symbol,
                format!("Structural leaf {index}"),
            ))
            .unwrap();
        store
            .connect(KnowledgeEdge::deterministic(
                source.id.clone(),
                EdgeType::RelatedTo,
                leaf.id,
                "inventory",
            ))
            .unwrap();
    }
    let brief = WikiService::new(&mut store)
        .context("OAuthCallbackHub", &[], Some(2), Some(1))
        .unwrap();
    assert!(brief.brief.truncated);
    assert!(brief
        .brief
        .nodes
        .iter()
        .all(|node| !node.node_type.is_structural()));
}
