//! Line-oriented declaration, import and reference extraction.
//!
//! This is deliberately not a compiler front end. It reads what a file says
//! about itself — what it declares, what it imports, what each declaration
//! invokes, and what supertypes it announces — and it stops where a real
//! parser would have to start guessing. Everything it produces is checked
//! again by the indexer against the rest of the repository before it becomes
//! a relationship, so a false positive here costs an edge, never a wrong one
//! (§21).

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::{
    RepositoryCall, RepositoryImport, RepositoryRelation, RepositorySymbol, SourceLanguage,
    SymbolKind,
};
use super::ExtractionMethod;

static RUST_DECL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(struct|enum|trait|type|fn|const|static|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
});
static RUST_IMPL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^impl(?:<[^>]+>)?\s+(?:[A-Za-z_][A-Za-z0-9_:<>]*\s+for\s+)?([A-Za-z_][A-Za-z0-9_:<>]*)",
    )
});
/// `impl Trait for Type` — both halves, so the supertype survives (§19).
static RUST_IMPL_FOR: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^impl(?:<[^>]+>)?\s+([A-Za-z_][A-Za-z0-9_:]*(?:<[^>]*>)?)\s+for\s+([A-Za-z_][A-Za-z0-9_:]*)",
    )
});
static RUST_USE: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*(?:pub\s+)?use\s+([^;]+);"));
static RUST_MOD: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;"));

static JS_DECL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(?:export|declare|default|abstract)\s+)*(?:async\s+)?(class|interface|type|enum|function)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
});
static JS_CONST_FN: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(?:export|declare)\s+)*(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*=>",
    )
});
static JS_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^[ \t]*(?:import(?:[\s\S]*?\sfrom\s+)?|export[\s\S]*?\sfrom\s+)(?:["'])([^"']+)["']"#,
    )
});
static JS_REQUIRE: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"(?:require|import)\(\s*["']([^"']+)["']\s*\)"#));
static JS_METHOD: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(?:public|private|protected|static|async|readonly|abstract|get|set)\s+)*([A-Za-z_$][A-Za-z0-9_$]*)\s*(?:<[^>]+>)?\s*\([^;]*\)\s*(?::[^={]+)?\s*\{",
    )
});
static JS_EXTENDS: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"\bextends\s+([A-Za-z_$][A-Za-z0-9_$.,<>\s]*?)\s*(?:\bimplements\b|\{|$)")
});
static JS_IMPLEMENTS: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\bimplements\s+([A-Za-z_$][A-Za-z0-9_$.,<>\s]*?)\s*(?:\{|$)"));

static PY_DECL: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^(async\s+def|def|class)\s+([A-Za-z_][A-Za-z0-9_]*)"));
static PY_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:from\s+([.A-Za-z_][.A-Za-z0-9_]*)\s+import|import\s+([.A-Za-z_][.A-Za-z0-9_]*))",
    )
});
static PY_BASES: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^class\s+[A-Za-z_][A-Za-z0-9_]*\s*\(([^)]*)\)"));

