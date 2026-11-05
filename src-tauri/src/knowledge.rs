//! Tauri commands: the entire surface the UI can reach.
//!
//! Every one of these is a thin translation between JSON and
//! [`athenaeum_core`]. No knowledge logic lives here — if a rule about how
//! knowledge behaves ends up in this file, it belongs in the library instead,
//! where it can be tested without a window and shared with the CLI.

use std::path::Path;
use std::sync::Mutex;

use athenaeum_core::journal::Event;
use athenaeum_core::prelude::*;
use athenaeum_core::vault::{
    lint, KnowledgeGraph, LintOptions, VaultIndexReport, VaultIndexer, VaultLintReport,
    VaultScanOptions,
};
use athenaeum_core::wiki::{WikiNodeDetail, WikiService};
use serde::{Deserialize, Serialize};

use crate::panic_guard::run_guarded;

/// The open knowledge base, or nothing if no project has been opened yet.
#[derive(Default)]
pub struct AppState {
    store: Mutex<Option<KnowledgeStore>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    /// Run `f` against the open store, or report that none is open.
    fn with<T>(
        &self,
        f: impl FnOnce(&mut KnowledgeStore) -> Result<T>,
    ) -> std::result::Result<T, String> {
        let mut guard = self.store.lock().map_err(|_| {
            // A poisoned lock means a previous command panicked mid-write.
            "the knowledge base is in an inconsistent state; reopen the project".to_string()
        })?;
        let store = guard.as_mut().ok_or("no project is open")?;
        f(store).map_err(|e| e.to_string())
    }

