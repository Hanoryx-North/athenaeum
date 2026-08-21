//! The acceptance test for the whole design (buildplan §90).
//!
//! Session 1 discovers knowledge and stores it. Session 2 needs the same
//! information and finds it by navigating, without rediscovering anything.
//! If this file fails, the product does not work, however good the graph
//! visualization looks.
//!
//! The scenario is the plan's own worked example (§26): fixing an OAuth
//! callback bug in a project whose conventions were learned earlier.

use athenaeum_core::prelude::*;
use athenaeum_core::{authority, graph};

/// Build the graph a first session would leave behind.
///
/// Deliberately written the way a curator would write it: concepts and
/// decisions as nodes, relationships between them, evidence pointing at real
/// source locations.
fn session_one(store: &mut KnowledgeStore) -> Result<()> {
    let session = SessionId::new();
    let by_agent = Provenance::model("claude-haiku-4-5").with_session(session.clone());

    let oauth = store.create_node(
        KnowledgeNode::new(NodeType::Concept, "OAuth")
            .with_summary("Third-party sign-in, handled through a redirect callback.")
            .with_provenance(by_agent.clone()),
    )?;

    let callback = store.create_node(
        KnowledgeNode::new(NodeType::Symbol, "OAuthCallback.handle")
            .with_summary(
                "Entry point for the provider redirect. Validates state, then creates the session.",
            )
            .with_authority(Authority::VerifiedEvidence)
            .with_provenance(Provenance::deterministic("ast scan")),
    )?;

    let auth_service = store.create_node(
        KnowledgeNode::new(NodeType::Concept, "AuthService")
            .with_summary("Token validation and session creation.")
            .with_authority(Authority::VerifiedEvidence)
            .with_provenance(by_agent.clone()),
    )?;

    let decision = store.create_node(
        KnowledgeNode::new(NodeType::Decision, "Decision_042")
            .with_id("Decision_042")
            .with_summary("OAuth state is validated before any session is created.")
            .with_authority(Authority::ProjectDecision)
            .with_provenance(Provenance::user()),
    )?;

    let gotcha = store.create_node(
        KnowledgeNode::new(NodeType::Gotcha, "Gotcha_017")
            .with_id("Gotcha_017")
            .with_summary(
                "Do not create a session before state validation — it opens a login CSRF.",
            )
            .with_authority(Authority::ProjectRule)
            .with_provenance(Provenance::user()),
    )?;

    let test = store.create_node(
        KnowledgeNode::new(NodeType::Test, "OAuthCallbackTest")
            .with_summary("Covers the expected callback flow, including the state mismatch case.")
            .with_provenance(Provenance::deterministic("test discovery")),
    )?;

    // Deterministic relationships — read out of the code, not guessed.
    store.connect(KnowledgeEdge::deterministic(
        oauth.id.clone(),
        EdgeType::ImplementedBy,
        callback.id.clone(),
        "symbol scan",
    ))?;
    store.connect(KnowledgeEdge::deterministic(
        callback.id.clone(),
        EdgeType::DependsOn,
        auth_service.id.clone(),
        "import",
    ))?;
    store.connect(KnowledgeEdge::deterministic(
        callback.id.clone(),
        EdgeType::VerifiedBy,
        test.id.clone(),
        "test references symbol",
    ))?;
    store.connect(KnowledgeEdge::deterministic(
        callback.id.clone(),
        EdgeType::ConstrainedBy,
        decision.id.clone(),
        "user stated",
    ))?;
    store.connect(KnowledgeEdge::deterministic(
        oauth.id.clone(),
        EdgeType::HasGotcha,
        gotcha.id.clone(),
        "user stated",
    ))?;

    // Evidence: why any of this is believed.
    store.attach_evidence(
        Evidence::new(callback.id.clone(), EvidenceKind::Symbol)
            .with_anchor(
                SourceAnchor::file("src/auth/oauth_callback.ts")
                    .with_symbol("OAuthCallback.handle")
                    .with_lines(24, 61)
                    .with_commit("abc123"),
            )
            .with_excerpt("if (!stateMatches(req)) throw new InvalidState();"),
    )?;
    store.attach_evidence(
        Evidence::new(test.id.clone(), EvidenceKind::Test)
            .with_anchor(SourceAnchor::file("src/auth/oauth_callback.test.ts")),
    )?;
    store.attach_evidence(
        Evidence::new(decision.id.clone(), EvidenceKind::ArchitecturalDecision)
            .with_note("ADR-042"),
    )?;

    Ok(())
}

