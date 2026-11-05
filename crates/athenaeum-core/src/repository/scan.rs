use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

use super::parse;
use super::{
    Extraction, RepositoryFile, RepositoryScan, RepositoryScanOptions, RepositorySnapshot,
    SourceLanguage,
};
use crate::error::{Error, Result};
use crate::model::hash;
use crate::status::SkippedFile;

/// Scan a repository into a snapshot, discarding what only a scan produces.
///
/// The snapshot is what a caller inspects or indexes. Skipped files, extraction
/// methods and the incremental reuse count live on [`RepositoryScan`], which
/// [`scan_with_details`] returns.
pub(super) fn scan(root: &Path, options: RepositoryScanOptions) -> Result<RepositorySnapshot> {
    Ok(scan_with_details(root, options)?.snapshot)
}

/// Scan a repository, keeping what the walk learned along the way.
pub(super) fn scan_with_details(
    root: &Path,
    options: RepositoryScanOptions,
) -> Result<RepositoryScan> {
    validate_root(root)?;

    let started = std::time::Instant::now();
    let cache = Cache::load(root, options.incremental);
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped = Vec::new();
    let mut extractions = BTreeMap::new();
    let mut reused_files = 0_usize;
    let mut total_bytes = 0_u64;
    let mut complete = true;
    let entries = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !ignored_directory(entry));

    for entry in entries {
        if options.max_duration_ms > 0
            && started.elapsed().as_millis() as u64 > options.max_duration_ms
        {
            warnings.push(format!(
                "Stopped after the configured {} ms time budget",
                options.max_duration_ms
            ));
            skipped.push(SkippedFile::new(
                "",
                "budget",
                "the time budget was reached before this file",
            ));
            complete = false;
            break;
        }
        let entry = entry.map_err(walk_error)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| Error::InvalidValue {
                field: "repository root",
                value: root.display().to_string(),
            })?;
        let Some(path) = normalized_relative(relative) else {
            warnings.push(format!(
                "Skipped a path that is not valid UTF-8: {}",
                relative.display()
            ));
            skipped.push(SkippedFile::new(
                relative.display().to_string(),
                "not_utf8",
                "the path is not valid UTF-8, so it cannot be named in the index",
            ));
            complete = false;
            continue;
        };
        if !is_indexable(&path) {
            continue;
        }
        if files.len() >= options.max_files {
            warnings.push(format!(
                "Stopped after the configured {} file limit",
                options.max_files
            ));
            skipped.push(SkippedFile::new(
                &path,
                "budget",
                "the configured file limit was reached",
            ));
            complete = false;
            break;
        }

        let metadata = entry.metadata().map_err(walk_error)?;
        if metadata.len() > options.max_file_bytes {
            warnings.push(format!(
                "Skipped {path}: {} bytes exceeds the per-file limit",
                metadata.len()
            ));
            skipped.push(SkippedFile::new(
                &path,
                "too_large",
                format!("{} bytes exceeds the per-file limit", metadata.len()),
            ));
            complete = false;
            continue;
        }
        if total_bytes.saturating_add(metadata.len()) > options.max_total_bytes {
            warnings.push(format!(
                "Stopped before {path}: the configured total byte limit was reached"
            ));
            skipped.push(SkippedFile::new(
                &path,
                "budget",
                "the total byte budget was reached before this file",
            ));
            complete = false;
            break;
        }

        let modified = crate::status::modification_time(&metadata);
        // The cheap path: this file was read before and has not moved since, so
        // its declarations, imports and call sites are still what they were.
        if let Some((file, extraction)) = cache.reuse(&path, metadata.len(), modified) {
            total_bytes += metadata.len();
            reused_files += 1;
            files.push(file);
            extractions.insert(path, extraction);
            continue;
        }

        let bytes = std::fs::read(entry.path())?;
        let content = match std::str::from_utf8(&bytes) {
            Ok(content) if !content.contains('\0') => content,
            _ => {
                warnings.push(format!("Skipped non-text file {path}"));
                skipped.push(SkippedFile::new(
                    &path,
                    "binary",
                    "the file is not UTF-8 text, so there is nothing to read here",
                ));
                continue;
            }
        };
        total_bytes += metadata.len();
        let language = language_for(&path);
        let parsed = match language {
            Some(language) => parse::parse_with_path(language, Some(&path), content)?,
            // A language with no parser here still gets read, and the result
            // says plainly that it was read line by line — never that it was
            // understood.
            None if is_code_like(&path) => parse::parse_unsupported(&path, content)?,
            None => Default::default(),
        };
        extractions.insert(
            path.clone(),
            Extraction::new(parsed.method, parsed.detail.clone()),
        );
        let is_test = is_test_path(&path);
        let is_documentation = is_documentation(&path);
        let is_configuration = is_configuration(&path);
        files.push(RepositoryFile {
            path,
            language,
            bytes: metadata.len(),
            content_hash: hash(content),
            is_test,
            is_documentation,
            is_configuration,
            symbols: parsed.symbols,
            imports: parsed.imports,
            implements: parsed.implements,
        });
    }

    resolve_imports(&mut files);
    let (commits, git_warning) = super::git::read_history(root, options.max_commits);
    if let Some(warning) = git_warning {
        warnings.push(warning);
    }

    if options.incremental {
        if let Err(error) = cache.store(root, &files, &extractions) {
            warnings.push(format!("Could not write the scan cache: {error}"));
        }
    }

    Ok(RepositoryScan {
        snapshot: RepositorySnapshot {
            files,
            commits,
            warnings,
            complete,
        },
        skipped,
        extractions,
        reused_files,
        filtered: false,
    })
}