static GO_DECL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"^(func|type|const|var)\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)")
});
static GO_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"^import\s+(?:[A-Za-z_.][A-Za-z0-9_.]*\s+)?"([^"]+)""#));

/// An identifier applied to an argument list, with whatever reached it.
/// Macros escape it for free: the `!` in `println!(` sits between the name
/// and the parenthesis.
static CALL_SITE: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(\.|::)?([A-Za-z_$][A-Za-z0-9_$]*)\s*\("));

/// Words that take an argument list without naming anything.
///
/// The list stays short on purpose. A call only becomes an edge if it names a
/// declaration this repository actually holds, so ordinary library calls fall
/// away without having to be enumerated here — only keywords that could
/// collide with a project's own declaration names need listing.
const CALL_KEYWORDS: &[&str] = &[
    "and",
    "assert",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "def",
    "defer",
    "delete",
    "do",
    "elif",
    "else",
    "enum",
    "except",
    "export",
    "extends",
    "fn",
    "for",
    "func",
    "function",
    "go",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "is",
    "lambda",
    "let",
    "loop",
    "match",
    "mod",
    "new",
    "not",
    "or",
    "pub",
    "raise",
    "return",
    "select",
    "sizeof",
    "static",
    "struct",
    "switch",
    "trait",
    "type",
    "typeof",
    "unsafe",
    "use",
    "var",
    "while",
    "with",
    "yield",
];

/// What one file said about itself, and how it was read.
#[derive(Debug, Default)]
pub(super) struct ParsedFile {
    pub symbols: Vec<RepositorySymbol>,
    pub imports: Vec<RepositoryImport>,
    pub implements: Vec<RepositoryRelation>,
    /// Call sites, attributed to their declaration by [`finish`].
    pub calls: Vec<(u32, RepositoryCall)>,
    pub method: ExtractionMethod,
    pub detail: Option<String>,
}

/// Read one file of a supported language, without a path to attribute it to.
///
/// Retained because callers that already know the language — and the pinned
/// tests that prove the fallback reader still reads what it always read — do
/// not all have a repository-relative path in hand. The path only ever refines
/// the attribution a parser can do, never whether a file is read at all.
///
/// This is the entry point for a caller that has content but no path — and the
/// pinned entry point the preserved inline tests exercise. Keep its signature.
#[allow(dead_code)]
pub(super) fn parse(language: SourceLanguage, content: &str) -> crate::Result<ParsedFile> {
    parse_with_path(language, None, content)
}

/// Read one file of a supported language, attributing it to `path` where the
/// parser can use it.
///
/// A real parser goes first, because it is the only reader here that can tell a
/// declaration from a word in a comment. The line reader is the fallback for a
/// file the parser could not make sense of, and the result says which of the
/// two produced it.
pub(super) fn parse_with_path(
    language: SourceLanguage,
    path: Option<&str>,
    content: &str,
) -> crate::Result<ParsedFile> {
    if let Some(parsed) = super::ast::parse_with_tree(language, path, content) {
        // The attribution, ordering and de-duplication passes are shared with
        // the fallback reader, so the two paths cannot disagree about which
        // declaration a call belongs to.
        let mut finished = finish(
            parsed.symbols,
            parsed.imports,
            parsed.implements,
            parsed.calls,
        )?;
        finished.method = parsed.method;
        finished.detail = parsed.detail;
        return Ok(finished);
    }
    let parsed = match language {
        SourceLanguage::Rust => parse_rust(content)?,
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => parse_javascript(content)?,
        SourceLanguage::Python => parse_python(content)?,
        SourceLanguage::Go => parse_go(content)?,
    };
    Ok(parsed
        .with_method(
            ExtractionMethod::Fallback,
            format!(
                "the {} parser reported syntax errors, so this file was read line by line",
                language.as_str()
            ),
        ))
}

/// Read a file of a language this build has no parser for.
///
/// The alternative was silence, and silence is the one answer that is actually
/// wrong: a Java or C# file contributes nothing to the graph and says nothing
/// about having contributed nothing. This recognises the declaration and import
/// shapes those languages share, marks every symbol it produces as a fallback,
/// and leaves the confidence to [`ExtractionMethod::Fallback`].
pub(super) fn parse_unsupported(path: &str, content: &str) -> crate::Result<ParsedFile> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut call_sites = Vec::new();
    let mut spans = BraceSpans::default();
    let mut depth = 0_i32;
    let mut last_line = 1;
    let mut block_comment = false;

    for (index, line) in content.lines().enumerate() {
        let line_number = index as u32 + 1;
        last_line = line_number;
        let code = strip_c_style_comments(line, &mut block_comment, true, true);
        let trimmed = code.trim();
        let scan = scan_braces(&code, depth);
        if trimmed.is_empty() {
            spans.advance(&mut symbols, line_number, &scan, false);
            depth = scan.end;
            continue;
        }

        if let Some(captures) = compiled(&GENERIC_DECL, "unsupported language")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            let kind = generic_kind(raw_kind);
            let exported = !trimmed.contains("private")
                && !trimmed.contains("protected")
                && !trimmed.contains("internal");
            symbols.push(symbol(name.clone(), name, kind, line_number, exported));
            // Only a declaration that opens a brace has a body to follow. In a
            // language that closes with `end`, the line itself is all we claim.
            if scan.max > depth {
                spans.open(symbols.len() - 1, depth);
            }
        }
        if let Some(captures) = compiled(&GENERIC_IMPORT, "unsupported language")?.captures(trimmed) {
            if let Some(specifier) = captures.get(1).or_else(|| captures.get(2)) {
                imports.push(import(specifier.as_str(), line_number));
            }
        }

        collect_calls(&mut call_sites, &code, line_number)?;
        spans.advance(&mut symbols, line_number, &scan, ends_statement(&code));
        depth = scan.end;
    }

    spans.close_remaining(&mut symbols, last_line);
    let mut parsed = finish(symbols, imports, Vec::new(), call_sites)?;
    parsed.detail = Some(format!(
        "no parser for this language; {path} was read line by line at lower confidence"
    ));
    Ok(parsed)
}

impl ParsedFile {
    fn with_method(mut self, method: ExtractionMethod, detail: impl Into<String>) -> Self {
        self.method = method;
        self.detail = Some(detail.into());
        self
    }
}

/// Declaration shapes shared by the languages without a grammar here: the
/// C family, Java, Kotlin, C#, Ruby, PHP, Swift and the rest.
static GENERIC_DECL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(?:public|private|protected|internal|static|final|abstract|sealed|open|override|virtual|async|export|declare|pub|unsafe|extern|partial|data|inline|constexpr)\s+)*(func|function|fun|def|class|interface|struct|enum|trait|type|typedef|const|var|let|val|record|protocol|extension|namespace|module|object|package)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
});