#[test]
fn session_two_finds_what_session_one_learned() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    // Session 2 begins with nothing but a task, exactly as a fresh agent
    // process would. It has no memory of session 1.
    let task = "Fix the OAuth callback bug";

    // Step 1: find seed nodes lexically.
    let hits = store.search("OAuth", 5).unwrap();
    assert!(
        !hits.is_empty(),
        "search found no seed for a term the graph knows"
    );
    let seeds: Vec<NodeId> = hits.iter().map(|h| h.node.id.clone()).collect();

    // Step 2: navigate outward and assemble a brief.
    let brief = KnowledgeBrief::assemble(
        &mut store,
        task,
        &seeds,
        &NavigationOptions::default().depth(3),
    )
    .unwrap();

    let titles: Vec<&str> = brief.nodes.iter().map(|n| n.title.as_str()).collect();

    // Everything the plan says the primary agent should receive (§26).
    for expected in [
        "OAuth",
        "OAuthCallback.handle",
        "AuthService",
        "Decision_042",
        "Gotcha_017",
        "OAuthCallbackTest",
    ] {
        assert!(
            titles.contains(&expected),
            "brief is missing {expected}: {titles:?}"
        );
    }

    // The gotcha must arrive as a warning, not buried in prose — the entire
    // point is that session 2 does not re-introduce the CSRF bug.
    assert!(
        brief.warnings.iter().any(|w| w.contains("Gotcha_017")),
        "the known trap was not surfaced as a warning: {:?}",
        brief.warnings
    );

    // And it must point at real source rather than describing it.
    assert!(
        brief
            .anchors
            .iter()
            .any(|a| a.contains("oauth_callback.ts")),
        "no source anchor to open: {:?}",
        brief.anchors
    );
}

#[test]
fn the_brief_stays_small_as_the_graph_grows() {
    // The value proposition is context efficiency (§87). A brief that grows
    // with the database is just a slower way of reading everything.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let seed = store.find_node_by_title("OAuth").unwrap().unwrap();

    let before = KnowledgeBrief::assemble(
        &mut store,
        "task",
        std::slice::from_ref(&seed.id),
        &NavigationOptions::default(),
    )
    .unwrap();

    // Add 200 unrelated nodes, as months of unrelated sessions would.
    for i in 0..200 {
        store
            .create_node(KnowledgeNode::new(
                NodeType::Fact,
                format!("Unrelated fact {i}"),
            ))
            .unwrap();
    }

    let after = KnowledgeBrief::assemble(
        &mut store,
        "task",
        &[seed.id],
        &NavigationOptions::default(),
    )
    .unwrap();

    assert_eq!(
        before.nodes.len(),
        after.nodes.len(),
        "unrelated knowledge leaked into the brief"
    );
}

#[test]
fn navigation_reaches_further_than_search_alone() {
    // This is the difference from plain top-k retrieval (§54): "Decision_042"
    // does not contain the word "OAuth", so no amount of lexical or semantic
    // similarity to the task finds it. The relationship does.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let searched: Vec<String> = store
        .search("OAuth", 20)
        .unwrap()
        .into_iter()
        .map(|h| h.node.title)
        .collect();
    assert!(
        !searched.contains(&"AuthService".to_string()),
        "precondition failed: search already finds AuthService"
    );

    let seed = store.find_node_by_title("OAuth").unwrap().unwrap();
    let reached: Vec<String> =
        graph::navigate(&store, &[seed.id], &NavigationOptions::default().depth(3))
            .unwrap()
            .into_iter()
            .map(|r| r.node.title)
            .collect();

    assert!(
        reached.contains(&"AuthService".to_string()),
        "navigation failed to reach a node only connected by relationship"
    );
}