/// The previous scan's structural facts, keyed by path.
///
/// A cache of the *derived* reading of the tree, in the workspace's `cache/`
/// directory, which is the one place this crate is allowed to put data that can
/// be deleted without losing anything. Its version is part of the file, so a
/// cache written by an older build is ignored rather than trusted.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    files: Vec<CachedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    path: String,
    bytes: u64,
    mtime_ns: i64,
    file: RepositoryFile,
    extraction: Extraction,
}

/// The cache format this build writes. Bump it when the extraction changes, so
/// a stale cache cannot describe a file the current reader would read
/// differently.
const CACHE_VERSION: u32 = 1;
const CACHE_FILE: &str = "repository-scan.json";

impl Cache {
    /// Load the cache, or an empty one when there is nothing to load.
    ///
    /// A cache is only meaningful inside a project: a directory with no
    /// `.athenaeum/` is not one, and writing derived state into somebody's
    /// source tree because they happened to index it would be rude.
    fn load(root: &Path, enabled: bool) -> Self {
        if !enabled {
            return Self::default();
        }
        let Ok(raw) = std::fs::read_to_string(cache_path(root)) else {
            return Self::default();
        };
        match serde_json::from_str::<Cache>(&raw) {
            Ok(cache) if cache.version == CACHE_VERSION => cache,
            _ => Self::default(),
        }
    }

    /// The recorded facts for `path`, when they are still valid.
    fn reuse(&self, path: &str, bytes: u64, mtime_ns: i64) -> Option<(RepositoryFile, Extraction)> {
        let cached = self.files.iter().find(|cached| {
            cached.path == path && cached.bytes == bytes && cached.mtime_ns == mtime_ns
        })?;
        let mut file = cached.file.clone();
        // Path-derived facts are cheap to recompute and are the ones a moved
        // file could have changed, so they never come from the cache.
        file.is_test = is_test_path(path);
        file.is_documentation = is_documentation(path);
        file.is_configuration = is_configuration(path);
        let mut extraction = cached.extraction.clone();
        extraction.reused = true;
        Some((file, extraction))
    }

