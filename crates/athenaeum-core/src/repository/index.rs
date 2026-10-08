use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    GitCommit, RepositoryCall, RepositoryFile, RepositoryScanOptions, RepositorySnapshot,
    RepositorySymbol, SourceLanguage,
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
    pub exported_symbols: usize,
    pub resolved_imports: usize,
    /// Call sites that named a declaration this repository actually holds.
    pub resolved_calls: usize,
    /// `implements` / `extends` / `impl ... for ...` relationships resolved
    /// to two declarations this repository holds.
    pub implementations: usize,
    /// References from a test file into code under test.
    pub test_links: usize,
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
            exported_symbols: snapshot
                .files
                .iter()
                .flat_map(|file| &file.symbols)
                .filter(|symbol| symbol.exported)
                .count(),
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

        let identities = symbol_identities(&snapshot.files);
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

            for (index, symbol) in file.symbols.iter().enumerate() {
                let identity = identity_of(&identities, &file.path, index, symbol);
                let desired = symbol_node(file, symbol, &identity);
                let symbol_id = desired.id.clone();
                ensure_node(store, desired, &mut report)?;
                current_structural_nodes.insert(symbol_id.clone());
                symbol_nodes.insert((file.path.clone(), identity.key.clone()), symbol_id.clone());
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

        let names = SymbolNames::of(&snapshot.files, &identities, &symbol_nodes);
        let mut expected: BTreeMap<NodeId, Expected> = BTreeMap::new();

        for file in &snapshot.files {
            let Some(file_id) = file_nodes.get(&file.path) else {
                continue;
            };
            // Every indexed node gets an entry even when nothing is expected
            // of it, so that a relationship which used to exist and no longer
            // does is reconciled away rather than left behind.
            let declared = file
                .symbols
                .iter()
                .enumerate()
                .filter_map(|(index, symbol)| {
                    let identity = identity_of(&identities, &file.path, index, symbol);
                    symbol_nodes.get(&(file.path.clone(), identity.key))
                })
                .cloned()
                .collect::<Vec<_>>();
            for symbol_id in &declared {
                expected.entry(symbol_id.clone()).or_default();
            }

            let outgoing = expected.entry(file_id.clone()).or_default();
            for symbol_id in declared {
                outgoing.push(EdgeType::Contains, symbol_id);
            }
            for import in &file.imports {
                if let Some(target_id) = import
                    .target
                    .as_ref()
                    .and_then(|target| file_nodes.get(target))
                {
                    outgoing.push(EdgeType::DependsOn, target_id.clone());
                }
            }
            for commit in relevant_commits
                .iter()
                .filter(|commit| commit.files.contains(&file.path))
            {
                if let Some(commit_id) = commit_nodes.get(&commit.id) {
                    outgoing.push(EdgeType::ModifiedBy, commit_id.clone());
                }
            }
        }

        for file in &snapshot.files {
            let Some(file_id) = file_nodes.get(&file.path) else {
                continue;
            };

            for (index, symbol) in file.symbols.iter().enumerate() {
                let identity = identity_of(&identities, &file.path, index, symbol);
                let Some(symbol_id) = symbol_nodes.get(&(file.path.clone(), identity.key)) else {
                    continue;
                };
                for call in &symbol.calls {
                    let Some(target) = names.resolve(file, call) else {
                        continue;
                    };
                    if target.node == *symbol_id {
                        continue;
                    }
                    // A test naming the code it exercises is a test
                    // relationship, not a plain call (§19). Recording it as
                    // both would describe one fact with two edges. Whether
                    // the caller is a test is asked of the declaration, not
                    // only of its file: Rust keeps its tests inside the file
                    // they cover.
                    let caller_is_test = file.is_test || symbol.test_scoped;
                    let (edge_type, counter) = if caller_is_test && !target.is_test {
                        (EdgeType::Tests, &mut report.test_links)
                    } else {
                        (EdgeType::Calls, &mut report.resolved_calls)
                    };
                    let target_file = target.file.clone();
                    let target_node = target.node.clone();
                    if expected
                        .entry(symbol_id.clone())
                        .or_default()
                        .push(edge_type, target_node)
                    {
                        *counter += 1;
                    }
                    if edge_type == EdgeType::Tests {
                        if let Some(tested_file) = file_nodes.get(&target_file) {
                            if tested_file != file_id {
                                expected
                                    .entry(file_id.clone())
                                    .or_default()
                                    .push(EdgeType::Tests, tested_file.clone());
                            }
                        }
                    }
                }
            }

            for relation in &file.implements {
                // A supertype is always written as a bare name in the clause
                // that announces it, never reached through a receiver.
                let (Some(subject), Some(object)) = (
                    names.resolve(file, &bare(&relation.subject)),
                    names.resolve(file, &bare(&relation.object)),
                ) else {
                    continue;
                };
                if subject.node == object.node {
                    continue;
                }
                if expected
                    .entry(subject.node.clone())
                    .or_default()
                    .push(EdgeType::Implements, object.node.clone())
                {
                    report.implementations += 1;
                }
            }
        }

        for (source, outgoing) in expected {
            reconcile_outgoing_edges(store, &source, &outgoing.seen, &mut report)?;
            for (edge_type, target) in outgoing.ordered {
                if store
                    .find_edge_between(&source, &target, edge_type)?
                    .is_none()
                {
                    store.connect(KnowledgeEdge::deterministic(
                        source.clone(),
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

/// The relationships one node should have once this pass is finished.
///
/// Insertion order is kept so that repeated runs write the same journal, and
/// the set alongside it is what tells the reconciler which existing edges are
/// no longer implied by the repository.
#[derive(Debug, Default)]
struct Expected {
    seen: HashSet<(EdgeType, NodeId)>,
    ordered: Vec<(EdgeType, NodeId)>,
}

impl Expected {
    /// Returns true when this relationship had not already been expected.
    fn push(&mut self, edge_type: EdgeType, target: NodeId) -> bool {
        if self.seen.insert((edge_type, target.clone())) {
            self.ordered.push((edge_type, target));
            return true;
        }
        false
    }
}

/// A declaration a written name was resolved to.
#[derive(Debug, Clone)]
struct ResolvedSymbol {
    node: NodeId,
    file: String,
    language: Option<SourceLanguage>,
    is_test: bool,
    /// The type a method hangs off, when it hangs off one: `WikiService` for
    /// `WikiService::context`.
    owner: Option<String>,
}

/// Turns names written at a call site or in an `implements` clause back into
/// declarations.
///
/// It resolves only what it can resolve without guessing, and what counts as
/// a guess depends on how the name was written.
///
/// A name declared in the same file is the same file's. Beyond that, a **bare
/// name** had to be in lexical scope to be called at all, so if exactly one
/// declaration in the repository carries it, that is the one. A **name on a
/// receiver** is dispatched on the receiver's type, which this reader cannot
/// see: `values.push(x)` and a project's own `push` are written identically,
/// and a repository that happens to declare exactly one `push` is not
/// evidence that every `.push(` in it means that one. So a receiver call
/// reaches another file only when this file imports it — you can only call
/// what you can name.
///
/// Everything else is dropped. That is the conservative half of §21: a graph
/// where `open` links to all eleven `open`s is worse than one where it links
/// to none, because the reader cannot tell the difference.
#[derive(Debug, Default)]
struct SymbolNames {
    /// `None` marks a name that more than one declaration answers to.
    in_file: HashMap<(String, String), Option<ResolvedSymbol>>,
    repository: HashMap<String, Option<ResolvedSymbol>>,
}

impl SymbolNames {
    fn of(
        files: &[RepositoryFile],
        identities: &HashMap<String, Vec<SymbolIdentity>>,
        symbol_nodes: &HashMap<(String, String), NodeId>,
    ) -> Self {
        let mut names = SymbolNames::default();
        for file in files {
            for (index, symbol) in file.symbols.iter().enumerate() {
                let identity = identity_of(identities, &file.path, index, symbol);
                let Some(node) = symbol_nodes.get(&(file.path.clone(), identity.key)) else {
                    continue;
                };
                let resolved = ResolvedSymbol {
                    node: node.clone(),
                    file: file.path.clone(),
                    language: file.language,
                    is_test: file.is_test || symbol.test_scoped,
                    owner: owner_of(&symbol.qualified_name),
                };
                for key in [&symbol.name, &symbol.qualified_name] {
                    record(
                        &mut names.in_file,
                        (file.path.clone(), key.clone()),
                        &resolved,
                    );
                    record(&mut names.repository, key.clone(), &resolved);
                }
            }
        }
        names
    }

    fn resolve(&self, from: &RepositoryFile, call: &RepositoryCall) -> Option<&ResolvedSymbol> {
        if let Some(local) = self.in_file.get(&(from.path.clone(), call.name.clone())) {
            return local.as_ref();
        }
        let resolved = self.repository.get(&call.name)?.as_ref()?;
        // A TypeScript test calling `bare()` does not reach a Rust `fn bare`,
        // however unique that name is. Nothing in one language can call a
        // declaration in another without a boundary this scan does not read.
        if resolved.language != from.language {
            return None;
        }
        if call.on_receiver && !can_name(from, resolved) {
            return None;
        }
        Some(resolved)
    }
}

/// Whether `file` could have written this name: it imports the file that
/// declares it, or it imports the type the method hangs off.
///
/// The second half is what keeps a real call across a crate boundary. A file
/// holding `use athenaeum_core::wiki::WikiService` can write
/// `…​.context(…)`, and that it did not import a *file* only means the
/// declaration lives somewhere this scan resolves by name rather than by
/// path.
fn can_name(file: &RepositoryFile, resolved: &ResolvedSymbol) -> bool {
    if file
        .imports
        .iter()
        .any(|import| import.target.as_deref() == Some(resolved.file.as_str()))
    {
        return true;
    }
    let Some(owner) = &resolved.owner else {
        return false;
    };
    file.imports
        .iter()
        .any(|import| names_segment(&import.specifier, owner))
}

/// Whether `specifier` mentions `name` as a whole path segment, rather than
/// as a substring of a longer identifier.
fn names_segment(specifier: &str, name: &str) -> bool {
    specifier
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .any(|segment| segment == name)
}

/// The type part of a qualified name, for the names that have one.
fn owner_of(qualified_name: &str) -> Option<String> {
    for separator in ["::", "."] {
        if let Some((owner, _)) = qualified_name.rsplit_once(separator) {
            if !owner.is_empty() {
                return Some(owner.to_string());
            }
        }
    }
    None
}

/// A name written plainly, not reached through anything.
fn bare(name: &str) -> RepositoryCall {
    RepositoryCall {
        name: name.to_string(),
        on_receiver: false,
    }
}

fn record<K: std::hash::Hash + Eq>(
    into: &mut HashMap<K, Option<ResolvedSymbol>>,
    key: K,
    resolved: &ResolvedSymbol,
) {
    into.entry(key)
        .and_modify(|existing| {
            if existing
                .as_ref()
                .is_some_and(|held| held.node != resolved.node)
            {
                *existing = None;
            }
        })
        .or_insert_with(|| Some(resolved.clone()));
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

fn symbol_node(
    file: &RepositoryFile,
    symbol: &RepositorySymbol,
    identity: &SymbolIdentity,
) -> KnowledgeNode {
    deterministic_node(
        stable_node_id("symbol", &format!("{}\0{}", file.path, identity.key)),
        NodeType::Symbol,
        identity.title.clone(),
        format!(
            "{}{} declared at {}:{}.",
            if symbol.exported { "Exported " } else { "" },
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

/// The identity computed for `file.symbols[index]`, falling back to the
/// declaration's own name if the tables ever disagree about a file.
fn identity_of(
    identities: &HashMap<String, Vec<SymbolIdentity>>,
    path: &str,
    index: usize,
    symbol: &RepositorySymbol,
) -> SymbolIdentity {
    identities
        .get(path)
        .and_then(|named| named.get(index))
        .cloned()
        .unwrap_or_else(|| SymbolIdentity {
            key: symbol.qualified_name.clone(),
            title: symbol.qualified_name.clone(),
        })
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

/// What one declaration is called, and what tells it apart from the others.
#[derive(Debug, Clone)]
struct SymbolIdentity {
    /// Stable within its file, and the thing the node id is derived from.
    key: String,
    /// What a person reads.
    title: String,
}

/// Names every declaration in the snapshot, twice over: once so that no two
/// of them can claim the same node, and once so that a person can tell them
/// apart in a list.
///
/// Both problems are real. A file can declare the same qualified name more
/// than once — `From::from` on two `impl From` blocks, a test helper
/// redefined further down — and without the first distinction those two
/// declarations collapse into one node that rewrites itself on every index.
/// A repository can also hold the same name in several files, which is not a
/// collision but is unreadable in a list of results.
///
/// The returned vectors run parallel to each file's `symbols`.
fn symbol_identities(files: &[RepositoryFile]) -> HashMap<String, Vec<SymbolIdentity>> {
    let mut repository_counts = HashMap::<&str, usize>::new();
    for symbol in files.iter().flat_map(|file| &file.symbols) {
        *repository_counts
            .entry(symbol.qualified_name.as_str())
            .or_default() += 1;
    }

    let mut identities = HashMap::new();
    for file in files {
        let mut file_counts = HashMap::<&str, usize>::new();
        for symbol in &file.symbols {
            *file_counts
                .entry(symbol.qualified_name.as_str())
                .or_default() += 1;
        }

        let mut seen = HashMap::<&str, usize>::new();
        let named = file
            .symbols
            .iter()
            .map(|symbol| {
                let occurrence = seen
                    .entry(symbol.qualified_name.as_str())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                // The first occurrence keeps the plain name, so the ordinary
                // case — one declaration, one name — never grows a suffix.
                let key = if *occurrence == 1 {
                    symbol.qualified_name.clone()
                } else {
                    format!("{}#{occurrence}", symbol.qualified_name)
                };
                let repeated_here = file_counts
                    .get(symbol.qualified_name.as_str())
                    .copied()
                    .unwrap_or(0)
                    > 1;
                let title = if repeated_here {
                    format!(
                        "{} — {}:{}",
                        symbol.qualified_name, file.path, symbol.line_start
                    )
                } else if repository_counts
                    .get(symbol.qualified_name.as_str())
                    .copied()
                    .unwrap_or(0)
                    > 1
                {
                    format!("{} — {}", symbol.qualified_name, file.path)
                } else {
                    symbol.qualified_name.clone()
                };
                SymbolIdentity { key, title }
            })
            .collect::<Vec<_>>();
        identities.insert(file.path.clone(), named);
    }
    identities
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
    source: &NodeId,
    expected: &HashSet<(EdgeType, NodeId)>,
    report: &mut RepositoryIndexReport,
) -> Result<()> {
    let obsolete = store
        .edges_of(source)?
        .into_iter()
        .filter(|edge| {
            edge.from == *source
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
        EdgeType::Calls => "repository_index:call",
        EdgeType::Implements => "repository_index:implements",
        EdgeType::Tests => "repository_index:test_reference",
        _ => "repository_index:structural",
    }
}

pub(crate) fn stable_node_id(kind: &str, identity: &str) -> NodeId {
    NodeId::from_existing(format!("repo_{kind}_{}", hash(identity)))
}

fn stable_evidence_id(node: &NodeId) -> EvidenceId {
    EvidenceId::from_existing(format!("ev_repo_{}", hash(node.as_str())))
}