#[test]
fn knowledge_survives_closing_and_reopening_the_application() {
    // §63: closing the app, or switching providers, must not lose knowledge.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".athenaeum");

    {
        let mut store = KnowledgeStore::open(&root).unwrap();
        session_one(&mut store).unwrap();
    } // dropped: connection closed, as on app exit

    let store = KnowledgeStore::open(&root).unwrap();
    let recovered = store.find_node_by_title("Decision_042").unwrap();
    assert!(recovered.is_some(), "knowledge did not survive a restart");
    assert!(store.stats().unwrap().edges >= 5);
}

#[test]
fn relationships_are_navigable_from_both_ends() {
    // §35: reaching the test from the symbol must not be a one-way trip.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let callback = store
        .find_node_by_title("OAuthCallback.handle")
        .unwrap()
        .unwrap();
    let test = store
        .find_node_by_title("OAuthCallbackTest")
        .unwrap()
        .unwrap();

    let forward = store
        .neighbors(&callback.id, &EdgeFilter::traversable())
        .unwrap();
    let to_test = forward
        .iter()
        .find(|n| n.node == test.id)
        .expect("no edge to the test");
    assert_eq!(to_test.edge_type, EdgeType::VerifiedBy);
    assert!(to_test.outbound);

    let backward = store
        .neighbors(&test.id, &EdgeFilter::traversable())
        .unwrap();
    let to_callback = backward
        .iter()
        .find(|n| n.node == callback.id)
        .expect("cannot get back from the test");
    // Same stored row, inverted for the direction of travel.
    assert_eq!(to_callback.edge_type, EdgeType::Verifies);
    assert!(!to_callback.outbound);
}

#[test]
fn a_path_explains_how_two_things_are_related() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let oauth = store.find_node_by_title("OAuth").unwrap().unwrap();
    let service = store.find_node_by_title("AuthService").unwrap().unwrap();

    let path = graph::shortest_path(
        &store,
        &oauth.id,
        &service.id,
        &NavigationOptions::default().depth(4),
    )
    .unwrap()
    .expect("no path between two connected nodes");

    assert_eq!(path.len(), 2, "expected OAuth → callback → AuthService");
    assert_eq!(path[0].edge_type, EdgeType::ImplementedBy);
    assert_eq!(path[1].edge_type, EdgeType::DependsOn);
}

#[test]
fn superseded_knowledge_leaves_the_briefs_but_not_the_database() {
    // §17/§36: old knowledge is history, not garbage.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let old = store.find_node_by_title("Decision_042").unwrap().unwrap();
    let new = store
        .create_node(
            KnowledgeNode::new(NodeType::Decision, "Decision_051")
                .with_summary("OAuth now uses PKCE; state validation still precedes the session.")
                .with_authority(Authority::ProjectDecision),
        )
        .unwrap();

    store.supersede(&old.id, &new.id).unwrap();

    // Gone from retrieval...
    let hits = store.search("Decision_042", 10).unwrap();
    assert!(
        hits.iter().all(|h| h.node.id != old.id),
        "superseded node still retrieved"
    );

    // ...but still on disk, and still readable.
    let still_there = store.get_node(&old.id).unwrap();
    assert_eq!(still_there.status, NodeStatus::Superseded);
}

#[test]
fn editing_a_node_preserves_what_it_used_to_say() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let mut decision = store.find_node_by_title("Decision_042").unwrap().unwrap();
    assert_eq!(decision.version, 1);

    decision.content = "Updated to support PKCE.".into();
    let updated = store
        .update_node(decision, Some("PKCE migration".into()))
        .unwrap();

    assert_eq!(updated.version, 2);

    let history = store.history(&updated.id).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].version, 1);
    assert_eq!(history[0].reason.as_deref(), Some("PKCE migration"));
}

