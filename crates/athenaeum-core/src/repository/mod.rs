//! Deterministic, read-only repository understanding (buildplan §29/§75).
//!
//! This module indexes structural facts that do not require a model: files,
//! declarations, imports and local Git history. It does not expose a Git or
//! filesystem tool to an agent, mutate the checkout, or execute arbitrary
//! commands. The resulting graph nodes and relationships retain deterministic
//! provenance and are valid on arrival.

mod git;
mod index;
mod parse;
mod scan;

use serde::{Deserialize, Serialize};

pub use index::{RepositoryIndexReport, RepositoryIndexer};

/// Hard budgets for a repository scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryScanOptions {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_commits: usize,
}

impl Default for RepositoryScanOptions {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 1_000_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_commits: 500,
        }
    }
}

/// Supported source languages with deterministic declaration/import parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLanguage {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
}

impl SourceLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceLanguage::Rust => "Rust",
            SourceLanguage::TypeScript => "TypeScript",
            SourceLanguage::JavaScript => "JavaScript",
            SourceLanguage::Python => "Python",
            SourceLanguage::Go => "Go",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
    Module,
    Constant,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Interface => "interface",
            SymbolKind::Type => "type",
            SymbolKind::Module => "module",
            SymbolKind::Constant => "constant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub line_start: u32,
    /// The last line of the declaration's body, not of its signature. This is
    /// the region a symbol's evidence anchor covers, so it is also what an
    /// anchor fingerprint has to match after a refactor (§16).
    pub line_end: u32,
    /// Whether the declaration is visible outside the file that declares it
    /// (§29 exports). Deliberately coarse: `pub(crate)` counts, because the
    /// question being answered is "can anything else name this".
    #[serde(default)]
    pub exported: bool,
    /// Names invoked inside the body, exactly as written at the call site
    /// (§30). Unresolved at this stage — the indexer decides which of them
    /// name something this repository actually declares.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<RepositoryCall>,
}

/// One call site, and how it was written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCall {
    pub name: String,
    /// True when the name was reached through something else —
    /// `value.name(…)` or `Type::name(…)`.
    ///
    /// It matters because the two forms can be trusted differently. A bare
    /// name had to be in lexical scope, so if the repository declares exactly
    /// one thing by that name, that is the thing. A name on a receiver is
    /// dispatched on the receiver's type, which a line-oriented reader cannot
    /// see — `values.push(x)` and a project's own `push` are written
    /// identically.
    #[serde(default)]
    pub on_receiver: bool,
}

/// A supertype named at a declaration: a trait implemented, an interface
/// implemented, a class extended, a base class (§19).
///
/// Kept beside the file rather than on the symbol because Rust announces the
/// relationship from an `impl` block, which need not sit in the file that
/// declares the type it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRelation {
    /// The declaration that gains the supertype, as written.
    pub subject: String,
    /// The supertype, as written.
    pub object: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryImport {
    pub specifier: String,
    pub line: u32,
    /// Repository-relative target when the import resolves locally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<SourceLanguage>,
    pub bytes: u64,
    pub content_hash: String,
    pub is_test: bool,
    pub is_documentation: bool,
    pub is_configuration: bool,
    pub symbols: Vec<RepositorySymbol>,
    pub imports: Vec<RepositoryImport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<RepositoryRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub id: String,
    pub author: String,
    pub authored_at: String,
    pub subject: String,
    pub files: Vec<String>,
}

/// A storage-independent structural snapshot. It can be inspected or tested
/// before committing anything to a knowledge graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub files: Vec<RepositoryFile>,
    pub commits: Vec<GitCommit>,
    pub warnings: Vec<String>,
    /// False when a hard scan budget stopped traversal before the tree ended.
    pub complete: bool,
}

pub(crate) fn validate_relative_path(path: &str, field: &'static str) -> crate::Result<()> {
    use std::path::{Component, Path};

    let source = Path::new(path);
    if path.is_empty()
        || path.as_bytes().get(1) == Some(&b':')
        || source.is_absolute()
        || source.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(crate::Error::InvalidValue {
            field,
            value: path.to_string(),
        });
    }
    Ok(())
}

impl RepositorySnapshot {
    pub fn scan(
        root: impl AsRef<std::path::Path>,
        options: RepositoryScanOptions,
    ) -> crate::Result<Self> {
        scan::scan(root.as_ref(), options)
    }
}

pub(crate) fn region_by_lines(content: &str, start: u32, end: u32) -> Option<&str> {
    if start == 0 || end < start {
        return None;
    }
    let mut offsets = content
        .match_indices('\n')
        .map(|(index, _)| index + 1)
        .collect::<Vec<_>>();
    offsets.insert(0, 0);
    offsets.push(content.len());
    let start_index = *offsets.get(start.saturating_sub(1) as usize)?;
    let end_index = *offsets.get((end as usize).min(offsets.len().saturating_sub(1)))?;
    Some(content[start_index..end_index].trim_end_matches(['\n', '\r']))
}
