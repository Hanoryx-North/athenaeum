use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use super::parse;
use super::{RepositoryFile, RepositoryScanOptions, RepositorySnapshot, SourceLanguage};
use crate::error::{Error, Result};
use crate::model::hash;

pub(super) fn scan(root: &Path, options: RepositoryScanOptions) -> Result<RepositorySnapshot> {
    validate_root(root)?;

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut total_bytes = 0_u64;
    let mut complete = true;
    let entries = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !ignored_directory(entry));

    for entry in entries {
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
            complete = false;
            break;
        }

        let metadata = entry.metadata().map_err(walk_error)?;
        if metadata.len() > options.max_file_bytes {
            warnings.push(format!(
                "Skipped {path}: {} bytes exceeds the per-file limit",
                metadata.len()
            ));
            complete = false;
            continue;
        }
        if total_bytes.saturating_add(metadata.len()) > options.max_total_bytes {
            warnings.push(format!(
                "Stopped before {path}: the configured total byte limit was reached"
            ));
            complete = false;
            break;
        }

        let bytes = std::fs::read(entry.path())?;
        let content = match std::str::from_utf8(&bytes) {
            Ok(content) if !content.contains('\0') => content,
            _ => {
                warnings.push(format!("Skipped non-text file {path}"));
                continue;
            }
        };
        total_bytes += metadata.len();
        let language = language_for(&path);
        let parsed = match language {
            Some(language) => parse::parse(language, content)?,
            None => parse::ParsedFile::default(),
        };
        files.push(RepositoryFile {
            path: path.clone(),
            language,
            bytes: metadata.len(),
            content_hash: hash(content),
            is_test: is_test_path(&path),
            is_documentation: is_documentation(&path),
            is_configuration: is_configuration(&path),
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

    Ok(RepositorySnapshot {
        files,
        commits,
        warnings,
        complete,
    })
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
