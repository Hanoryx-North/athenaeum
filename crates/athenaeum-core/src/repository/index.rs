use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    Extraction, ExtractionMethod, GitCommit, RepositoryCall, RepositoryFile, RepositoryScan,
    RepositoryScanOptions, RepositorySnapshot, RepositorySymbol, SourceLanguage,
};
use crate::error::Result;
use crate::id::{EvidenceId, NodeId};
use crate::model::{
    hash, Authority, EdgeType, Evidence, EvidenceKind, KnowledgeEdge, KnowledgeNode, NodeStatus,
    NodeType, Provenance, ProvenanceSource, Scope, SourceAnchor,
};
use crate::status::{record_scan_state, ScanState, SkippedFile};
use crate::store::{KnowledgeStore, NodeFilter, SourceRecord, SourceScope};

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
    /// Existing relationships whose recorded reason or confidence this pass
    /// corrected. Zero on an unchanged repository.
    #[serde(default)]
    pub edges_updated: usize,
    /// References from a test file into code under test.
    pub test_links: usize,
    pub commits: usize,
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub nodes_marked_stale: usize,
    pub edges_created: usize,
    pub edges_removed: usize,
    pub evidence_attached: usize,
    /// Files the scan saw and deliberately did not read, with the reason.
    #[serde(default)]
    pub skipped: Vec<SkippedFile>,
    /// Files whose structural facts came from the previous scan unchanged.
    #[serde(default)]
    pub reused_files: usize,
    /// Files read from a syntax tree.
    #[serde(default)]
    pub ast_files: usize,
    /// Files read by the line-oriented fallback, with a reason in the record.
    #[serde(default)]
    pub fallback_files: usize,
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
        let scan = RepositorySnapshot::scan_detailed(root.as_ref(), options)?;
        Self::index_scan(store, root, scan)
    }

    /// Index an already-scanned repository, with nothing known about how it was
    /// read.
    ///
    /// The snapshot is the caller's, so no claim is made about a parser having
    /// been involved: nodes and edges carry no extraction method, which is
    /// exactly what "unknown" means here.
    pub fn index(
        store: &mut KnowledgeStore,
        root: impl AsRef<Path>,
        snapshot: RepositorySnapshot,
    ) -> Result<RepositoryIndexReport> {
        Self::index_scan(store, root, RepositoryScan::new(snapshot))
    }

    /// Index a repository scan, recording how each file was read.
    pub fn index_scan(
        store: &mut KnowledgeStore,
        root: impl AsRef<Path>,
        scan: RepositoryScan,
    ) -> Result<RepositoryIndexReport> {
        let root = root.as_ref();
        let snapshot = &scan.snapshot;
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
            skipped: scan.skipped.clone(),
            reused_files: scan.reused_files,
            ast_files: scan
                .extractions
                .values()
                .filter(|extraction| extraction.method == ExtractionMethod::Ast)
                .count(),
            fallback_files: scan
                .extractions
                .values()
                .filter(|extraction| extraction.method == ExtractionMethod::Fallback)
                .count(),
            warnings: snapshot.warnings.clone(),
            ..RepositoryIndexReport::default()
        };

        let identities = symbol_identities(&snapshot.files);
        let mut file_nodes = HashMap::new();
        let mut symbol_nodes = HashMap::new();
        let mut current_structural_nodes = HashSet::new();

        for file in &snapshot.files {
            let desired = file_node(file, scan.extractions.get(&file.path));
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

        // A narrowed scan saw a subset of the tree on purpose, and a truncated
        // one did not finish. Absence proves nothing in either case, so nothing
        // is marked stale on the strength of it.
        if snapshot.complete && !scan.filtered {
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

            let file_method = method_of(&scan, &file.path);
            let outgoing = expected.entry(file_id.clone()).or_default();
            for symbol_id in declared {
                outgoing.push(
                    EdgeType::Contains,
                    symbol_id,
                    method_of(&scan, &file.path),
                    Some(format!("{} declares this symbol", file.path)),
                );
            }
            for import in &file.imports {
                if let Some(target_id) = import
                    .target
                    .as_ref()
                    .and_then(|target| file_nodes.get(target))
                {
                    outgoing.push(
                        EdgeType::DependsOn,
                        target_id.clone(),
                        file_method,
                        Some(format!(
                            "{} imports {:?}, which resolves to a file in this repository",
                            file.path, import.specifier
                        )),
                    );
                }
            }
            for commit in relevant_commits
                .iter()
                .filter(|commit| commit.files.contains(&file.path))
            {
                if let Some(commit_id) = commit_nodes.get(&commit.id) {
                    let short = commit.id.get(..12).unwrap_or(&commit.id);
                    outgoing.push(
                        EdgeType::ModifiedBy,
                        commit_id.clone(),
                        file_method,
                        Some(format!("commit {short} modified {}", file.path)),
                    );
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
                let caller_method = symbol.extraction;
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
                    let note = match edge_type {
                        EdgeType::Tests => format!(
                            "{} exercises it (test code naming the declaration it covers)",
                            symbol.qualified_name
                        ),
                        _ => format!(
                            "{} invokes it (the call site resolved to this declaration)",
                            symbol.qualified_name
                        ),
                    };
                    if expected.entry(symbol_id.clone()).or_default().push(
                        edge_type,
                        target_node,
                        caller_method,
                        Some(note),
                    ) {
                        *counter += 1;
                    }
                    if edge_type == EdgeType::Tests {
                        if let Some(tested_file) = file_nodes.get(&target_file) {
                            if tested_file != file_id {
                                expected.entry(file_id.clone()).or_default().push(
                                    EdgeType::Tests,
                                    tested_file.clone(),
                                    caller_method,
                                    Some(format!("{} is a test file for this one", file.path)),
                                );
                            }
                        }
                    }
                }
            }

            for relation in &file.implements {
                let subject_method = file
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name == relation.subject)
                    .map_or_else(|| method_of(&scan, &file.path), |symbol| symbol.extraction);
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
                if expected.entry(subject.node.clone()).or_default().push(
                    EdgeType::Implements,
                    object.node.clone(),
                    subject_method,
                    Some(format!(
                        "{} implements {} in its declaration",
                        relation.subject, relation.object
                    )),
                ) {
                    report.implementations += 1;
                }
            }
        }

        for (source, outgoing) in expected {
            reconcile_outgoing_edges(store, &source, &outgoing.seen, &mut report)?;
            for (edge_type, target, method, note) in outgoing.ordered {
                // The reason says which reading produced the relationship, and
                // the confidence says how certain that reading makes it. Both
                // are inspectable in the graph; a snapshot that never said how
                // it was read carries neither a claim about the parser nor a
                // fabricated number.
                let detail = edge_detail(edge_type, method);
                let note = note.map(|note| match method {
                    ExtractionMethod::Fallback => {
                        format!("{note} (read by a line-oriented fallback, not a syntax tree)")
                    }
                    _ => note,
                });
                let confidence = edge_confidence(edge_type, method);

                match store.find_edge_between(&source, &target, edge_type)? {
                    // A relationship this crate owns, read again: keep the edge,
                    // correct its reason and confidence if they have moved on.
                    Some(existing) if is_repository_edge(&existing) => {
                        if store.refresh_edge_metadata(
                            &existing.id,
                            note.as_deref(),
                            confidence,
                            &detail,
                        )? {
                            report.edges_updated += 1;
                        }
                    }
                    // Somebody else's relationship — a model's, a person's.
                    // Never touched.
                    Some(_) => {}
                    None => {
                        let mut edge =
                            KnowledgeEdge::deterministic(source.clone(), edge_type, target, detail);
                        if let Some(confidence) = confidence {
                            edge = edge.with_confidence(confidence);
                        }
                        if let Some(note) = note {
                            edge = edge.with_note(note);
                        }
                        store.connect(edge)?;
                        report.edges_created += 1;
                    }
                }
            }
        }

        if !scan.filtered {
            record_inventory(store, root, &scan)?;
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
    ordered: Vec<(EdgeType, NodeId, ExtractionMethod, Option<String>)>,
}