#[test]
fn a_project_rule_beats_a_global_preference() {
    // §14, resolved against the real store rather than in the abstract.
    let mut store = KnowledgeStore::open_in_memory().unwrap();

    store
        .create_node(
            KnowledgeNode::new(NodeType::Preference, "Database")
                .with_summary("Use PostgreSQL.")
                .with_authority(Authority::UserPreference)
                .with_scope(Scope::Universal),
        )
        .unwrap();
    store
        .create_node(
            KnowledgeNode::new(NodeType::Rule, "Database")
                .with_summary("This project uses SQLite.")
                .with_authority(Authority::ProjectRule)
                .with_scope(Scope::Project),
        )
        .unwrap();

    let candidates = store
        .list_nodes(&NodeFilter::live())
        .unwrap()
        .into_iter()
        .filter(|n| n.title == "Database")
        .collect::<Vec<_>>();

    let governing = authority::resolve(&candidates).unwrap();
    assert_eq!(governing.summary, "This project uses SQLite.");
}

#[test]
fn changing_a_file_locates_the_knowledge_that_may_now_be_wrong() {
    // §17: staleness detection has to be cheap, or it never runs.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let affected = store
        .nodes_citing_file("src/auth/oauth_callback.ts")
        .unwrap();
    assert_eq!(affected.len(), 1);

    let node = store.get_node(&affected[0]).unwrap();
    assert_eq!(node.title, "OAuthCallback.handle");

    let stale = store.set_node_status(&node.id, NodeStatus::Stale).unwrap();
    assert_eq!(stale.status, NodeStatus::Stale);
    // Still live: staleness is not falsehood.
    assert!(stale.is_live());
}

#[test]
fn retrieval_is_measured_rather_than_assumed() {
    // §41: the system must be able to tell whether a node ever helped.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let oauth = store.find_node_by_title("OAuth").unwrap().unwrap();
    assert_eq!(store.utility(&oauth.id).unwrap().retrievals, 0);

    KnowledgeBrief::assemble(
        &mut store,
        "task",
        std::slice::from_ref(&oauth.id),
        &NavigationOptions::default(),
    )
    .unwrap();

    let utility = store.utility(&oauth.id).unwrap();
    assert_eq!(utility.retrievals, 1);
    assert_eq!(
        utility.uses, 0,
        "retrieval must not be mistaken for usefulness"
    );

    store
        .record(NewEvent::about(EventKind::NodeUsed, &oauth.id))
        .unwrap();
    assert_eq!(store.utility(&oauth.id).unwrap().use_rate(), Some(1.0));
}

#[test]
fn model_proposals_do_not_silently_become_facts() {
    // §20: an inferred edge is stored, but not followed until promoted.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let oauth = store.find_node_by_title("OAuth").unwrap().unwrap();
    let service = store.find_node_by_title("AuthService").unwrap().unwrap();

    let proposed = store
        .relate(&oauth.id, EdgeType::RelatedTo, &service.id)
        .unwrap();
    assert_eq!(proposed.status, NodeStatus::Proposed);

    let followed = store
        .neighbors(&oauth.id, &EdgeFilter::traversable())
        .unwrap();
    assert!(
        !followed.iter().any(|n| n.edge.id == proposed.id),
        "a model proposal was traversed as though it were established"
    );

    // Reviewed and accepted, it becomes navigable.
    store.promote_edge(&proposed.id).unwrap();
    let after = store
        .neighbors(&oauth.id, &EdgeFilter::traversable())
        .unwrap();
    assert!(after.iter().any(|n| n.edge.id == proposed.id));
}

#[test]
fn the_same_relationship_asserted_twice_is_stored_once() {
    // §21/§40: repeated sessions must not grow a hairball.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();
    let edges_before = store.stats().unwrap().edges;

    let oauth = store.find_node_by_title("OAuth").unwrap().unwrap();
    let callback = store
        .find_node_by_title("OAuthCallback.handle")
        .unwrap()
        .unwrap();

    for _ in 0..5 {
        store
            .connect(KnowledgeEdge::deterministic(
                oauth.id.clone(),
                EdgeType::ImplementedBy,
                callback.id.clone(),
                "symbol scan",
            ))
            .unwrap();
    }

    assert_eq!(store.stats().unwrap().edges, edges_before);
}