    /// The project directory holding the open workspace — the vault root.
    fn project_root(&self) -> std::result::Result<std::path::PathBuf, String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "could not take the store lock".to_string())?;
        let store = guard.as_ref().ok_or("no project is open")?;
        store
            .root()
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "the workspace has no project directory".to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNode {
    #[serde(rename = "type")]
    pub node_type: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    /// True when the user typed this themselves, which is the most
    /// authoritative provenance there is.
    #[serde(default)]
    pub stated_by_user: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEdit {
    pub id: NodeId,
    pub title: String,
    pub summary: String,
    pub content: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Parse one of the library's vocabularies, reporting the field name so the
/// UI can say which dropdown was wrong rather than just "invalid input".
fn parse<T: std::str::FromStr>(raw: &str, field: &str) -> std::result::Result<T, String> {
    raw.parse::<T>()
        .map_err(|_| format!("unknown {field}: {raw:?}"))
}

#[tauri::command]
pub fn open_workspace(
    state: tauri::State<'_, AppState>,
    path: String,
) -> std::result::Result<Stats, String> {
    run_guarded("open_workspace", || {
        let workspace = Workspace::init(&path).map_err(|e| e.to_string())?;
        let store = workspace.open().map_err(|e| e.to_string())?;
        let stats = store.stats().map_err(|e| e.to_string())?;

        let mut guard = state
            .store
            .lock()
            .map_err(|_| "could not take the store lock".to_string())?;
        *guard = Some(store);
        Ok(stats)
    })
}

#[tauri::command]
pub fn workspace_path(state: tauri::State<'_, AppState>) -> Option<String> {
    let guard = state.store.lock().ok()?;
    guard.as_ref().map(|s| s.root().display().to_string())
}

#[tauri::command]
pub fn stats(state: tauri::State<'_, AppState>) -> std::result::Result<Stats, String> {
    run_guarded("stats", || state.with(|store| store.stats()))
}

#[tauri::command]
pub fn list_nodes(
    state: tauri::State<'_, AppState>,
    node_type: Option<String>,
    include_archived: Option<bool>,
    limit: Option<u32>,
) -> std::result::Result<Vec<KnowledgeNode>, String> {
    run_guarded("list_nodes", || {
        let mut filter = if include_archived.unwrap_or(false) {
            NodeFilter::all()
        } else {
            NodeFilter::live()
        };
        if let Some(raw) = &node_type {
            filter = filter.of_type(parse(raw, "node type")?);
        }
        state.with(|store| store.list_nodes(&filter.limit(limit.unwrap_or(200))))
    })
}

#[tauri::command]
pub fn node_detail(
    state: tauri::State<'_, AppState>,
    id: NodeId,
) -> std::result::Result<WikiNodeDetail, String> {
    run_guarded("node_detail", || {
        state.with(|store| WikiService::new(store).open(id.as_str()))
    })
}

#[tauri::command]
pub fn create_node(
    state: tauri::State<'_, AppState>,
    input: NewNode,
) -> std::result::Result<KnowledgeNode, String> {
    run_guarded("create_node", || {
        let node_type = parse(&input.node_type, "node type")?;
        let authority = match &input.authority {
            Some(raw) => parse(raw, "authority")?,
            None => Authority::default(),
        };
        let scope = match &input.scope {
            Some(raw) => parse(raw, "scope")?,
            None => Scope::default(),
        };

        let mut node = KnowledgeNode::new(node_type, input.title)
            .with_summary(input.summary)
            .with_content(input.content)
            .with_authority(authority)
            .with_scope(scope);
        if input.stated_by_user {
            node = node.with_provenance(Provenance::user());
        }

        state.with(|store| store.create_node(node))
    })
}

#[tauri::command]
pub fn update_node(
    state: tauri::State<'_, AppState>,
    input: NodeEdit,
) -> std::result::Result<KnowledgeNode, String> {
    run_guarded("update_node", || {
        state.with(|store| {
            let mut node = store.get_node(&input.id)?;
            node.title = input.title.clone();
            node.summary = input.summary.clone();
            node.content = input.content.clone();
            store.update_node(node, input.reason.clone())
        })
    })
}

#[tauri::command]
pub fn set_node_status(
    state: tauri::State<'_, AppState>,
    id: NodeId,
    status: String,
) -> std::result::Result<KnowledgeNode, String> {
    run_guarded("set_node_status", || {
        let status = parse(&status, "status")?;
        state.with(|store| store.set_node_status(&id, status))
    })
}

#[tauri::command]
pub fn delete_node(
    state: tauri::State<'_, AppState>,
    id: NodeId,
) -> std::result::Result<(), String> {
    run_guarded("delete_node", || state.with(|store| store.delete_node(&id)))
}

#[tauri::command]
pub fn link_nodes(
    state: tauri::State<'_, AppState>,
    from: NodeId,
    edge_type: String,
    to: NodeId,
    note: Option<String>,
) -> std::result::Result<KnowledgeEdge, String> {
    run_guarded("link_nodes", || {
        let edge_type = parse(&edge_type, "edge type")?;
        state.with(|store| {
            // A relationship the user drew by hand is stated, not proposed.
            let mut edge = KnowledgeEdge::new(from.clone(), edge_type, to.clone())
                .with_provenance(Provenance::user())
                .with_status(NodeStatus::Valid);
            if let Some(note) = &note {
                edge = edge.with_note(note);
            }
            store.connect(edge)
        })
    })
}

#[tauri::command]
pub fn promote_edge(
    state: tauri::State<'_, AppState>,
    id: EdgeId,
) -> std::result::Result<KnowledgeEdge, String> {
    run_guarded("promote_edge", || {
        state.with(|store| store.promote_edge(&id))
    })
}

#[tauri::command]
pub fn remove_edge(
    state: tauri::State<'_, AppState>,
    id: EdgeId,
) -> std::result::Result<(), String> {
    run_guarded("remove_edge", || {
        state.with(|store| athenaeum_core::portable::memory::unlink(store, &id))
    })
}

#[tauri::command]
pub fn attach_evidence(
    state: tauri::State<'_, AppState>,
    node: NodeId,
    file: String,
    symbol: Option<String>,
    excerpt: Option<String>,
    kind: Option<String>,
) -> std::result::Result<Evidence, String> {
    run_guarded("attach_evidence", || {
        let kind = match &kind {
            Some(raw) => parse(raw, "evidence kind")?,
            None => EvidenceKind::SourceFile,
        };
        state.with(|store| {
            let mut anchor = SourceAnchor::file(&file);
            if let Some(symbol) = &symbol {
                anchor = anchor.with_symbol(symbol);
            }
            let mut evidence = Evidence::new(node.clone(), kind).with_anchor(anchor);
            if let Some(excerpt) = &excerpt {
                evidence = evidence.with_excerpt(excerpt);
            }
            store.attach_evidence(evidence)
        })
    })
}

#[tauri::command]
pub fn search(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> std::result::Result<Vec<SearchHit>, String> {
    run_guarded("search", || {
        state.with(|store| WikiService::new(store).search(&query, limit))
    })
}

#[tauri::command]
pub fn navigate_from(
    state: tauri::State<'_, AppState>,
    id: NodeId,
    depth: Option<u8>,
    max_nodes: Option<usize>,
) -> std::result::Result<Vec<Reached>, String> {
    run_guarded("navigate_from", || {
        state.with(|store| WikiService::new(store).navigate(id.as_str(), depth, max_nodes))
    })
}

#[tauri::command]
pub fn assemble_brief(
    state: tauri::State<'_, AppState>,
    task: String,
    seeds: Option<Vec<NodeId>>,
    depth: Option<u8>,
) -> std::result::Result<KnowledgeBrief, String> {
    run_guarded("assemble_brief", || {
        state.with(|store| {
            let seeds = seeds
                .unwrap_or_default()
                .into_iter()
                .map(NodeId::into_string)
                .collect::<Vec<_>>();
            Ok(WikiService::new(store)
                .context(&task, &seeds, depth, None)?
                .brief)
        })
    })
}

#[tauri::command]
pub fn journal(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> std::result::Result<Vec<Event>, String> {
    run_guarded("journal", || {
        state.with(|store| store.events(&EventFilter::default().limit(limit.unwrap_or(50))))
    })
}

/// The vocabularies the UI needs to populate its dropdowns.
///
/// A typed struct rather than a hand-built `serde_json::Value`: the key
/// names are half of a contract with `src/api.ts`, and a hand-written key is
/// a contract the compiler cannot check.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vocabularies {
    pub node_types: Vec<&'static str>,
    pub edge_types: Vec<&'static str>,
    pub authorities: Vec<&'static str>,
    pub scopes: Vec<&'static str>,
    pub evidence_kinds: Vec<&'static str>,
}

/// Rebuild the derived index over the vault's markdown.
///
/// Cheap enough to call after every save: the indexer writes only what
/// changed, and a pass over an unchanged vault touches nothing. The vault
/// is the project directory containing `.athenaeum/`, so no path is passed
/// — asking the UI which vault to index would let the two disagree.
#[tauri::command]
pub fn index_vault(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<VaultIndexReport, String> {
    run_guarded("index_vault", || {
        let root = state.project_root()?;
        state.with(|store| VaultIndexer::scan_and_index(store, &root, VaultScanOptions::default()))
    })
}

/// What the wiki links at, and what links at nothing.
///
/// The lint panel used to answer this by walking `wiki/` and resolving every
/// `[[link]]` in TypeScript, beside the copy of those rules that already
/// lives in the indexer. One implementation, in the library, where it is
/// tested against a real vault.
#[tauri::command]
pub fn lint_vault(
    state: tauri::State<'_, AppState>,
    options: Option<LintOptions>,
) -> std::result::Result<VaultLintReport, String> {
    run_guarded("lint_vault", || {
        let root = state.project_root()?;
        let options = options.unwrap_or_default();
        state.with(|store| lint::scan_and_lint(store, &root, VaultScanOptions::default(), &options))
    })
}

/// The whole live knowledge graph, in one read, for the graph view.
#[tauri::command]
pub fn knowledge_graph(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<KnowledgeGraph, String> {
    run_guarded("knowledge_graph", || {
        state.with(|store| athenaeum_core::vault::graph::project(store))
    })
}

/// Served from the library rather than duplicated in TypeScript, so adding a
/// node type in Rust cannot leave the UI silently unable to offer it.
#[tauri::command]
pub fn vocabularies() -> Vocabularies {
    fn spellings<T: 'static>(
        all: &'static [T],
        as_str: fn(&T) -> &'static str,
    ) -> Vec<&'static str> {
        all.iter().map(as_str).collect()
    }

    Vocabularies {
        node_types: spellings(NodeType::ALL, NodeType::as_str),
        edge_types: spellings(EdgeType::ALL, EdgeType::as_str),
        authorities: spellings(Authority::ALL, Authority::as_str),
        scopes: spellings(Scope::ALL, Scope::as_str),
        evidence_kinds: spellings(EvidenceKind::ALL, EvidenceKind::as_str),
    }
}