impl Expected {
    /// Returns true when this relationship had not already been expected.
    ///
    /// The method and the reason travel with the relationship so that the edge
    /// can carry both: how sure the reading was, and what it actually read.
    fn push(
        &mut self,
        edge_type: EdgeType,
        target: NodeId,
        method: ExtractionMethod,
        note: Option<String>,
    ) -> bool {
        if self.seen.insert((edge_type, target.clone())) {
            self.ordered.push((edge_type, target, method, note));
            return true;
        }
        false
    }
}

/// True for a relationship this indexer created and therefore maintains.
fn is_repository_edge(edge: &KnowledgeEdge) -> bool {
    edge.provenance.source == ProvenanceSource::Deterministic
        && edge
            .provenance
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with(EDGE_PROVENANCE_PREFIX))
}

/// How a file was read, or `Unknown` when the scan did not say.
fn method_of(scan: &RepositoryScan, path: &str) -> ExtractionMethod {
    scan.extractions
        .get(path)
        .map_or(ExtractionMethod::Unknown, |extraction| extraction.method)
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

fn file_node(file: &RepositoryFile, extraction: Option<&Extraction>) -> KnowledgeNode {
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
    let understood = extraction
        .and_then(|extraction| extraction.method.explanation())
        .map(|explanation| format!(" {explanation}"))
        .unwrap_or_default();
    deterministic_node(
        stable_node_id("file", &file.path),
        if file.is_test {
            NodeType::Test
        } else {
            NodeType::File
        },
        file.path.clone(),
        format!("{language}{role} at {}.{understood}", file.path),
        FILE_PROVENANCE,
    )
}

fn symbol_node(
    file: &RepositoryFile,
    symbol: &RepositorySymbol,
    identity: &SymbolIdentity,
) -> KnowledgeNode {
    let understood = symbol
        .extraction
        .explanation()
        .map(|explanation| format!(" {explanation}"))
        .unwrap_or_default();
    deterministic_node(
        stable_node_id("symbol", &format!("{}\0{}", file.path, identity.key)),
        NodeType::Symbol,
        identity.title.clone(),
        format!(
            "{}{} declared at {}:{}.{understood}",
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

/// How certain a structural relationship is, given how it was read.
///
/// Certainty is not one number for a whole file. A file *containing* a
/// declaration is what the syntax tree literally says; an import resolved to a
/// path is a path join that either found the file or did not; a call site
/// resolved to a declaration is a name match that could be a different `open`,
/// which is why it is lower and why the line reader's version is lower still.
/// `Unknown` returns `None`: a snapshot that did not record its method has
/// nothing to be confident about, and silence is not the same as certainty.
fn edge_confidence(edge_type: EdgeType, method: ExtractionMethod) -> Option<f32> {
    let base = match edge_type {
        EdgeType::Contains | EdgeType::ModifiedBy => 1.0,
        EdgeType::DependsOn => 0.95,
        EdgeType::Implements => 0.85,
        EdgeType::Calls | EdgeType::Tests => 0.8,
        EdgeType::PartOf => 1.0,
        _ => 0.8,
    };
    match method {
        ExtractionMethod::Ast => Some(base),
        ExtractionMethod::Fallback => {
            Some((base * ExtractionMethod::Fallback.confidence() * 100.0).round() / 100.0)
        }
        ExtractionMethod::Unknown => None,
    }
}

/// The inspectable reason an edge exists.
///
/// A relationship read by the line-oriented fallback says so in the reason
/// itself, so a reader of the graph never has to guess whether a link came from
/// a syntax tree. The `repository_index:` prefix is what the reconciler looks
/// for, so the suffix is free to carry it.
fn edge_detail(edge_type: EdgeType, method: ExtractionMethod) -> String {
    let reason = match edge_type {
        EdgeType::Contains => "repository_index:file_contains_symbol",
        EdgeType::DependsOn => "repository_index:import",
        EdgeType::ModifiedBy => "repository_index:git_history",
        EdgeType::Calls => "repository_index:call",
        EdgeType::Implements => "repository_index:implements",
        EdgeType::Tests => "repository_index:test_reference",
        _ => "repository_index:structural",
    };
    match method {
        ExtractionMethod::Fallback => format!("{reason}_fallback"),
        _ => reason.to_string(),
    }
}

/// Record what this pass read, so `sources`, `status` and `review` need no
/// second walk of the tree.
///
/// A filtered pass records nothing: it saw a subset of the repository, and a
/// source inventory built from a subset would claim the rest had gone away.
fn record_inventory(store: &mut KnowledgeStore, root: &Path, scan: &RepositoryScan) -> Result<()> {
    let at = chrono::Utc::now().to_rfc3339();
    let mut records = Vec::with_capacity(scan.snapshot.files.len());
    for file in &scan.snapshot.files {
        records.push(SourceRecord {
            path: file.path.clone(),
            bytes: file.bytes,
            sha256: file.content_hash.clone(),
            mtime_ns: mtime_of(root, &file.path),
            scanned: true,
            detail: None,
            indexed_at: at.clone(),
        });
    }
    for skipped in scan.skipped.iter().take(MAX_RECORDED_SKIPS) {
        let metadata = std::fs::metadata(root.join(&skipped.path)).ok();
        records.push(SourceRecord {
            path: skipped.path.clone(),
            bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
            sha256: String::new(),
            mtime_ns: metadata
                .as_ref()
                .map_or(0, crate::status::modification_time),
            scanned: false,
            detail: Some(format!("{}: {}", skipped.reason, skipped.detail)),
            indexed_at: at.clone(),
        });
    }
    store.replace_sources(SourceScope::Repository, &records)?;
    record_scan_state(
        store,
        SourceScope::Repository,
        &ScanState {
            complete: scan.snapshot.complete,
            scanned: scan.snapshot.files.len(),
            skipped: scan.skipped.iter().take(MAX_SCAN_SAMPLE).cloned().collect(),
            skipped_total: scan.skipped.len(),
            at,
        },
    )
}

/// Skip rows kept in the derived inventory. A pathologically broken tree can
/// skip thousands of files; the count stays exact and the list stays bounded.
const MAX_RECORDED_SKIPS: usize = 2_000;

/// Skip entries kept in the scan state that the review queue reads.
const MAX_SCAN_SAMPLE: usize = 200;

/// The modification time a later scan compares against.
fn mtime_of(root: &Path, relative: &str) -> i64 {
    std::fs::metadata(root.join(relative))
        .map(|metadata| crate::status::modification_time(&metadata))
        .unwrap_or(0)
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

    /// Writes `files` into a fresh repository and indexes it without Git.
    fn indexed(files: &[(&str, &str)]) -> (tempfile::TempDir, KnowledgeStore) {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, content).unwrap();
        }
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
        (dir, store)
    }

    fn no_git() -> RepositoryScanOptions {
        RepositoryScanOptions {
            max_commits: 0,
            ..RepositoryScanOptions::default()
        }
    }

    /// Every edge of `edge_type` leaving the node titled `title`.
    fn out(store: &KnowledgeStore, title: &str, edge_type: EdgeType) -> Vec<String> {
        let node = store
            .find_node_by_title(title)
            .unwrap()
            .unwrap_or_else(|| panic!("no node titled {title}"));
        let mut titles = store
            .neighbors(&node.id, &EdgeFilter::traversable())
            .unwrap()
            .into_iter()
            .filter(|neighbor| neighbor.edge_type == edge_type)
            .map(|neighbor| store.get_node(&neighbor.node).unwrap().title)
            .collect::<Vec<_>>();
        titles.sort();
        titles
    }

    #[test]
    fn two_declarations_sharing_a_name_in_one_file_stay_two_nodes() {
        // Regression: both were derived from the same `path + qualified name`,
        // so they claimed one node and rewrote each other on every index.
        let (_dir, mut store) = indexed(&[(
            "src/id.rs",
            "impl From<String> for NodeId {\n    fn from(value: String) {}\n}\nimpl From<u32> for NodeId {\n    fn from(value: u32) {}\n}\n",
        )]);

        let symbols = store
            .list_nodes(&NodeFilter::all())
            .unwrap()
            .into_iter()
            .filter(|node| node.node_type == NodeType::Symbol)
            .filter(|node| node.title.starts_with("NodeId::from"))
            .collect::<Vec<_>>();
        assert_eq!(symbols.len(), 2, "{symbols:#?}");
        // Both are readable, and the title says which is which.
        assert!(symbols
            .iter()
            .any(|node| node.title.ends_with("src/id.rs:2")));
        assert!(symbols
            .iter()
            .any(|node| node.title.ends_with("src/id.rs:5")));
        let _ = &mut store;
    }

    #[test]
    fn re_indexing_an_unchanged_repository_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        // Includes the duplicate-name shape that used to oscillate.
        std::fs::write(
            dir.path().join("src/id.rs"),
            "impl From<String> for NodeId {\n    fn from(v: String) {}\n}\nimpl From<u32> for NodeId {\n    fn from(v: u32) {}\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/auth.rs"),
            "pub fn authenticate() {\n    validate();\n}\npub fn validate() {}\n",
        )
        .unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();

        RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
        for _ in 0..3 {
            let again =
                RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
            assert_eq!(again.nodes_created, 0);
            assert_eq!(again.nodes_updated, 0);
            assert_eq!(again.edges_created, 0);
            assert_eq!(again.edges_removed, 0);
            assert_eq!(again.evidence_attached, 0);
        }
    }

    #[test]
    fn a_resolved_call_becomes_an_edge_that_reads_both_ways() {
        let (_dir, store) = indexed(&[(
            "src/auth.rs",
            "pub fn authenticate() {\n    validate_token();\n}\npub fn validate_token() {}\n",
        )]);

        assert_eq!(
            out(&store, "authenticate", EdgeType::Calls),
            ["validate_token"]
        );
        // Arriving at the callee, the relationship reads the other way (§35).
        assert_eq!(
            out(&store, "validate_token", EdgeType::CalledBy),
            ["authenticate"]
        );
    }

    #[test]
    fn calls_across_files_resolve_and_calls_to_nothing_indexed_do_not() {
        let (_dir, store) = indexed(&[
            (
                "src/auth.rs",
                "pub fn authenticate() {\n    create_session();\n    log_to_stderr();\n}\n",
            ),
            ("src/session.rs", "pub fn create_session() {}\n"),
        ]);

        // `log_to_stderr` is declared nowhere in this repository, so it is
        // not a relationship — only a word.
        assert_eq!(
            out(&store, "authenticate", EdgeType::Calls),
            ["create_session"]
        );
    }

    #[test]
    fn a_method_call_does_not_reach_a_file_this_one_never_named() {
        // Regression: `values.push(x)` resolved to a project's own unique
        // `push`, because the name matched and nothing checked that the
        // caller could possibly have meant it.
        let (_dir, store) = indexed(&[
            (
                "src/collect.rs",
                "pub struct Bag;\nimpl Bag {\n    pub fn push(&self, v: u32) {}\n}\n",
            ),
            (
                "src/parse.rs",
                "pub fn parse() {\n    let mut values = Vec::new();\n    values.push(1);\n}\n",
            ),
        ]);

        assert!(
            out(&store, "parse", EdgeType::Calls).is_empty(),
            "a standard-library method call became a project relationship"
        );
    }

    #[test]
    fn a_method_call_resolves_once_this_file_can_name_its_type() {
        let (_dir, store) = indexed(&[
            (
                "src/collect.rs",
                "pub struct Bag;\nimpl Bag {\n    pub fn stash(&self, v: u32) {}\n}\n",
            ),
            (
                "src/parse.rs",
                "use crate::collect::Bag;\npub fn parse() {\n    let bag = Bag;\n    bag.stash(1);\n}\n",
            ),
        ]);

        assert_eq!(out(&store, "parse", EdgeType::Calls), ["Bag::stash"]);
    }

    #[test]
    fn a_bare_call_still_reaches_a_uniquely_named_declaration_anywhere() {
        // A bare name had to be in scope to be written at all, so a single
        // repository-wide match is the one that was meant.
        let (_dir, store) = indexed(&[
            ("src/session.rs", "pub fn create_session() {}\n"),
            (
                "src/auth.rs",
                "pub fn authenticate() {\n    create_session();\n}\n",
            ),
        ]);

        assert_eq!(
            out(&store, "authenticate", EdgeType::Calls),
            ["create_session"]
        );
    }

    #[test]
    fn a_call_never_crosses_from_one_language_into_another() {
        // Regression: a TypeScript test calling `bare()` was linked to a Rust
        // `fn bare`, on the strength of the name alone.
        let (_dir, store) = indexed(&[
            ("src/helpers.rs", "pub fn shared() {}\n"),
            ("src/app.ts", "export function run() {\n  shared();\n}\n"),
        ]);

        assert!(
            out(&store, "run", EdgeType::Calls).is_empty(),
            "a TypeScript call reached a Rust declaration"
        );
    }

    #[test]
    fn an_ambiguous_name_resolves_locally_or_not_at_all() {
        let (_dir, store) = indexed(&[
            (
                "src/a.rs",
                "pub fn run() {\n    helper();\n}\npub fn helper() {}\n",
            ),
            ("src/b.rs", "pub fn helper() {}\n"),
            ("src/c.rs", "pub fn caller() {\n    helper();\n}\n"),
        ]);

        // Two declarations answer to `helper`. From `a.rs` the local one wins.
        let local = store
            .find_node_by_title("helper — src/a.rs")
            .unwrap()
            .unwrap();
        let from_run = store
            .neighbors(
                &store.find_node_by_title("run").unwrap().unwrap().id,
                &EdgeFilter::traversable(),
            )
            .unwrap();
        assert_eq!(
            from_run
                .iter()
                .filter(|neighbor| neighbor.edge_type == EdgeType::Calls)
                .map(|neighbor| neighbor.node.clone())
                .collect::<Vec<_>>(),
            vec![local.id]
        );

        // From a file that declares neither, guessing would be worse than
        // silence (§21).
        assert!(out(&store, "caller", EdgeType::Calls).is_empty());
    }

    #[test]
    fn implemented_traits_and_interfaces_become_implements_edges() {
        let (_dir, store) = indexed(&[
            (
                "src/store.rs",
                "pub trait Store {}\npub struct Sqlite;\nimpl Store for Sqlite {}\n",
            ),
            (
                "src/auth.ts",
                "export interface AuthProvider {}\nexport class AuthService implements AuthProvider {}\n",
            ),
        ]);

        assert_eq!(out(&store, "Sqlite", EdgeType::Implements), ["Store"]);
        assert_eq!(out(&store, "Store", EdgeType::ImplementedBy), ["Sqlite"]);
        assert_eq!(
            out(&store, "AuthService", EdgeType::Implements),
            ["AuthProvider"]
        );
    }

    #[test]
    fn a_rust_test_module_covers_the_file_it_lives_in() {
        let (_dir, store) = indexed(&[(
            "src/auth.rs",
            "pub fn authenticate() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn it_authenticates() {\n        authenticate();\n    }\n}\n",
        )]);

        assert_eq!(
            out(&store, "it_authenticates", EdgeType::Tests),
            ["authenticate"]
        );
        assert_eq!(
            out(&store, "authenticate", EdgeType::TestedBy),
            ["it_authenticates"]
        );
        assert!(out(&store, "it_authenticates", EdgeType::Calls).is_empty());
    }

    #[test]
    fn a_test_naming_the_code_it_exercises_is_a_test_edge_not_a_call() {
        let (_dir, store) = indexed(&[
            ("src/auth.rs", "pub fn authenticate() {}\n"),
            (
                "tests/auth_test.rs",
                "fn covers_authentication() {\n    authenticate();\n}\n",
            ),
        ]);

        assert_eq!(
            out(&store, "covers_authentication", EdgeType::Tests),
            ["authenticate"]
        );
        assert!(out(&store, "covers_authentication", EdgeType::Calls).is_empty());
        // And the same fact at file granularity, which is the one a person
        // reads as "what covers this file" (§30).
        assert_eq!(
            out(&store, "tests/auth_test.rs", EdgeType::Tests),
            ["src/auth.rs"]
        );
        assert_eq!(
            out(&store, "src/auth.rs", EdgeType::TestedBy),
            ["tests/auth_test.rs"]
        );
    }

    #[test]
    fn the_code_graph_is_reconciled_rather_than_accumulated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/auth.rs");
        std::fs::write(
            &path,
            "pub fn authenticate() {\n    validate_token();\n}\npub fn validate_token() {}\n",
        )
        .unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();

        let first = RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
        assert_eq!(first.resolved_calls, 1);

        // Re-indexing an unchanged repository writes nothing at all.
        let again = RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
        assert_eq!(again.edges_created, 0);
        assert_eq!(again.edges_removed, 0);
        assert_eq!(again.nodes_updated, 0);

        // Dropping the call drops the edge rather than leaving a relationship
        // the code no longer claims.
        std::fs::write(
            &path,
            "pub fn authenticate() {}\npub fn validate_token() {}\n",
        )
        .unwrap();
        let third = RepositoryIndexer::scan_and_index(&mut store, dir.path(), no_git()).unwrap();
        assert_eq!(third.resolved_calls, 0);
        assert_eq!(third.edges_removed, 1);
        assert!(out(&store, "authenticate", EdgeType::Calls).is_empty());
    }

    #[test]
    fn symbol_evidence_anchors_the_whole_body_so_it_can_be_relocated() {
        let (dir, store) = indexed(&[(
            "src/auth.rs",
            "pub fn authenticate() {\n    let token = 1;\n    token\n}\n",
        )]);
        let _ = &dir;

        let node = store.find_node_by_title("authenticate").unwrap().unwrap();
        let evidence = store.evidence_for(&node.id).unwrap();
        let anchor = evidence[0].anchor.as_ref().unwrap();
        assert_eq!((anchor.line_start, anchor.line_end), (Some(1), Some(4)));
        assert!(anchor.region_hash.is_some());
    }

    #[test]
    fn exported_declarations_say_so() {
        let (_dir, store) = indexed(&[("src/auth.rs", "pub fn open() {}\nfn hidden() {}\n")]);

        assert!(store
            .find_node_by_title("open")
            .unwrap()
            .unwrap()
            .summary
            .starts_with("Exported function"));
        assert!(store
            .find_node_by_title("hidden")
            .unwrap()
            .unwrap()
            .summary
            .starts_with("function"));
    }

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
                implements: Vec::new(),
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
                implements: Vec::new(),
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