/// Import shapes shared by the same languages.
static GENERIC_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r#"^(?:import|using|use|#include|#import|require|include|from)\s+(?:static\s+)?(?:"([^"]+)"|<([^>]+)>|([A-Za-z_][A-Za-z0-9_.:]*))"#,
    )
});

fn parse_rust(content: &str) -> crate::Result<ParsedFile> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut implements = Vec::new();
    let mut call_sites = Vec::new();
    let mut spans = BraceSpans::default();
    let mut implementation: Option<(String, i32)> = None;
    let mut test_modules = Vec::new();
    let mut cfg_test_pending = false;
    let mut depth = 0_i32;
    let cleaned = strip_comments_from_content(content, false, false);

    for captures in compiled(&RUST_USE, "Rust")?.captures_iter(&cleaned) {
        if let (Some(whole), Some(specifier)) = (captures.get(0), captures.get(1)) {
            imports.push(import(
                specifier
                    .as_str()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                line_number_at(&cleaned, whole.start()),
            ));
        }
    }

    let mut last_line = 1;
    for (index, line) in cleaned.lines().enumerate() {
        let line_number = index as u32 + 1;
        last_line = line_number;
        let trimmed = line.trim_start();
        let scan = scan_braces(line, depth);
        if trimmed.is_empty() {
            spans.advance(&mut symbols, line_number, &scan, false);
            depth = scan.end;
            continue;
        }

        // `#[cfg(test)]` sits on the line above the item it guards, possibly
        // with further attributes between.
        if trimmed.starts_with("#[") {
            cfg_test_pending |= trimmed.contains("cfg(test)");
            spans.advance(&mut symbols, line_number, &scan, ends_statement(line));
            depth = scan.end;
            continue;
        }

        if implementation.is_none() {
            if let Some(captures) = compiled(&RUST_IMPL, "Rust")?.captures(trimmed) {
                implementation = Some((simplify_type(&captures[1]), depth));
            }
            if let Some(captures) = compiled(&RUST_IMPL_FOR, "Rust")?.captures(trimmed) {
                implements.push(relation(
                    simplify_type(&captures[2]),
                    simplify_type(&captures[1]),
                    line_number,
                ));
            }
        }

        if let Some(captures) = compiled(&RUST_DECL, "Rust")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            let in_impl = implementation
                .as_ref()
                .is_some_and(|(_, implementation_depth)| depth > *implementation_depth);
            let method_owner = implementation
                .as_ref()
                .filter(|_| raw_kind == "fn" && in_impl)
                .map(|(owner, _)| owner);
            let (kind, qualified_name) = match method_owner {
                Some(owner) => (SymbolKind::Method, format!("{owner}::{name}")),
                None => (rust_kind(raw_kind), name.clone()),
            };
            let exported = trimmed.starts_with("pub");
            symbols.push(symbol(name, qualified_name, kind, line_number, exported));
            if cfg_test_pending && kind == SymbolKind::Module {
                test_modules.push(symbols.len() - 1);
            }
            spans.open(symbols.len() - 1, depth);
        }
        cfg_test_pending = false;
        if let Some(captures) = compiled(&RUST_MOD, "Rust")?.captures(trimmed) {
            imports.push(import(format!("mod:{}", &captures[1]), line_number));
        }

        collect_calls(&mut call_sites, line, line_number)?;
        spans.advance(&mut symbols, line_number, &scan, ends_statement(line));
        depth = scan.end;
        if implementation
            .as_ref()
            .is_some_and(|(_, implementation_depth)| depth <= *implementation_depth)
        {
            implementation = None;
        }
    }
    spans.close_remaining(&mut symbols, last_line);
    mark_test_modules(&mut symbols, &test_modules);
    finish(symbols, imports, implements, call_sites)
}

