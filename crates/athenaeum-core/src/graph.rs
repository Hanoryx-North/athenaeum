//! Graph navigation.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::NodeId;
use crate::model::{EdgeType, KnowledgeNode, RecallSelection};
use crate::store::{EdgeFilter, KnowledgeStore};

/// Budget and shape of a walk.
#[derive(Debug, Clone)]
pub struct NavigationOptions {
    /// How many hops from the seeds to travel.
    pub max_depth: u8,
    /// Hard ceiling on nodes returned, seeds included.
    pub max_nodes: usize,
    /// Which edges may be followed.
    pub edges: EdgeFilter,
    pub selection: Option<RecallSelection>,
}

impl Default for NavigationOptions {
    fn default() -> Self {
        // Two hops reaches "the thing, what it depends on, and what tests it"
        // without pulling in the whole project.
        Self {
            max_depth: 2,
            max_nodes: 25,
            edges: EdgeFilter::traversable(),
            selection: None,
        }
    }
}

impl NavigationOptions {
    pub fn depth(mut self, max_depth: u8) -> Self {
        self.max_depth = max_depth;
        self
    }

    pub fn max_nodes(mut self, max_nodes: usize) -> Self {
        self.max_nodes = max_nodes;
        self
    }

    pub fn following(mut self, edges: EdgeFilter) -> Self {
        self.edges = edges;
        self
    }

    /// Follow only these relationship types.
    pub fn only(mut self, types: impl IntoIterator<Item = EdgeType>) -> Self {
        self.edges = self.edges.of_types(types);
        self
    }
}

/// A node the walk arrived at, and how it got there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reached {
    pub node: KnowledgeNode,
    /// Hops from the nearest seed. Seeds are at depth 0.
    pub depth: u8,
    /// The node this was reached from, and the relationship followed to get
    /// here — oriented in the direction of travel. `None` for seeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Step>,
}

/// One traversal step, as seen from the node being left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub from: NodeId,
    pub edge_type: EdgeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl Step {
    /// The step taken by following one stored relationship.
    pub fn of(from: &NodeId, edge: &crate::model::KnowledgeEdge, edge_type: EdgeType) -> Self {
        Self {
            from: from.clone(),
            edge_type,
            note: edge.note.clone(),
            confidence: edge.confidence.map(crate::model::wire_confidence),
        }
    }
}

/// Walk outward from `seeds`, breadth-first, within budget.
pub fn navigate(
    store: &KnowledgeStore,
    seeds: &[NodeId],
    options: &NavigationOptions,
) -> Result<Vec<Reached>> {
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<(NodeId, u8, Option<Step>)> = VecDeque::new();
    let mut reached: Vec<Reached> = Vec::new();

    for seed in seeds {
        if visited.insert(seed.clone()) {
            queue.push_back((seed.clone(), 0, None));
        }
    }

    while let Some((id, depth, via)) = queue.pop_front() {
        if reached.len() >= options.max_nodes {
            break;
        }

        let Some(node) = store.find_node(&id)? else {
            continue;
        };
        // Superseded and archived knowledge stays in the database but is not
        // handed to an agent as if it were current (§17).
        if !node.is_live()
            || options.selection.as_ref().is_some_and(|selection| {
                node.applicability.as_ref().is_some_and(|applicability| {
                    !applicability.matches(selection)
                        || (applicability.core && crate::portable::memory::is_proposal(&node))
                })
            })
        {
            continue;
        }

        reached.push(Reached { node, depth, via });

        if depth >= options.max_depth {
            continue;
        }

        for neighbor in store.neighbors(&id, &options.edges)? {
            if visited.insert(neighbor.node.clone()) {
                queue.push_back((
                    neighbor.node,
                    depth + 1,
                    Some(Step::of(&id, &neighbor.edge, neighbor.edge_type)),
                ));
            }
        }
    }

    Ok(reached)
}

/// The shortest chain of relationships connecting two nodes, if any.
pub fn shortest_path(
    store: &KnowledgeStore,
    from: &NodeId,
    to: &NodeId,
    options: &NavigationOptions,
) -> Result<Option<Vec<Step>>> {
    if from == to {
        return Ok(Some(Vec::new()));
    }

    let mut visited: HashSet<NodeId> = HashSet::from([from.clone()]);
    let mut parents: HashMap<NodeId, Step> = HashMap::new();
    let mut queue: VecDeque<(NodeId, u8)> = VecDeque::from([(from.clone(), 0)]);

    while let Some((id, depth)) = queue.pop_front() {
        if depth >= options.max_depth {
            continue;
        }

        for neighbor in store.neighbors(&id, &options.edges)? {
            if !visited.insert(neighbor.node.clone()) {
                continue;
            }
            parents.insert(
                neighbor.node.clone(),
                Step::of(&id, &neighbor.edge, neighbor.edge_type),
            );

            if &neighbor.node == to {
                return Ok(Some(reconstruct(&parents, from, to)));
            }
            queue.push_back((neighbor.node, depth + 1));
        }
    }

    Ok(None)
}

/// Walk the parent chain back from `to` and reverse it into travel order.
fn reconstruct(parents: &HashMap<NodeId, Step>, from: &NodeId, to: &NodeId) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut cursor = to.clone();

    while let Some(step) = parents.get(&cursor) {
        steps.push(step.clone());
        if &step.from == from {
            break;
        }
        cursor = step.from.clone();
    }

    steps.reverse();
    steps
}

/// Nodes with the most relationships — the natural entry points to a graph.
pub fn hubs(store: &KnowledgeStore, limit: u32) -> Result<Vec<(NodeId, u32)>> {
    let mut stmt = store.connection().prepare(
        "SELECT id, degree FROM (
             SELECT n.id AS id, COUNT(e.id) AS degree
             FROM nodes n
             LEFT JOIN edges e
                    ON (e.from_node = n.id OR e.to_node = n.id)
                   AND e.status IN ('valid', 'stale')
             WHERE n.status IN ('valid', 'stale', 'stale_requires_review')
             GROUP BY n.id
         )
         WHERE degree > 0
         ORDER BY degree DESC, id ASC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok((row.get::<_, NodeId>(0)?, row.get::<_, i64>(1)? as u32))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
