//! The knowledge graph, projected for a graph view.
//!
//! The app used to draw its graph by re-reading every markdown file and
//! re-extracting every `[[wikilink]]` on the way to the screen. That made
//! two knowledge models: one the graph believed and one the knowledge
//! library believed, differing in exactly the places that matter — typed
//! relationships, evidence, and anything the user or the agent added that
//! is not a file.
//!
//! This is the single model, read once. It is a projection rather than a
//! second store: everything here is derived from `nodes` and `edges`, and
//! the projection is read-only.
//!
//! What the view still owns is layout — community detection, force
//! placement, weighting, filtering. Those are presentation, they change
//! whenever the visual design changes, and none of them belong in a
//! library that must stay free of the UI.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::NodeId;
use crate::model::{Authority, EdgeType, NodeStatus, NodeType};
use crate::store::KnowledgeStore;

use super::index::{PAGE_PROVENANCE, SOURCE_PROVENANCE};

/// Where a node came from, which decides what clicking it can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphOrigin {
    /// A wiki page. Has a path, so it opens in the editor.
    Page,
    /// A file under `raw/sources/`. Has a path, and opens in the reader.
    Source,
    /// Knowledge that is not a file: typed in by the user, proposed by the
    /// agent, or derived by the repository indexer. Opens as a node.
    Curated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: NodeId,
    pub title: String,
    /// The vault's own page type (`concept`, `query`, `synthesis`) for
    /// nodes that came from a page, and the knowledge model's node type
    /// (`decision`, `gotcha`) for everything else.
    ///
    /// The two vocabularies are kept apart here rather than merged because
    /// the vault's is finer: collapsing `query` and `thesis` into
    /// `observation` on the way to the screen would throw away a
    /// distinction the user made deliberately.
    pub kind: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Vault-relative path, for the nodes that are files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub summary: String,
    pub authority: Authority,
    pub status: NodeStatus,
    pub origin: GraphOrigin,
    /// Edges touching this node in either direction.
    pub link_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub from: NodeId,
    pub to: NodeId,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    /// False for a relationship a model proposed, so the view can draw it
    /// as the suggestion it is rather than as an established fact.
    pub deterministic: bool,
}

