use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{Applicability, EvidenceKind, KnowledgeNode, NodeType, Scope};
use crate::store::{KnowledgeStore, NodeFilter};
use crate::wiki::{WikiCaller, WikiEvidenceCreate, WikiNodeCreate, WikiService};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principle {
    pub title: String,
    pub summary: String,
    pub stages: Vec<String>,
}

pub fn catalog() -> Vec<Principle> {
    serde_json::from_str(include_str!("principles.json")).expect("validated principle catalog")
}

pub fn propose(store: &mut KnowledgeStore) -> Result<Vec<KnowledgeNode>> {
    let existing = store.list_nodes(&NodeFilter::all())?;
    let mut service = WikiService::new(store);
    let caller = WikiCaller {
        model: "athenaeum-principles".into(),
        session: None,
    };
    let mut proposed = Vec::new();
    for principle in catalog() {
        if existing.iter().any(|node| node.title == principle.title) {
            continue;
        }
        let node = service.create(
            WikiNodeCreate {
                node_type: NodeType::Procedure,
                title: principle.title,
                summary: principle.summary.clone(),
                content: principle.summary,
                scope: Scope::Universal,
                applicability: Some(Applicability {
                    core: true,
                    stages: principle.stages,
                    ..Default::default()
                }),
            },
            caller.clone(),
        )?;
        service.attach_evidence(WikiEvidenceCreate {
            node: node.id.to_string(), kind: EvidenceKind::ExternalSource, anchor: None,
            excerpt: None, note: Some("Owner-supplied engineering and agent CLI talks, 2026-11-17. General guidance; project outcomes require local verification. Catalog: crates/athenaeum-core/src/principles.json.".into()),
        }, caller.clone())?;
        proposed.push(node);
    }
    Ok(proposed)
}
