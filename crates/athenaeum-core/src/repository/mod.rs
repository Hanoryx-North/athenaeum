//! Deterministic, read-only repository understanding (buildplan §29/§75).
//!
//! This module indexes structural facts that do not require a model: files,
//! declarations, imports and local Git history. It does not expose a Git or
//! filesystem tool to an agent, mutate the checkout, or execute arbitrary
//! commands. The resulting graph nodes and relationships retain deterministic
//! provenance and are valid on arrival.

mod ast;
mod git;
mod index;
mod parse;
mod scan;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use index::{RepositoryIndexReport, RepositoryIndexer};

/// Hard budgets for a repository scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryScanOptions {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_commits: usize,
    /// Wall-clock ceiling for one scan, in milliseconds. `0` means no ceiling.
    ///
    /// A size budget bounds how much is read; it does not bound how long a
    /// pathological repository takes to read it, and a scan that runs for
    /// minutes is indistinguishable from a hang to the person waiting. When the
    /// ceiling is reached the walk stops and reports itself incomplete, which is
    /// the same honest answer a size budget produces.
    pub max_duration_ms: u64,
    /// Reuse the previous scan's structural facts for files whose size and
    /// modification time are unchanged, instead of reading and parsing them
    /// again.
    ///
    /// This is what makes a re-index of a large repository cheap. It is a
    /// heuristic and it is safe to be wrong about: a file whose content changed
    /// without its size or timestamp moving is caught by the content hash the
    /// index verifies before it writes anything. Set false to read every file
    /// again regardless — what a first scan, or a suspicious one, does.
    pub incremental: bool,
}

impl Default for RepositoryScanOptions {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 1_000_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_commits: 500,
            max_duration_ms: 30_000,
            incremental: true,
        }
    }
}

/// How a file's structure was read.
///
/// The vocabulary is deliberately small and the default is deliberately
/// uninformative: a snapshot deserialized from an older build, or handed in by
/// a caller that did its own parsing, must not be described as something it
/// never claimed to be.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    /// Read by a real parser into a syntax tree, then walked.
    Ast,
    /// Read by a line-oriented extractor. It recognises declarations and the
    /// shapes around them; it does not understand the language, and nothing
    /// derived this way is ever presented as understanding.
    Fallback,
    /// Nothing recorded — an older snapshot, or a caller-supplied one.
    #[default]
    Unknown,
}

impl ExtractionMethod {
    pub const ALL: &'static [ExtractionMethod] = &[
        ExtractionMethod::Ast,
        ExtractionMethod::Fallback,
        ExtractionMethod::Unknown,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ExtractionMethod::Ast => "ast",
            ExtractionMethod::Fallback => "fallback",
            ExtractionMethod::Unknown => "unknown",
        }
    }

    /// The confidence a relationship derived this way carries by default.
    ///
    /// A syntax tree is as certain as reading gets without type resolution, so
    /// its edges are fully confident. A line reader's are not: it can mistake a
    /// word in a string for a declaration, so its edges say so. `Unknown` makes
    /// no claim at all and therefore does not lower anything.
    pub fn confidence(&self) -> f32 {
        match self {
            ExtractionMethod::Ast => 1.0,
            ExtractionMethod::Fallback => 0.6,
            ExtractionMethod::Unknown => 1.0,
        }
    }

    /// The sentence appended to a node's summary, so the graph itself records
    /// which of the two readings produced it.
    pub fn explanation(&self) -> Option<&'static str> {
        match self {
            ExtractionMethod::Ast => Some("Read from a syntax tree."),
            ExtractionMethod::Fallback => {
                Some("Read by a line-oriented fallback, not a syntax tree.")
            }
            ExtractionMethod::Unknown => None,
        }
    }
}

impl std::fmt::Display for ExtractionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// How one file was read, and whether its facts came from the previous scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub method: ExtractionMethod,
    /// Why this method, when there is something to say: which language had no
    /// parser, or that the parser found an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// True when the structural facts were reused from the previous scan
    /// because the file had not changed.
    #[serde(default)]
    pub reused: bool,
}

