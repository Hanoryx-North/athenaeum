//! Structural validation of a knowledge base (buildplan §79).
//!
//! Reports knowledge that has drifted into a state a user would want to know
//! about. Kept separate from the command dispatch because these are rules
//! about knowledge quality, not about argument parsing — and because they are
//! the part worth testing.

use athenaeum_core::prelude::*;
use serde::Serialize;

/// One thing wrong with the knowledge base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub node: NodeId,
    pub title: String,
    /// What is wrong, phrased for a human reading a terminal.
    pub problem: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.title.is_empty() {
            write!(f, "{}: {}", self.node, self.problem)
        } else {
            write!(f, "{} ({}): {}", self.node, self.title, self.problem)
        }
    }
}

/// Every problem in the knowledge base, in node order.
pub fn run(store: &KnowledgeStore) -> Result<Vec<Problem>> {
    let mut problems = Vec::new();

    for node in store.list_nodes(&NodeFilter::all())? {
        let mut note = |problem: String| {
            problems.push(Problem {
                node: node.id.clone(),
                title: node.title.clone(),
                problem,
            })
        };

        if node.title.trim().is_empty() {
            note("has no title".to_string());
        }
        if node.summary.trim().is_empty() && node.content.trim().is_empty() {
            note("has no content at all".to_string());
        }
        if node.needs_review() {
            note(format!("is {} and needs review", node.status));
        }
        // A rule or decision nobody can justify is the kind of knowledge that
        // quietly becomes wrong (§15).
        if matches!(node.node_type, NodeType::Rule | NodeType::Decision)
            && store.evidence_for(&node.id)?.is_empty()
        {
            note(format!("is a {} with no evidence", node.node_type));
        }
    }

    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> KnowledgeStore {
        KnowledgeStore::open_in_memory().unwrap()
    }

    #[test]
    fn a_healthy_knowledge_base_has_nothing_to_report() {
        let mut store = store();
        let node = store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Use SQLite").with_summary("Local-first."),
            )
            .unwrap();
        store
            .attach_evidence(
                Evidence::new(node.id, EvidenceKind::SourceFile)
                    .with_anchor(SourceAnchor::file("Cargo.toml")),
            )
            .unwrap();

        assert_eq!(run(&store).unwrap(), Vec::new());
    }

    #[test]
    fn a_decision_with_no_evidence_is_reported() {
        let mut store = store();
        store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Use SQLite").with_summary("Local-first."),
            )
            .unwrap();

        let problems = run(&store).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].problem.contains("no evidence"), "{problems:?}");
        assert!(problems[0].to_string().contains("Use SQLite"));
    }

    #[test]
    fn an_observation_with_no_evidence_is_not_reported() {
        // Only rules and decisions are held to the evidence bar; an
        // observation is allowed to be just an observation (§15).
        let mut store = store();
        store
            .create_node(KnowledgeNode::new(NodeType::Observation, "Looks slow").with_summary("s"))
            .unwrap();
        assert!(run(&store).unwrap().is_empty());
    }

    #[test]
    fn an_empty_node_is_reported() {
        let mut store = store();
        store
            .create_node(KnowledgeNode::new(NodeType::Concept, "Nothing here"))
            .unwrap();

        let problems = run(&store).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].problem.contains("no content"), "{problems:?}");
    }

    #[test]
    fn contradicted_knowledge_is_flagged_for_review() {
        let mut store = store();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Fact, "Sessions last 24h").with_summary("s"))
            .unwrap();
        store
            .set_node_status(&node.id, NodeStatus::Contradicted)
            .unwrap();

        let problems = run(&store).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].problem.contains("needs review"), "{problems:?}");
    }

    #[test]
    fn archived_knowledge_is_still_inspected() {
        // `NodeFilter::all()`, not `live()`: a check that skipped archived
        // nodes could not report anything about the history it retains.
        let mut store = store();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Rule, "Old rule").with_summary("s"))
            .unwrap();
        store
            .set_node_status(&node.id, NodeStatus::Archived)
            .unwrap();

        assert_eq!(run(&store).unwrap().len(), 1);
    }
}