fn parse_javascript(content: &str) -> crate::Result<ParsedFile> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut implements = Vec::new();
    let mut call_sites = Vec::new();
    let mut spans = BraceSpans::default();
    let mut class: Option<(String, i32, bool)> = None;
    let mut depth = 0_i32;
    let cleaned = strip_comments_from_content(content, true, true);

    for captures in compiled(&JS_IMPORT, "JavaScript/TypeScript")?.captures_iter(&cleaned) {
        if let (Some(whole), Some(specifier)) = (captures.get(0), captures.get(1)) {
            imports.push(import(
                specifier.as_str(),
                line_number_at(&cleaned, whole.start()),
            ));
        }
    }

    let mut last_line = 1;
    for (index, line) in cleaned.lines().enumerate() {
        let line_number = index as u32 + 1;
        last_line = line_number;
        let trimmed = line.trim_start();
        let scan = scan_braces(line, depth);
        if trimmed.is_empty() {
            spans.advance(&mut symbols, line_number, &scan, false);
            depth = scan.end;
            continue;
        }

        if let Some(captures) = compiled(&JS_DECL, "JavaScript/TypeScript")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            let exported = trimmed.starts_with("export") || trimmed.starts_with("declare");
            symbols.push(symbol(
                name.clone(),
                name.clone(),
                js_kind(raw_kind),
                line_number,
                exported,
            ));
            spans.open(symbols.len() - 1, depth);
            if matches!(raw_kind, "class" | "interface") {
                collect_js_supertypes(&mut implements, &name, trimmed, line_number)?;
            }
            if raw_kind == "class" {
                class = Some((name, depth, exported));
            }
        } else if let Some(captures) =
            compiled(&JS_CONST_FN, "JavaScript/TypeScript")?.captures(trimmed)
        {
            let name = captures[1].to_string();
            symbols.push(symbol(
                name.clone(),
                name,
                SymbolKind::Function,
                line_number,
                trimmed.starts_with("export"),
            ));
            spans.open(symbols.len() - 1, depth);
        } else if let (Some((owner, class_depth, class_exported)), Some(captures)) = (
            class.as_ref(),
            compiled(&JS_METHOD, "JavaScript/TypeScript")?.captures(trimmed),
        ) {
            if depth > *class_depth && &captures[1] != "constructor" {
                let name = captures[1].to_string();
                let exported = *class_exported && !trimmed.starts_with("private");
                symbols.push(symbol(
                    name.clone(),
                    format!("{owner}.{name}"),
                    SymbolKind::Method,
                    line_number,
                    exported,
                ));
                spans.open(symbols.len() - 1, depth);
            }
        }

        for captures in compiled(&JS_REQUIRE, "JavaScript/TypeScript")?.captures_iter(trimmed) {
            imports.push(import(&captures[1], line_number));
        }

        collect_calls(&mut call_sites, line, line_number)?;
        spans.advance(&mut symbols, line_number, &scan, ends_statement(line));
        depth = scan.end;
        if class
            .as_ref()
            .is_some_and(|(_, class_depth, _)| depth <= *class_depth)
        {
            class = None;
        }
    }
    spans.close_remaining(&mut symbols, last_line);
    finish(symbols, imports, implements, call_sites)
}

