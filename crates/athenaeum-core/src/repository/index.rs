use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    GitCommit, RepositoryFile, RepositoryScanOptions, RepositorySnapshot, RepositorySymbol,
};
use crate::error::Result;
use crate::id::{EvidenceId, NodeId};
use crate::model::{
    hash, Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, NodeStatus,
    NodeType, Provenance, ProvenanceSource, Scope, SourceAnchor,
};
use crate::store::{KnowledgeStore, NodeFilter};

const FILE_PROVENANCE: &str = "repository_index:file";
const SYMBOL_PROVENANCE: &str = "repository_index:symbol";
const COMMIT_PROVENANCE: &str = "repository_index:git_commit";
const EDGE_PROVENANCE_PREFIX: &str = "repository_index:";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryIndexReport {
    pub scan_complete: bool,
    pub files: usize,
    pub symbols: usize,
    pub resolved_imports: usize,
    pub commits: usize,
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub nodes_marked_stale: usize,
    pub edges_created: usize,
    pub edges_removed: usize,
    pub evidence_attached: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryIndexer;

impl RepositoryIndexer {
    pub fn scan_and_index(
        store: &mut KnowledgeStore,
        root: impl AsRef<Path>,
        options: RepositoryScanOptions,
    ) -> Result<RepositoryIndexReport> {
        let snapshot = RepositorySnapshot::scan(root.as_ref(), options)?;
        Self::index(store, root, snapshot)
    }

    pub fn index(
        store: &mut KnowledgeStore,
        root: impl AsRef<Path>,
        snapshot: RepositorySnapshot,
    ) -> Result<RepositoryIndexReport> {
        let root = root.as_ref();
        let mut contents = HashMap::new();
        for file in &snapshot.files {
            super::validate_relative_path(&file.path, "repository file")?;
            let content = std::fs::read_to_string(root.join(&file.path))?;
            if hash(&content) != file.content_hash {
                return Err(crate::Error::InvalidValue {
                    field: "repository snapshot",
                    value: format!("{} changed after it was scanned", file.path),
                });
            }
            contents.insert(file.path.clone(), content);
        }
        let mut report = RepositoryIndexReport {
            scan_complete: snapshot.complete,
            files: snapshot.files.len(),
            symbols: snapshot.files.iter().map(|file| file.symbols.len()).sum(),
            resolved_imports: snapshot
                .files
                .iter()
                .flat_map(|file| &file.imports)
                .filter(|import| import.target.is_some())
                .count(),
            commits: 0,
            warnings: snapshot.warnings,
            ..RepositoryIndexReport::default()
        };

        let symbol_titles = symbol_titles(&snapshot.files);
        let mut file_nodes = HashMap::new();
        let mut symbol_nodes = HashMap::new();
        let mut current_structural_nodes = HashSet::new();

        for file in &snapshot.files {
            let desired = file_node(file);
            let id = desired.id.clone();
            ensure_node(store, desired, &mut report)?;
            current_structural_nodes.insert(id.clone());
            file_nodes.insert(file.path.clone(), id.clone());
            let content = contents
                .get(&file.path)
                .ok_or_else(|| crate::Error::InvalidValue {
                    field: "repository snapshot",
                    value: file.path.clone(),
                })?;
            if ensure_file_evidence(store, file, content, &id)? {
                report.evidence_attached += 1;
            }

            for symbol in &file.symbols {
                let title = symbol_titles
                    .get(&(file.path.clone(), symbol.qualified_name.clone()))
                    .cloned()
                    .unwrap_or_else(|| symbol.qualified_name.clone());
                let desired = symbol_node(file, symbol, title);
                let symbol_id = desired.id.clone();
                ensure_node(store, desired, &mut report)?;
                current_structural_nodes.insert(symbol_id.clone());
                symbol_nodes.insert(
                    (file.path.clone(), symbol.qualified_name.clone()),
                    symbol_id.clone(),
                );
                if ensure_symbol_evidence(store, file, symbol, content, &symbol_id)? {
                    report.evidence_attached += 1;
                }
            }
        }

        let mut commit_nodes = HashMap::new();
        let relevant_commits = snapshot
            .commits
            .iter()
            .filter(|commit| {
                commit
                    .files
                    .iter()
                    .any(|path| file_nodes.contains_key(path))
            })
            .collect::<Vec<_>>();
        report.commits = relevant_commits.len();
        for commit in &relevant_commits {
            let desired = commit_node(commit);
            let id = desired.id.clone();
            ensure_node(store, desired, &mut report)?;
            commit_nodes.insert(commit.id.clone(), id);
        }

        if snapshot.complete {
            mark_missing_structural_nodes_stale(store, &current_structural_nodes, &mut report)?;
        }

        for file in &snapshot.files {
            let Some(file_id) = file_nodes.get(&file.path) else {
                continue;
            };
            let mut expected = HashSet::new();
            let mut expected_ordered = Vec::new();

            for symbol in &file.symbols {
                if let Some(symbol_id) =
                    symbol_nodes.get(&(file.path.clone(), symbol.qualified_name.clone()))
                {
                    push_expected(
                        &mut expected,
                        &mut expected_ordered,
                        EdgeType::Contains,
                        symbol_id.clone(),
                    );
                }
            }
            for import in &file.imports {
                if let Some(target_id) = import
                    .target
                    .as_ref()
                    .and_then(|target| file_nodes.get(target))
                {
                    push_expected(
                        &mut expected,
                        &mut expected_ordered,
                        EdgeType::DependsOn,
                        target_id.clone(),
                    );
                }
            }
            for commit in relevant_commits
                .iter()
                .filter(|commit| commit.files.contains(&file.path))
            {
                if let Some(commit_id) = commit_nodes.get(&commit.id) {
                    push_expected(
                        &mut expected,
                        &mut expected_ordered,
                        EdgeType::ModifiedBy,
                        commit_id.clone(),
                    );
                }
            }

            reconcile_outgoing_edges(store, file_id, &expected, &mut report)?;
            for (edge_type, target) in expected_ordered {
                if store
                    .find_edge_between(file_id, &target, edge_type)?
                    .is_none()
                {
                    store.connect(KnowledgeEdge::deterministic(
                        file_id.clone(),
                        edge_type,
                        target,
                        edge_detail(edge_type),
                    ))?;
                    report.edges_created += 1;
                }
            }
        }

        Ok(report)
    }
}

fn push_expected(
    seen: &mut HashSet<(EdgeType, NodeId)>,
    ordered: &mut Vec<(EdgeType, NodeId)>,
    edge_type: EdgeType,
    target: NodeId,
) {
    if seen.insert((edge_type, target.clone())) {
        ordered.push((edge_type, target));
    }
}

fn ensure_node(
    store: &mut KnowledgeStore,
    desired: KnowledgeNode,
    report: &mut RepositoryIndexReport,
) -> Result<()> {
    match store.find_node(&desired.id)? {
        None => {
            store.create_node(desired)?;
            report.nodes_created += 1;
        }
        Some(existing) if node_changed(&existing, &desired) => {
            let mut updated = desired;
            updated.created_at = existing.created_at;
            updated.version = existing.version;
            store.update_node_attributed(
                updated,
                Some("deterministic repository re-index".to_string()),
                Provenance::deterministic("repository_index"),
            )?;
            report.nodes_updated += 1;
        }
        Some(_) => {}
    }
    Ok(())
}

fn node_changed(existing: &KnowledgeNode, desired: &KnowledgeNode) -> bool {
    existing.node_type != desired.node_type
        || existing.title != desired.title
        || existing.summary != desired.summary
        || existing.content != desired.content
        || existing.scope != desired.scope
        || existing.authority != desired.authority
        || existing.status != desired.status
        || existing.provenance != desired.provenance
}

fn file_node(file: &RepositoryFile) -> KnowledgeNode {
    let role = if file.is_test {
        "test file"
    } else if file.is_documentation {
        "documentation file"
    } else if file.is_configuration {
        "configuration file"
    } else {
        "source file"
    };
    let language = file
        .language
        .map(|language| format!("{} ", language.as_str()))
        .unwrap_or_default();
    deterministic_node(
        stable_node_id("file", &file.path),
        if file.is_test {
            NodeType::Test
        } else {
            NodeType::File
        },
        file.path.clone(),
        format!("{language}{role} at {}.", file.path),
        FILE_PROVENANCE,
    )
}

fn symbol_node(file: &RepositoryFile, symbol: &RepositorySymbol, title: String) -> KnowledgeNode {
    deterministic_node(
        stable_node_id(
            "symbol",
            &format!("{}\0{}", file.path, symbol.qualified_name),
        ),
        NodeType::Symbol,
        title,
        format!(
            "{} declared at {}:{}.",
            symbol.kind.as_str(),
            file.path,
            symbol.line_start
        ),
        SYMBOL_PROVENANCE,
    )
}

fn commit_node(commit: &GitCommit) -> KnowledgeNode {
    let short = commit.id.get(..12).unwrap_or(&commit.id);
    deterministic_node(
        stable_node_id("commit", &commit.id),
        NodeType::Source,
        format!("{short} {}", commit.subject),
        format!(
            "Git commit by {} at {}; modifies {} indexed file(s).",
            commit.author,
            commit.authored_at,
            commit.files.len()
        ),
        COMMIT_PROVENANCE,
    )
    .with_content(format!(
        "Commit: {}\nSubject: {}",
        commit.id, commit.subject
    ))
    .with_scope(Scope::Repository)
}

fn deterministic_node(
    id: NodeId,
    node_type: NodeType,
    title: String,
    summary: String,
    provenance: &'static str,
) -> KnowledgeNode {
    KnowledgeNode::new(node_type, title)
        .with_id(id)
        .with_summary(summary)
        .with_scope(Scope::File)
        .with_authority(Authority::ImportedKnowledge)
        .with_status(NodeStatus::Valid)
        .with_provenance(Provenance::deterministic(provenance))
}

fn symbol_titles(files: &[RepositoryFile]) -> HashMap<(String, String), String> {
    let mut counts = HashMap::<String, usize>::new();
    for symbol in files.iter().flat_map(|file| &file.symbols) {
        *counts.entry(symbol.qualified_name.clone()).or_default() += 1;
    }
    let mut titles = HashMap::new();
    for file in files {
        for symbol in &file.symbols {
            let title = if counts.get(&symbol.qualified_name).copied().unwrap_or(0) > 1 {
                format!("{} — {}", symbol.qualified_name, file.path)
            } else {
                symbol.qualified_name.clone()
            };
            titles.insert((file.path.clone(), symbol.qualified_name.clone()), title);
        }
    }
    titles
}

fn ensure_file_evidence(
    store: &mut KnowledgeStore,
    file: &RepositoryFile,
    content: &str,
    node: &NodeId,
) -> Result<bool> {
    let id = stable_evidence_id(node);
    if store.find_evidence(&id)?.is_some() {
        return Ok(false);
    }
    let line_count = content.lines().count().max(1) as u32;
    let region = super::region_by_lines(content, 1, line_count).unwrap_or_default();
    let anchor = SourceAnchor::file(&file.path)
        .with_lines(1, line_count)
        .fingerprint(region, content);
    let kind = if file.is_test {
        EvidenceKind::Test
    } else if file.is_documentation {
        EvidenceKind::Documentation
    } else if file.is_configuration {
        EvidenceKind::Configuration
    } else {
        EvidenceKind::SourceFile
    };
    store.attach_evidence(
        Evidence::new(node.clone(), kind)
            .with_id(id)
            .with_anchor(anchor),
    )?;
    Ok(true)
}

fn ensure_symbol_evidence(
    store: &mut KnowledgeStore,
    file: &RepositoryFile,
    symbol: &RepositorySymbol,
    content: &str,
    node: &NodeId,
) -> Result<bool> {
    let id = stable_evidence_id(node);
    if store.find_evidence(&id)?.is_some() {
        return Ok(false);
    }
    let region =
        super::region_by_lines(content, symbol.line_start, symbol.line_end).unwrap_or_default();
    let anchor = SourceAnchor::file(&file.path)
        .with_symbol(&symbol.qualified_name)
        .with_lines(symbol.line_start, symbol.line_end)
        .fingerprint(region, content);
    store.attach_evidence(
        Evidence::new(node.clone(), EvidenceKind::Symbol)
            .with_id(id)
            .with_anchor(anchor),
    )?;
    Ok(true)
}

fn reconcile_outgoing_edges(
    store: &mut KnowledgeStore,
    file: &NodeId,
    expected: &HashSet<(EdgeType, NodeId)>,
    report: &mut RepositoryIndexReport,
) -> Result<()> {
    let obsolete = store
        .edges_of(file)?
        .into_iter()
        .filter(|edge| {
            edge.from == *file
                && edge.provenance.source == ProvenanceSource::Deterministic
                && edge
                    .provenance
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with(EDGE_PROVENANCE_PREFIX))
                && !expected.contains(&(edge.edge_type, edge.to.clone()))
        })
        .map(|edge| edge.id)
        .collect::<Vec<_>>();
    for edge in obsolete {
        store.remove_edge(&edge)?;
        report.edges_removed += 1;
    }
    Ok(())
}