#[test]
fn the_search_index_can_be_rebuilt_from_the_nodes() {
    // §60: derived data must be reconstructible, or the system is fragile.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    store
        .connection()
        .execute("DELETE FROM nodes_fts", [])
        .unwrap();
    assert!(store.search("OAuth", 5).unwrap().is_empty());

    store.rebuild_search_index().unwrap();
    assert!(!store.search("OAuth", 5).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Regressions found by driving the CLI against a real knowledge base.
// ---------------------------------------------------------------------------

#[test]
fn a_task_sentence_finds_seeds_even_though_no_node_contains_every_word() {
    // Regression: requiring every term meant the brief for "Fix the OAuth
    // callback bug" came back empty, because no node contains "fix", "the"
    // and "bug". Briefs are always seeded from task text, so this made the
    // primary entry point return nothing.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let hits = store.search("Fix the OAuth callback bug", 5).unwrap();
    assert!(!hits.is_empty(), "a natural-language task found no seeds");

    let titles: Vec<&str> = hits.iter().map(|h| h.node.title.as_str()).collect();
    assert!(
        titles.contains(&"OAuth"),
        "the most obviously relevant node was not among the seeds: {titles:?}"
    );
}

#[test]
fn requiring_every_term_still_wins_when_something_matches_them_all() {
    // The widening above must not cost precision: when a node does match
    // every term, loose matches must not be mixed in alongside it.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();
    store
        .create_node(
            KnowledgeNode::new(NodeType::Fact, "Session expiry")
                .with_summary("Sessions expire after 24 hours."),
        )
        .unwrap();

    let hits = store.search("session creation", 10).unwrap();
    assert!(!hits.is_empty());
    for hit in &hits {
        let text = format!("{} {}", hit.node.title, hit.node.summary).to_lowercase();
        assert!(
            text.contains("session") && text.contains("creat"),
            "{} matched only some terms while a full match existed",
            hit.node.title
        );
    }
}

#[test]
fn the_brief_states_relationships_between_the_nodes_it_includes() {
    // Regression: only the edge a breadth-first walk arrived by was reported,
    // so a brief whose nodes were all seeds listed no relationships at all —
    // dropping precisely the structure that distinguishes this from top-k.
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    // Seed with everything, so no relationship is a traversal tree edge.
    let seeds: Vec<NodeId> = store
        .list_nodes(&NodeFilter::live())
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();

    let brief = KnowledgeBrief::assemble(
        &mut store,
        "understand OAuth",
        &seeds,
        &NavigationOptions::default(),
    )
    .unwrap();

    let rendered: Vec<String> = brief
        .relationships
        .iter()
        .map(|r| format!("{} {} {}", r.from, r.edge_type, r.to))
        .collect();

    for expected in [
        "OAuthCallback.handle verified_by OAuthCallbackTest",
        "OAuthCallback.handle constrained_by Decision_042",
        "OAuth has_gotcha Gotcha_017",
    ] {
        assert!(
            rendered.iter().any(|r| r == expected),
            "brief omitted {expected}: {rendered:?}"
        );
    }
}

#[test]
fn each_relationship_is_stated_once_rather_than_from_both_ends() {
    let mut store = KnowledgeStore::open_in_memory().unwrap();
    session_one(&mut store).unwrap();

    let seeds: Vec<NodeId> = store
        .list_nodes(&NodeFilter::live())
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();

    let brief = KnowledgeBrief::assemble(&mut store, "task", &seeds, &NavigationOptions::default())
        .unwrap();

    let mut seen = std::collections::HashSet::new();
    for relation in &brief.relationships {
        let key = format!("{}|{}|{}", relation.from, relation.edge_type, relation.to);
        assert!(seen.insert(key.clone()), "relationship listed twice: {key}");
    }
    // And the same edge must not also appear inverted.
    for relation in &brief.relationships {
        let inverted = format!(
            "{}|{}|{}",
            relation.to,
            relation.edge_type.inverse(),
            relation.from
        );
        assert!(
            !seen.contains(&inverted),
            "the same edge was stated from both ends: {inverted}"
        );
    }
}