fn parse_python(content: &str) -> crate::Result<ParsedFile> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut implements = Vec::new();
    let mut call_sites = Vec::new();
    let mut spans = IndentSpans::default();
    let mut class: Option<(String, usize)> = None;
    let mut triple_quote: Option<&str> = None;
    let mut last_significant = 1;

    for (index, line) in content.lines().enumerate() {
        let line_number = index as u32 + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if python_string_line(trimmed, &mut triple_quote) {
            last_significant = line_number;
            continue;
        }
        let indent = line.len() - trimmed.len();
        spans.close_to(&mut symbols, indent, last_significant);
        if class
            .as_ref()
            .is_some_and(|(_, class_indent)| indent <= *class_indent)
        {
            class = None;
        }
        if let Some(captures) = compiled(&PY_DECL, "Python")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            let (kind, qualified_name) = if raw_kind.ends_with("def") {
                match &class {
                    Some((owner, _)) => (SymbolKind::Method, format!("{owner}.{name}")),
                    None => (SymbolKind::Function, name.clone()),
                }
            } else {
                collect_python_bases(&mut implements, &name, trimmed, line_number)?;
                class = Some((name.clone(), indent));
                (SymbolKind::Class, name.clone())
            };
            let exported = !name.starts_with('_');
            symbols.push(symbol(name, qualified_name, kind, line_number, exported));
            spans.open(symbols.len() - 1, indent);
        }
        if let Some(captures) = compiled(&PY_IMPORT, "Python")?.captures(trimmed) {
            if let Some(specifier) = captures.get(1).or_else(|| captures.get(2)) {
                imports.push(import(specifier.as_str(), line_number));
            }
        }
        collect_calls(&mut call_sites, trimmed, line_number)?;
        last_significant = line_number;
    }
    spans.close_remaining(&mut symbols, last_significant);
    finish(symbols, imports, implements, call_sites)
}

fn parse_go(content: &str) -> crate::Result<ParsedFile> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut call_sites = Vec::new();
    let mut spans = BraceSpans::default();
    let mut import_block = false;
    let mut block_comment = false;
    let mut depth = 0_i32;
    let mut last_line = 1;

    for (index, line) in content.lines().enumerate() {
        let line_number = index as u32 + 1;
        last_line = line_number;
        let code = strip_c_style_comments(line, &mut block_comment, true, true);
        let trimmed = code.trim();
        let scan = scan_braces(&code, depth);
        if trimmed.is_empty() {
            spans.advance(&mut symbols, line_number, &scan, false);
            depth = scan.end;
            continue;
        }
        if trimmed == "import (" {
            import_block = true;
            spans.advance(&mut symbols, line_number, &scan, false);
            depth = scan.end;
            continue;
        }
        if import_block {
            if trimmed == ")" {
                import_block = false;
            } else if let Some(specifier) = quoted(trimmed) {
                imports.push(import(specifier, line_number));
            }
        } else if let Some(captures) = compiled(&GO_IMPORT, "Go")?.captures(trimmed) {
            imports.push(import(&captures[1], line_number));
        }
        if let Some(captures) = compiled(&GO_DECL, "Go")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            let kind = match raw_kind {
                "func" if trimmed.starts_with("func (") => SymbolKind::Method,
                "func" => SymbolKind::Function,
                "type" => SymbolKind::Type,
                _ => SymbolKind::Constant,
            };
            let exported = name.chars().next().is_some_and(char::is_uppercase);
            symbols.push(symbol(name.clone(), name, kind, line_number, exported));
            spans.open(symbols.len() - 1, depth);
        }
        collect_calls(&mut call_sites, &code, line_number)?;
        spans.advance(&mut symbols, line_number, &scan, ends_statement(&code));
        depth = scan.end;
    }
    spans.close_remaining(&mut symbols, last_line);
    finish(symbols, imports, Vec::new(), call_sites)
}