fn mark_missing_structural_nodes_stale(
    store: &mut KnowledgeStore,
    current: &HashSet<NodeId>,
    report: &mut RepositoryIndexReport,
) -> Result<()> {
    let missing = store
        .list_nodes(&NodeFilter::all())?
        .into_iter()
        .filter(|node| {
            matches!(
                node.provenance.detail.as_deref(),
                Some(FILE_PROVENANCE | SYMBOL_PROVENANCE)
            ) && !current.contains(&node.id)
                && node.status != NodeStatus::StaleRequiresReview
        })
        .map(|node| node.id)
        .collect::<Vec<_>>();
    for node in missing {
        let obsolete = store
            .edges_of(&node)?
            .into_iter()
            .filter(|edge| {
                edge.from == node
                    && edge.provenance.source == ProvenanceSource::Deterministic
                    && edge
                        .provenance
                        .detail
                        .as_deref()
                        .is_some_and(|detail| detail.starts_with(EDGE_PROVENANCE_PREFIX))
            })
            .map(|edge| edge.id)
            .collect::<Vec<_>>();
        for edge in obsolete {
            store.remove_edge(&edge)?;
            report.edges_removed += 1;
        }
        store.set_node_status(&node, NodeStatus::StaleRequiresReview)?;
        report.nodes_marked_stale += 1;
    }
    Ok(())
}