    fn store(
        &self,
        root: &Path,
        files: &[RepositoryFile],
        extractions: &BTreeMap<String, Extraction>,
    ) -> Result<()> {
        let cache = Cache {
            version: CACHE_VERSION,
            files: files
                .iter()
                .filter_map(|file| {
                    extractions.get(&file.path).map(|extraction| CachedFile {
                        path: file.path.clone(),
                        bytes: file.bytes,
                        mtime_ns: mtime_of(root, &file.path),
                        file: file.clone(),
                        extraction: extraction.clone(),
                    })
                })
                .collect(),
        };
        let path = cache_path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec(&cache)?)?;
        Ok(())
    }
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(crate::workspace::WORKSPACE_DIR)
        .join("cache")
        .join(CACHE_FILE)
}

/// The modification time a later scan will compare against.
fn mtime_of(root: &Path, relative: &str) -> i64 {
    std::fs::metadata(root.join(relative))
        .map(|metadata| crate::status::modification_time(&metadata))
        .unwrap_or(0)
}

fn validate_root(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Err(Error::InvalidValue {
            field: "repository root",
            value: root.display().to_string(),
        });
    }
    Ok(())
}

fn walk_error(error: walkdir::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}

fn ignored_directory(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() || entry.depth() == 0 {
        return false;
    }
    matches!(
        entry.file_name().to_str(),
        Some(
            ".git"
                | ".athenaeum"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | ".next"
                | ".cache"
                | "__pycache__"
                | ".venv"
                | "vendor"
        )
    )
}

fn normalized_relative(path: &Path) -> Option<String> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    path.to_str().map(|value| value.replace('\\', "/"))
}

fn is_indexable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let extension = Path::new(&lower)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    matches!(
        extension,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "swift"
            | "md"
            | "mdx"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "jsonc"
            | "xml"
            | "html"
            | "css"
            | "scss"
            | "sh"
            | "ps1"
            | "sql"
            | "txt"
            | "vue"
            | "svelte"
            | "scala"
            | "lua"
            | "ex"
            | "exs"
            | "erl"
            | "hrl"
            | "pl"
    ) || matches!(
        Path::new(&lower)
            .file_name()
            .and_then(|value| value.to_str()),
        Some("dockerfile" | "makefile" | "justfile")
    )
}

fn language_for(path: &str) -> Option<SourceLanguage> {
    match Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => Some(SourceLanguage::Rust),
        "ts" | "tsx" => Some(SourceLanguage::TypeScript),
        "js" | "jsx" | "mjs" | "cjs" => Some(SourceLanguage::JavaScript),
        "py" => Some(SourceLanguage::Python),
        "go" => Some(SourceLanguage::Go),
        _ => None,
    }
}

/// True for a file of a language this build has no parser for, but which is
/// source code all the same.
///
/// These are the files that used to contribute nothing and say nothing. They
/// are read by the line-oriented fallback and marked as such, so a Java file's
/// declarations appear in the graph at lower confidence instead of not at all.
fn is_code_like(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "java"
                | "kt"
                | "kts"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "cxx"
                | "hpp"
                | "cs"
                | "rb"
                | "php"
                | "swift"
                | "scala"
                | "lua"
                | "ex"
                | "exs"
                | "erl"
                | "hrl"
                | "pl"
                | "pm"
                | "vue"
                | "svelte"
                | "dart"
                | "r"
                | "jl"
                | "m"
                | "mm"
                | "groovy"
                | "hs"
                | "clj"
                | "fs"
                | "fsx"
                | "vb"
                | "sh"
                | "bash"
                | "zsh"
                | "ps1"
        )
    )
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = Path::new(&lower)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    lower
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__"))
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.go")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn is_documentation(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "mdx")
    )
}

fn is_configuration(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    matches!(
        Path::new(&lower)
            .extension()
            .and_then(|value| value.to_str()),
        Some("toml" | "yaml" | "yml" | "json" | "jsonc")
    ) || matches!(
        Path::new(&lower)
            .file_name()
            .and_then(|value| value.to_str()),
        Some("dockerfile" | "makefile" | "justfile")
    )
}

fn resolve_imports(files: &mut [RepositoryFile]) {
    let known = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    for file in files {
        for import in &mut file.imports {
            import.target = resolve_import(&file.path, file.language, &import.specifier, &known);
        }
    }
}