fn finish(
    mut symbols: Vec<RepositorySymbol>,
    mut imports: Vec<RepositoryImport>,
    mut implements: Vec<RepositoryRelation>,
    call_sites: Vec<(u32, RepositoryCall)>,
) -> crate::Result<ParsedFile> {
    deduplicate(&mut symbols, &mut imports);
    attribute_calls(&mut symbols, call_sites);
    implements
        .sort_by(|a, b| (a.line, &a.subject, &a.object).cmp(&(b.line, &b.subject, &b.object)));
    implements.dedup_by(|a, b| a.subject == b.subject && a.object == b.object);
    Ok(ParsedFile {
        symbols,
        imports,
        implements,
        calls: Vec::new(),
        method: ExtractionMethod::Fallback,
        detail: None,
    })
}

/// Hands every call site to the innermost declaration whose body contains it.
///
/// A name that matches the enclosing declaration is dropped: on a declaration
/// line it is the declaration itself, and inside the body it is recursion,
/// which describes nothing a reader of the graph did not already know.
fn attribute_calls(symbols: &mut [RepositorySymbol], mut call_sites: Vec<(u32, RepositoryCall)>) {
    if symbols.is_empty() || call_sites.is_empty() {
        return;
    }
    call_sites.sort();
    call_sites.dedup();

    let mut open: Vec<usize> = Vec::new();
    let mut next = 0usize;
    for (line, call) in call_sites {
        while next < symbols.len() && symbols[next].line_start <= line {
            while open
                .last()
                .is_some_and(|index| symbols[*index].line_end < symbols[next].line_start)
            {
                open.pop();
            }
            open.push(next);
            next += 1;
        }
        while open
            .last()
            .is_some_and(|index| symbols[*index].line_end < line)
        {
            open.pop();
        }
        let Some(&enclosing) = open.last() else {
            continue;
        };
        if symbols[enclosing].name == call.name {
            continue;
        }
        symbols[enclosing].calls.push(call);
    }

    for symbol in symbols.iter_mut() {
        symbol.calls.sort();
        symbol.calls.dedup();
    }
}

fn collect_calls(
    sites: &mut Vec<(u32, RepositoryCall)>,
    line: &str,
    line_number: u32,
) -> crate::Result<()> {
    static KEYWORDS: LazyLock<HashSet<&'static str>> =
        LazyLock::new(|| CALL_KEYWORDS.iter().copied().collect());
    for captures in compiled(&CALL_SITE, "call site")?.captures_iter(line) {
        let name = &captures[2];
        if !KEYWORDS.contains(name) {
            sites.push((
                line_number,
                RepositoryCall {
                    name: name.to_string(),
                    on_receiver: captures.get(1).is_some(),
                },
            ));
        }
    }
    Ok(())
}

fn collect_js_supertypes(
    implements: &mut Vec<RepositoryRelation>,
    subject: &str,
    line: &str,
    line_number: u32,
) -> crate::Result<()> {
    for regex in [&JS_EXTENDS, &JS_IMPLEMENTS] {
        if let Some(captures) = compiled(regex, "JavaScript/TypeScript")?.captures(line) {
            for object in captures[1].split(',') {
                let object = simplify_type(object.trim());
                if !object.is_empty() {
                    implements.push(relation(subject.to_string(), object, line_number));
                }
            }
        }
    }
    Ok(())
}

fn collect_python_bases(
    implements: &mut Vec<RepositoryRelation>,
    subject: &str,
    line: &str,
    line_number: u32,
) -> crate::Result<()> {
    let Some(captures) = compiled(&PY_BASES, "Python")?.captures(line) else {
        return Ok(());
    };
    for base in captures[1].split(',') {
        let base = base.trim();
        // `metaclass=` and `**kwargs` are class arguments, not base classes.
        if base.is_empty() || base == "object" || base.contains('=') || base.starts_with('*') {
            continue;
        }
        let base = simplify_type(base);
        if !base.is_empty() {
            implements.push(relation(subject.to_string(), base, line_number));
        }
    }
    Ok(())
}

/// Declarations still waiting for the brace that closes them.
///
/// Entries nest, so the innermost is always the one that closes first and a
/// plain stack is enough.
#[derive(Debug, Default)]
struct BraceSpans {
    open: Vec<OpenSpan>,
}