fn edge_detail(edge_type: EdgeType) -> &'static str {
    match edge_type {
        EdgeType::Contains => "repository_index:file_contains_symbol",
        EdgeType::DependsOn => "repository_index:import",
        EdgeType::ModifiedBy => "repository_index:git_history",
        _ => "repository_index:structural",
    }
}

pub(crate) fn stable_node_id(kind: &str, identity: &str) -> NodeId {
    NodeId::from_existing(format!("repo_{kind}_{}", hash(identity)))
}

fn stable_evidence_id(node: &NodeId) -> EvidenceId {
    EvidenceId::from_existing(format!("ev_repo_{}", hash(node.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EdgeFilter;

    #[test]
    fn indexing_builds_valid_idempotent_structural_graph() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "mod store;\npub fn open() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/store.rs"), "pub struct Store;\n").unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();

        let first = RepositoryIndexer::scan_and_index(
            &mut store,
            dir.path(),
            RepositoryScanOptions {
                max_commits: 0,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();
        assert_eq!(first.files, 2);
        assert_eq!(first.resolved_imports, 1);
        assert!(first.nodes_created >= 4);
        assert!(first.edges_created >= 2);

        let lib = store.find_node_by_title("src/lib.rs").unwrap().unwrap();
        let neighbors = store
            .neighbors(&lib.id, &EdgeFilter::traversable())
            .unwrap();
        assert!(neighbors
            .iter()
            .any(|neighbor| neighbor.edge_type == EdgeType::DependsOn));
        assert!(neighbors
            .iter()
            .all(|neighbor| neighbor.edge.is_traversable()));

        let second = RepositoryIndexer::scan_and_index(
            &mut store,
            dir.path(),
            RepositoryScanOptions {
                max_commits: 0,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();
        assert_eq!(second.nodes_created, 0);
        assert_eq!(second.nodes_updated, 0);
        assert_eq!(second.edges_created, 0);
        assert_eq!(second.evidence_attached, 0);
    }

    #[test]
    fn reindex_removes_obsolete_import_edges_and_marks_removed_symbols_stale() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.ts"),
            "import { b } from './b';\nexport function a() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.ts"), "export function b() {}\n").unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let options = RepositoryScanOptions {
            max_commits: 0,
            ..RepositoryScanOptions::default()
        };
        RepositoryIndexer::scan_and_index(&mut store, dir.path(), options).unwrap();

        std::fs::write(
            dir.path().join("a.ts"),
            "export function replacement() {}\n",
        )
        .unwrap();
        let report = RepositoryIndexer::scan_and_index(&mut store, dir.path(), options).unwrap();
        assert_eq!(report.edges_removed, 2);
        assert_eq!(report.nodes_marked_stale, 1);
        let old = store.find_node_by_title("a").unwrap().unwrap();
        assert_eq!(old.status, NodeStatus::StaleRequiresReview);
    }

    #[test]
    fn a_budget_truncated_scan_does_not_mark_unvisited_nodes_stale() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "pub fn b() {}\n").unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        RepositoryIndexer::scan_and_index(
            &mut store,
            dir.path(),
            RepositoryScanOptions {
                max_commits: 0,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();

        let report = RepositoryIndexer::scan_and_index(
            &mut store,
            dir.path(),
            RepositoryScanOptions {
                max_files: 1,
                max_commits: 0,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();
        assert!(!report.scan_complete);
        assert_eq!(report.nodes_marked_stale, 0);
        assert_eq!(
            store.find_node_by_title("b").unwrap().unwrap().status,
            NodeStatus::Valid
        );
    }

    #[test]
    fn externally_supplied_snapshots_cannot_escape_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let snapshot = RepositorySnapshot {
            files: vec![RepositoryFile {
                path: "../outside.rs".to_string(),
                language: None,
                bytes: 0,
                content_hash: hash(""),
                is_test: false,
                is_documentation: false,
                is_configuration: false,
                symbols: Vec::new(),
                imports: Vec::new(),
            }],
            commits: Vec::new(),
            warnings: Vec::new(),
            complete: true,
        };
        assert!(matches!(
            RepositoryIndexer::index(&mut store, dir.path(), snapshot),
            Err(crate::Error::InvalidValue {
                field: "repository file",
                ..
            })
        ));
        assert_eq!(store.stats().unwrap().nodes, 0);
    }

    #[test]
    fn git_history_creates_only_relevant_valid_modification_edges() {
        let dir = tempfile::tempdir().unwrap();
        let content = "fn main() {}\n";
        std::fs::write(dir.path().join("main.rs"), content).unwrap();
        let snapshot = RepositorySnapshot {
            files: vec![RepositoryFile {
                path: "main.rs".to_string(),
                language: Some(super::super::SourceLanguage::Rust),
                bytes: content.len() as u64,
                content_hash: hash(content),
                is_test: false,
                is_documentation: false,
                is_configuration: false,
                symbols: Vec::new(),
                imports: Vec::new(),
            }],
            commits: vec![
                GitCommit {
                    id: "a".repeat(40),
                    author: "Developer".to_string(),
                    authored_at: "2026-08-21T00:00:00Z".to_string(),
                    subject: "change main".to_string(),
                    files: vec!["main.rs".to_string()],
                },
                GitCommit {
                    id: "b".repeat(40),
                    author: "Developer".to_string(),
                    authored_at: "2026-08-20T00:00:00Z".to_string(),
                    subject: "change image".to_string(),
                    files: vec!["logo.png".to_string()],
                },
            ],
            warnings: Vec::new(),
            complete: true,
        };
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let report = RepositoryIndexer::index(&mut store, dir.path(), snapshot).unwrap();
        assert_eq!(report.commits, 1);

        let file = store.find_node_by_title("main.rs").unwrap().unwrap();
        let modified = store
            .neighbors(&file.id, &EdgeFilter::traversable())
            .unwrap()
            .into_iter()
            .find(|neighbor| neighbor.edge_type == EdgeType::ModifiedBy)
            .unwrap();
        assert_eq!(
            store.get_node(&modified.node).unwrap().node_type,
            NodeType::Source
        );
        assert_eq!(
            modified.edge.provenance.source,
            ProvenanceSource::Deterministic
        );
    }
}