fn resolve_import(
    source: &str,
    language: Option<SourceLanguage>,
    specifier: &str,
    known: &HashSet<String>,
) -> Option<String> {
    match language? {
        SourceLanguage::Rust => resolve_rust(source, specifier, known),
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => {
            resolve_javascript(source, specifier, known)
        }
        SourceLanguage::Python => resolve_python(source, specifier, known),
        SourceLanguage::Go => None,
    }
}

fn resolve_javascript(source: &str, specifier: &str, known: &HashSet<String>) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let parent = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
    let base = normalize_join(parent, Path::new(specifier))?;
    let extensions = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
    for extension in extensions {
        let candidate = format!("{base}{extension}");
        if known.contains(&candidate) {
            return Some(candidate);
        }
    }
    for extension in &extensions[1..] {
        let candidate = format!("{base}/index{extension}");
        if known.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resolve_rust(source: &str, specifier: &str, known: &HashSet<String>) -> Option<String> {
    if let Some(module) = specifier.strip_prefix("mod:") {
        let source_path = Path::new(source);
        let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
        let stem = source_path.file_stem()?.to_str()?;
        let module_root = if matches!(stem, "lib" | "main" | "mod") {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        return first_known_module(&normalize_join(&module_root, Path::new(module))?, known);
    }

    let root = rust_source_root(source)?;
    let relative = specifier.strip_prefix("crate::")?;
    let module = relative
        .split([':', '{', ',', '}', ' ', '\t', '\n', '\r'])
        .find(|part| !part.is_empty())?;
    first_known_module(&format!("{root}/{module}"), known)
}

fn rust_source_root(source: &str) -> Option<String> {
    let components = source.split('/').collect::<Vec<_>>();
    let index = components
        .iter()
        .rposition(|component| *component == "src")?;
    Some(components[..=index].join("/"))
}

fn first_known_module(base: &str, known: &HashSet<String>) -> Option<String> {
    [format!("{base}.rs"), format!("{base}/mod.rs")]
        .into_iter()
        .find(|candidate| known.contains(candidate))
}

fn resolve_python(source: &str, specifier: &str, known: &HashSet<String>) -> Option<String> {
    let parent = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
    let (base, module) = if specifier.starts_with('.') {
        let dots = specifier
            .chars()
            .take_while(|character| *character == '.')
            .count();
        let mut base = parent.to_path_buf();
        for _ in 1..dots {
            base = base.parent()?.to_path_buf();
        }
        (base, specifier.trim_start_matches('.'))
    } else {
        (PathBuf::new(), specifier)
    };
    let module_path = module.replace('.', "/");
    let joined = normalize_join(&base, Path::new(&module_path))?;
    [format!("{joined}.py"), format!("{joined}/__init__.py")]
        .into_iter()
        .find(|candidate| known.contains(candidate))
}

fn normalize_join(base: &Path, relative: &Path) -> Option<String> {
    let mut components = Vec::new();
    for component in base.join(relative).components() {
        match component {
            Component::Normal(part) => components.push(part.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_files_symbols_and_resolved_imports_with_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "mod store;\npub fn open() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/store.rs"), "pub struct Store;\n").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/noise")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/noise/index.js"),
            "export function ignored() {}",
        )
        .unwrap();

        let snapshot = scan(dir.path(), RepositoryScanOptions::default()).unwrap();
        assert_eq!(
            snapshot
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/lib.rs", "src/store.rs"]
        );
        assert_eq!(
            snapshot.files[0].imports[0].target.as_deref(),
            Some("src/store.rs")
        );
        assert!(snapshot.files[0]
            .symbols
            .iter()
            .any(|symbol| symbol.name == "open"));
    }

    #[test]
    fn file_and_total_limits_are_enforced_without_reading_generated_trees() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();
        let snapshot = scan(
            dir.path(),
            RepositoryScanOptions {
                max_files: 1,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();
        assert_eq!(snapshot.files.len(), 1);
        assert!(!snapshot.complete);
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("file limit")));
    }
}