#[derive(Debug)]
struct OpenSpan {
    symbol: usize,
    depth: i32,
    body_seen: bool,
}

impl BraceSpans {
    fn open(&mut self, symbol: usize, depth: i32) {
        self.open.push(OpenSpan {
            symbol,
            depth,
            body_seen: false,
        });
    }

    /// Called once per line, after that line's declarations have been opened.
    ///
    /// `scan.max` rather than `scan.end` decides whether a body was entered,
    /// so a declaration that opens and closes on one line — `fn open() {}` —
    /// is recognised instead of hanging open to the end of the file.
    fn advance(
        &mut self,
        symbols: &mut [RepositorySymbol],
        line: u32,
        scan: &BraceScan,
        terminated: bool,
    ) {
        for span in self.open.iter_mut() {
            if scan.max > span.depth {
                span.body_seen = true;
            }
        }
        while let Some(span) = self.open.last() {
            let closed = scan.end <= span.depth && (span.body_seen || terminated);
            if !closed {
                break;
            }
            symbols[span.symbol].line_end = line;
            self.open.pop();
        }
    }

    fn close_remaining(&mut self, symbols: &mut [RepositorySymbol], line: u32) {
        for span in self.open.drain(..) {
            let symbol = &mut symbols[span.symbol];
            symbol.line_end = symbol.line_end.max(line);
        }
    }
}

/// The Python equivalent: a declaration ends where the indentation returns to
/// its own level or further out.
#[derive(Debug, Default)]
struct IndentSpans {
    open: Vec<(usize, usize)>,
}

impl IndentSpans {
    fn open(&mut self, symbol: usize, indent: usize) {
        self.open.push((symbol, indent));
    }

    fn close_to(&mut self, symbols: &mut [RepositorySymbol], indent: usize, previous: u32) {
        while self
            .open
            .last()
            .is_some_and(|(_, declared)| indent <= *declared)
        {
            let (symbol, _) = self.open.pop().expect("checked above");
            let symbol = &mut symbols[symbol];
            symbol.line_end = symbol.line_end.max(previous);
        }
    }

    fn close_remaining(&mut self, symbols: &mut [RepositorySymbol], line: u32) {
        for (symbol, _) in self.open.drain(..) {
            let symbol = &mut symbols[symbol];
            symbol.line_end = symbol.line_end.max(line);
        }
    }
}

fn compiled(
    regex: &'static LazyLock<std::result::Result<Regex, regex::Error>>,
    language: &'static str,
) -> crate::Result<&'static Regex> {
    regex
        .as_ref()
        .map_err(|error| crate::Error::RepositoryParser {
            language,
            message: error.to_string(),
        })
}

fn symbol(
    name: String,
    qualified_name: String,
    kind: SymbolKind,
    line: u32,
    exported: bool,
) -> RepositorySymbol {
    RepositorySymbol {
        name,
        qualified_name,
        kind,
        line_start: line,
        line_end: line,
        exported,
        calls: Vec::new(),
        test_scoped: false,
        extraction: ExtractionMethod::Fallback,
    }
}

/// Marks everything inside a `#[cfg(test)]` module as test code.
///
/// Run once spans are final, so a module's span is what decides membership
/// rather than the order declarations happen to appear in.
fn mark_test_modules(symbols: &mut [RepositorySymbol], modules: &[usize]) {
    let spans = modules
        .iter()
        .map(|index| (symbols[*index].line_start, symbols[*index].line_end))
        .collect::<Vec<_>>();
    for symbol in symbols.iter_mut() {
        symbol.test_scoped = spans
            .iter()
            .any(|(start, end)| symbol.line_start >= *start && symbol.line_start <= *end);
    }
}

fn import(specifier: impl Into<String>, line: u32) -> RepositoryImport {
    RepositoryImport {
        specifier: specifier.into(),
        line,
        target: None,
    }
}

fn relation(subject: String, object: String, line: u32) -> RepositoryRelation {
    RepositoryRelation {
        subject,
        object,
        line,
    }
}