/// Everything a graph view needs, in one read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Project the live knowledge graph.
///
/// Live nodes only: superseded and archived knowledge is retained for
/// history but must not be drawn as though it were current. Traversable
/// edges only, for the same reason — with the exception that a proposed
/// edge is included and flagged, because reviewing proposals is something
/// a person does *in* the graph.
pub fn project(store: &KnowledgeStore) -> Result<KnowledgeGraph> {
    let conn = store.connection();

    let mut statement = conn.prepare(
        "SELECT n.id, n.type, n.title, n.summary, n.authority, n.status,
                n.provenance_detail,
                (SELECT e.anchor_file FROM evidence e
                  WHERE e.node = n.id
                    AND (e.id LIKE 'ev_vault_page_%' OR e.id LIKE 'ev_vault_source_%')
                  LIMIT 1) AS path
           FROM nodes n
          WHERE n.status IN ('valid', 'stale', 'stale_requires_review')
          ORDER BY n.id",
    )?;
    let mut nodes = statement
        .query_map([], |row| {
            let node_type: NodeType = crate::store::parse_enum(&row.get::<_, String>(1)?, "type")?;
            let detail: Option<String> = row.get(6)?;
            let origin = origin_of(detail.as_deref());
            Ok(GraphNode {
                id: row.get(0)?,
                title: row.get(2)?,
                kind: kind_of(detail.as_deref(), node_type),
                node_type,
                path: row.get(7)?,
                summary: row.get(3)?,
                authority: crate::store::parse_enum(&row.get::<_, String>(4)?, "authority")?,
                status: crate::store::parse_enum(&row.get::<_, String>(5)?, "status")?,
                origin,
                link_count: 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let live = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut statement = conn.prepare(
        "SELECT from_node, to_node, type, provenance_source
           FROM edges
          WHERE status IN ('valid', 'stale', 'proposed')
          ORDER BY from_node, to_node, type",
    )?;
    let edges = statement
        .query_map([], |row| {
            Ok(GraphEdge {
                from: row.get(0)?,
                to: row.get(1)?,
                edge_type: crate::store::parse_enum(&row.get::<_, String>(2)?, "edge type")?,
                deterministic: row.get::<_, String>(3)? == "deterministic",
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        // An edge to a node the view is not drawing would be a dangling
        // line. The row stays in the database; it just is not on screen.
        .filter(|edge| live.contains(&edge.from) && live.contains(&edge.to))
        .collect::<Vec<_>>();

    let mut counts = std::collections::HashMap::<&NodeId, usize>::new();
    for edge in &edges {
        *counts.entry(&edge.from).or_default() += 1;
        *counts.entry(&edge.to).or_default() += 1;
    }
    let counts = counts
        .into_iter()
        .map(|(id, count)| (id.clone(), count))
        .collect::<std::collections::HashMap<_, _>>();
    for node in &mut nodes {
        node.link_count = counts.get(&node.id).copied().unwrap_or(0);
    }

    Ok(KnowledgeGraph { nodes, edges })
}

fn origin_of(detail: Option<&str>) -> GraphOrigin {
    match detail {
        Some(detail) if detail.starts_with(PAGE_PROVENANCE) => GraphOrigin::Page,
        Some(detail) if detail.starts_with(SOURCE_PROVENANCE) => GraphOrigin::Source,
        _ => GraphOrigin::Curated,
    }
}

fn kind_of(detail: Option<&str>, node_type: NodeType) -> String {
    match detail {
        Some(detail) if detail.starts_with(SOURCE_PROVENANCE) => "source".to_string(),
        Some(detail) => match detail.strip_prefix(PAGE_PROVENANCE) {
            Some(rest) => match rest.strip_prefix(':') {
                Some(page_type) if !page_type.is_empty() => page_type.to_string(),
                // A page whose location and frontmatter both said nothing.
                _ => node_type.as_str().to_string(),
            },
            None => node_type.as_str().to_string(),
        },
        None => node_type.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KnowledgeEdge, KnowledgeNode, Provenance};
    use crate::vault::{VaultIndexer, VaultScanOptions};

    fn write(root: &std::path::Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn indexed_vault() -> (tempfile::TempDir, KnowledgeStore) {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "wiki/concepts/chain-of-thought.md",
            "---\ntype: concept\ntitle: Chain of Thought\nsources: [\"wei.pdf\"]\n---\n\n# Chain of Thought\n\nLinks to [[openai]].\n",
        );
        write(
            dir.path(),
            "wiki/entities/openai.md",
            "---\ntype: entity\ntitle: OpenAI\n---\n\n# OpenAI\n",
        );
        write(
            dir.path(),
            "wiki/queries/does-it-scale.md",
            "# Does it scale\n",
        );
        write(dir.path(), "raw/sources/wei.pdf", "%PDF");
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        VaultIndexer::scan_and_index(&mut store, dir.path(), VaultScanOptions::default()).unwrap();
        (dir, store)
    }

    #[test]
    fn nodes_keep_the_vaults_own_vocabulary_and_their_paths() {
        let (_dir, store) = indexed_vault();
        let graph = project(&store).unwrap();

        let cot = graph
            .nodes
            .iter()
            .find(|node| node.title == "Chain of Thought")
            .unwrap();
        assert_eq!(cot.kind, "concept");
        assert_eq!(cot.node_type, NodeType::Concept);
        assert_eq!(
            cot.path.as_deref(),
            Some("wiki/concepts/chain-of-thought.md")
        );
        assert_eq!(cot.origin, GraphOrigin::Page);

        // `query` has no equivalent in the knowledge model's vocabulary and
        // reduces to an observation there — but the view still sees the
        // distinction the user made.
        let query = graph
            .nodes
            .iter()
            .find(|node| node.title == "Does it scale")
            .unwrap();
        assert_eq!(query.kind, "query");
        assert_eq!(query.node_type, NodeType::Observation);

        let source = graph
            .nodes
            .iter()
            .find(|node| node.title == "wei.pdf")
            .unwrap();
        assert_eq!(source.origin, GraphOrigin::Source);
        assert_eq!(source.path.as_deref(), Some("raw/sources/wei.pdf"));
    }

    #[test]
    fn edges_carry_their_type_and_whether_a_model_proposed_them() {
        let (_dir, mut store) = indexed_vault();
        let curated = store
            .create_node(
                KnowledgeNode::new(NodeType::Decision, "Use SQLite")
                    .with_provenance(Provenance::user()),
            )
            .unwrap();
        let cot = project(&store)
            .unwrap()
            .nodes
            .into_iter()
            .find(|node| node.title == "Chain of Thought")
            .unwrap();
        store
            .connect(
                KnowledgeEdge::new(curated.id.clone(), EdgeType::RelatedTo, cot.id.clone())
                    .with_provenance(Provenance::model("some-model")),
            )
            .unwrap();

        let graph = project(&store).unwrap();
        let derived = graph
            .edges
            .iter()
            .find(|edge| edge.edge_type == EdgeType::References)
            .unwrap();
        assert!(derived.deterministic);

        let proposed = graph
            .edges
            .iter()
            .find(|edge| edge.from == curated.id)
            .expect("a proposal is drawn so it can be reviewed in the graph");
        assert!(!proposed.deterministic);

        // Knowledge that is not a file is in the same graph — that is the
        // point of there being one model rather than two.
        let node = graph
            .nodes
            .iter()
            .find(|node| node.id == curated.id)
            .unwrap();
        assert_eq!(node.origin, GraphOrigin::Curated);
        assert_eq!(node.kind, "decision");
        assert_eq!(node.path, None);
    }

    #[test]
    fn link_counts_include_both_directions() {
        let (_dir, store) = indexed_vault();
        let graph = project(&store).unwrap();
        let openai = graph.nodes.iter().find(|n| n.title == "OpenAI").unwrap();
        // Linked from chain-of-thought and nowhere else.
        assert_eq!(openai.link_count, 1);
        let orphan = graph
            .nodes
            .iter()
            .find(|n| n.title == "Does it scale")
            .unwrap();
        assert_eq!(orphan.link_count, 0);
    }

    #[test]
    fn archived_knowledge_is_kept_but_not_drawn() {
        let (_dir, mut store) = indexed_vault();
        let openai = project(&store)
            .unwrap()
            .nodes
            .into_iter()
            .find(|node| node.title == "OpenAI")
            .unwrap();
        store
            .set_node_status(&openai.id, NodeStatus::Archived)
            .unwrap();

        let graph = project(&store).unwrap();
        assert!(!graph.nodes.iter().any(|node| node.id == openai.id));
        // And no edge is left dangling into it.
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.to == openai.id || edge.from == openai.id));
    }

    #[test]
    fn an_empty_store_projects_an_empty_graph() {
        let store = KnowledgeStore::open_in_memory().unwrap();
        assert_eq!(project(&store).unwrap(), KnowledgeGraph::default());
    }
}