impl Extraction {
    pub fn new(method: ExtractionMethod, detail: Option<String>) -> Self {
        Self {
            method,
            detail,
            reused: false,
        }
    }
}

/// A snapshot plus what only the scanner knows: what it skipped, how it read
/// each file, and whether it looked at the whole tree.
///
/// Kept beside [`RepositorySnapshot`] rather than inside it, for a plain
/// reason: a snapshot is a storage-independent value other code builds by
/// hand, and widening it would break every such caller to carry metadata only
/// a scan can produce.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryScan {
    pub snapshot: RepositorySnapshot,
    /// Files the walk saw and deliberately did not read, with the reason.
    pub skipped: Vec<crate::status::SkippedFile>,
    /// How each indexed file was read, keyed by repository-relative path.
    pub extractions: BTreeMap<String, Extraction>,
    /// Files whose structural facts came from the previous scan.
    pub reused_files: usize,
    /// True when a caller narrowed this scan to named paths. Absence then
    /// proves nothing, so the indexer must not mark anything stale.
    pub filtered: bool,
}

impl RepositoryScan {
    pub fn new(snapshot: RepositorySnapshot) -> Self {
        Self {
            snapshot,
            ..Self::default()
        }
    }

    /// Narrow the scan to the named files or directories.
    ///
    /// Returns the paths that matched nothing, so a caller asking for a file
    /// that does not exist is told rather than quietly given a smaller answer.
    /// Directories match by prefix, which is what a person typing `src` means.
    pub fn restrict_to(&mut self, paths: &[String]) -> crate::Result<Vec<String>> {
        let mut wanted = Vec::new();
        for path in paths {
            let normalized = path.trim().replace('\\', "/");
            // Named `paths` because that is the request field the caller wrote,
            // and an error about their input should quote it.
            validate_relative_path(&normalized, "paths")?;
            wanted.push(normalized);
        }
        if wanted.is_empty() {
            return Ok(Vec::new());
        }

        let tree = self
            .snapshot
            .files
            .iter()
            .map(|file| file.path.clone())
            .chain(self.skipped.iter().map(|file| file.path.clone()))
            .collect::<Vec<_>>();
        let matching = |candidate: &str| {
            wanted.iter().any(|wanted| {
                candidate == wanted
                    || candidate
                        .strip_prefix(wanted.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        };

        self.snapshot.files.retain(|file| matching(&file.path));
        self.skipped.retain(|file| matching(&file.path));
        self.extractions.retain(|path, _| matching(path));
        self.filtered = true;

        Ok(wanted
            .into_iter()
            .filter(|wanted| {
                !tree.iter().any(|candidate| {
                    candidate == wanted
                        || candidate
                            .strip_prefix(wanted.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            })
            .collect())
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
    /// Whether the declaration is test code although its file is not a test
    /// file — a Rust `#[cfg(test)] mod tests` and everything inside it.
    ///
    /// Languages that keep tests in their own files are covered by
    /// [`RepositoryFile::is_test`]; Rust conventionally does not, so without
    /// this a Rust project's tests reference nothing (§19, §29).
    #[serde(default)]
    pub test_scoped: bool,
    /// How this declaration was read. Carried per declaration rather than per
    /// file because one file can be read by a parser and still contain a
    /// declaration the parser could not place.
    #[serde(default)]
    pub extraction: ExtractionMethod,
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

    /// Scan a repository and keep what only a scan produces: which files were
    /// skipped and why, how each one was read, and how much was reused from the
    /// previous scan.
    pub fn scan_detailed(
        root: impl AsRef<std::path::Path>,
        options: RepositoryScanOptions,
    ) -> crate::Result<RepositoryScan> {
        scan::scan_with_details(root.as_ref(), options)
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