fn generic_kind(raw: &str) -> SymbolKind {
    match raw {
        "func" | "function" | "fun" | "def" => SymbolKind::Function,
        "class" => SymbolKind::Class,
        "interface" | "protocol" => SymbolKind::Interface,
        "trait" => SymbolKind::Trait,
        "struct" | "record" | "data" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "const" | "var" | "let" | "val" => SymbolKind::Constant,
        "namespace" | "module" | "object" | "package" => SymbolKind::Module,
        _ => SymbolKind::Type,
    }
}

fn rust_kind(raw: &str) -> SymbolKind {
    match raw {
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "trait" => SymbolKind::Trait,
        "type" => SymbolKind::Type,
        "fn" => SymbolKind::Function,
        "mod" => SymbolKind::Module,
        "const" | "static" => SymbolKind::Constant,
        _ => SymbolKind::Type,
    }
}

fn js_kind(raw: &str) -> SymbolKind {
    match raw {
        "class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "enum" => SymbolKind::Enum,
        "function" => SymbolKind::Function,
        _ => SymbolKind::Type,
    }
}

pub(super) fn simplify_type(raw: &str) -> String {
    raw.trim()
        .rsplit("::")
        .next()
        .unwrap_or(raw)
        .rsplit('.')
        .next()
        .unwrap_or(raw)
        .split('<')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

/// Where the brace depth ends up over one line, and how deep it ever got.
#[derive(Debug, Clone, Copy)]
struct BraceScan {
    end: i32,
    max: i32,
}

/// Counts braces outside string literals.
///
/// Quote state resets at the end of the line rather than carrying across one,
/// which keeps a raw string or a template literal from swallowing the rest of
/// a file when it spans lines.
fn scan_braces(line: &str, start: i32) -> BraceScan {
    let mut depth = start;
    let mut max = start;
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' | '`' => quote = Some(character),
            '{' => {
                depth += 1;
                max = max.max(depth);
            }
            '}' => depth -= 1,
            _ => {}
        }
    }
    BraceScan { end: depth, max }
}

/// True when a line closes a declaration that never opened a body, such as
/// `pub struct Marker;` or `mod protocol;`.
fn ends_statement(line: &str) -> bool {
    line.trim_end().ends_with(';')
}

fn strip_c_style_comments(
    line: &str,
    block_comment: &mut bool,
    single_quotes: bool,
    backticks: bool,
) -> String {
    let mut code = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut escaped = false;

    while let Some(character) = chars.next() {
        if *block_comment {
            if character == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            code.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        let is_quote = character == '"'
            || (single_quotes && character == '\'')
            || (backticks && character == '`');
        if is_quote {
            quote = Some(character);
            code.push(character);
        } else if character == '/' && chars.peek() == Some(&'/') {
            break;
        } else if character == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *block_comment = true;
        } else {
            code.push(character);
        }
    }
    code
}

fn strip_comments_from_content(content: &str, single_quotes: bool, backticks: bool) -> String {
    let mut block_comment = false;
    content
        .lines()
        .map(|line| strip_c_style_comments(line, &mut block_comment, single_quotes, backticks))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_number_at(content: &str, offset: usize) -> u32 {
    content[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1
}

fn python_string_line(line: &str, state: &mut Option<&'static str>) -> bool {
    if let Some(delimiter) = *state {
        if line.contains(delimiter) {
            *state = None;
        }
        return true;
    }
    for delimiter in ["\"\"\"", "'''"] {
        if let Some(start) = line.find(delimiter) {
            let after = &line[start + delimiter.len()..];
            if !after.contains(delimiter) {
                *state = Some(delimiter);
            }
            return line[..start].trim().is_empty();
        }
    }
    false
}

fn quoted(line: &str) -> Option<&str> {
    let start = line.find('"')? + 1;
    let end = line[start..].find('"')? + start;
    Some(&line[start..end])
}

fn deduplicate(symbols: &mut Vec<RepositorySymbol>, imports: &mut Vec<RepositoryImport>) {
    symbols.sort_by_key(|value| (value.line_start, value.qualified_name.clone()));
    symbols.dedup_by(|a, b| a.line_start == b.line_start && a.qualified_name == b.qualified_name);
    imports.sort_by_key(|value| (value.line, value.specifier.clone()));
    imports.dedup_by(|a, b| a.line == b.line && a.specifier == b.specifier);
}
