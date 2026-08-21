use std::sync::LazyLock;

use regex::Regex;

use super::{RepositoryImport, RepositorySymbol, SourceLanguage, SymbolKind};

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

static PY_DECL: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^(async\s+def|def|class)\s+([A-Za-z_][A-Za-z0-9_]*)"));
static PY_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:from\s+([.A-Za-z_][.A-Za-z0-9_]*)\s+import|import\s+([.A-Za-z_][.A-Za-z0-9_]*))",
    )
});

static GO_DECL: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"^(func|type|const|var)\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)")
});
static GO_IMPORT: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"^import\s+(?:[A-Za-z_.][A-Za-z0-9_.]*\s+)?"([^"]+)""#));

pub(super) fn parse(
    language: SourceLanguage,
    content: &str,
) -> crate::Result<(Vec<RepositorySymbol>, Vec<RepositoryImport>)> {
    match language {
        SourceLanguage::Rust => parse_rust(content),
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => parse_javascript(content),
        SourceLanguage::Python => parse_python(content),
        SourceLanguage::Go => parse_go(content),
    }
}

fn parse_rust(content: &str) -> crate::Result<(Vec<RepositorySymbol>, Vec<RepositoryImport>)> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut implementation: Option<(String, i32)> = None;
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

    for (index, line) in cleaned.lines().enumerate() {
        let line_number = index as u32 + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if implementation.is_none() {
            if let Some(captures) = compiled(&RUST_IMPL, "Rust")?.captures(trimmed) {
                implementation = Some((simplify_type(&captures[1]), depth));
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
            symbols.push(symbol(name, qualified_name, kind, line_number));
        }
        if let Some(captures) = compiled(&RUST_MOD, "Rust")?.captures(trimmed) {
            imports.push(import(format!("mod:{}", &captures[1]), line_number));
        }

        depth += brace_delta(line);
        if implementation
            .as_ref()
            .is_some_and(|(_, implementation_depth)| depth <= *implementation_depth)
        {
            implementation = None;
        }
    }
    deduplicate(&mut symbols, &mut imports);
    Ok((symbols, imports))
}

fn parse_javascript(
    content: &str,
) -> crate::Result<(Vec<RepositorySymbol>, Vec<RepositoryImport>)> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut class: Option<(String, i32)> = None;
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

    for (index, line) in cleaned.lines().enumerate() {
        let line_number = index as u32 + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(captures) = compiled(&JS_DECL, "JavaScript/TypeScript")?.captures(trimmed) {
            let raw_kind = &captures[1];
            let name = captures[2].to_string();
            symbols.push(symbol(
                name.clone(),
                name.clone(),
                js_kind(raw_kind),
                line_number,
            ));
            if raw_kind == "class" {
                class = Some((name, depth));
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
            ));
        } else if let (Some((owner, class_depth)), Some(captures)) = (
            class.as_ref(),
            compiled(&JS_METHOD, "JavaScript/TypeScript")?.captures(trimmed),
        ) {
            if depth > *class_depth && &captures[1] != "constructor" {
                let name = captures[1].to_string();
                symbols.push(symbol(
                    name.clone(),
                    format!("{owner}.{name}"),
                    SymbolKind::Method,
                    line_number,
                ));
            }
        }

        for captures in compiled(&JS_REQUIRE, "JavaScript/TypeScript")?.captures_iter(trimmed) {
            imports.push(import(&captures[1], line_number));
        }

        depth += brace_delta(line);
        if class
            .as_ref()
            .is_some_and(|(_, class_depth)| depth <= *class_depth)
        {
            class = None;
        }
    }
    deduplicate(&mut symbols, &mut imports);
    Ok((symbols, imports))
}

fn parse_python(content: &str) -> crate::Result<(Vec<RepositorySymbol>, Vec<RepositoryImport>)> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut class: Option<(String, usize)> = None;
    let mut triple_quote: Option<&str> = None;

    for (index, line) in content.lines().enumerate() {
        let line_number = index as u32 + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if python_string_line(trimmed, &mut triple_quote) {
            continue;
        }
        let indent = line.len() - trimmed.len();
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
                class = Some((name.clone(), indent));
                (SymbolKind::Class, name.clone())
            };
            symbols.push(symbol(name, qualified_name, kind, line_number));
        }
        if let Some(captures) = compiled(&PY_IMPORT, "Python")?.captures(trimmed) {
            if let Some(specifier) = captures.get(1).or_else(|| captures.get(2)) {
                imports.push(import(specifier.as_str(), line_number));
            }
        }
    }
    deduplicate(&mut symbols, &mut imports);
    Ok((symbols, imports))
}

fn parse_go(content: &str) -> crate::Result<(Vec<RepositorySymbol>, Vec<RepositoryImport>)> {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut import_block = false;
    let mut block_comment = false;
    for (index, line) in content.lines().enumerate() {
        let line_number = index as u32 + 1;
        let code = strip_c_style_comments(line, &mut block_comment, true, true);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "import (" {
            import_block = true;
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
            symbols.push(symbol(name.clone(), name, kind, line_number));
        }
    }
    deduplicate(&mut symbols, &mut imports);
    Ok((symbols, imports))
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

fn symbol(name: String, qualified_name: String, kind: SymbolKind, line: u32) -> RepositorySymbol {
    RepositorySymbol {
        name,
        qualified_name,
        kind,
        line_start: line,
        line_end: line,
    }
}

fn import(specifier: impl Into<String>, line: u32) -> RepositoryImport {
    RepositoryImport {
        specifier: specifier.into(),
        line,
        target: None,
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

fn simplify_type(raw: &str) -> String {
    raw.rsplit("::")
        .next()
        .unwrap_or(raw)
        .split('<')
        .next()
        .unwrap_or(raw)
        .to_string()
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, ch| match ch {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_parser_finds_containment_and_import_facts() {
        let source = r#"
use crate::store::{KnowledgeStore, NodeFilter};
pub struct WikiService;
impl WikiService {
    pub fn open(&self) {}
}
mod protocol;
"#;
        let (symbols, imports) = parse(SourceLanguage::Rust, source).unwrap();
        assert!(symbols
            .iter()
            .any(|value| value.qualified_name == "WikiService"));
        assert!(symbols
            .iter()
            .any(|value| value.qualified_name == "WikiService::open"));
        assert!(imports
            .iter()
            .any(|value| value.specifier == "crate::store::{KnowledgeStore, NodeFilter}"));
        assert!(imports
            .iter()
            .any(|value| value.specifier == "mod:protocol"));
    }

    #[test]
    fn typescript_parser_finds_methods_and_relative_imports() {
        let source = r#"
import { Store } from "./store";
export class WikiService {
  async open(id: string): Promise<void> {}
}
export const createWiki = () => new WikiService();
"#;
        let (symbols, imports) = parse(SourceLanguage::TypeScript, source).unwrap();
        assert!(symbols
            .iter()
            .any(|value| value.qualified_name == "WikiService.open"));
        assert!(symbols
            .iter()
            .any(|value| value.qualified_name == "createWiki"));
        assert_eq!(imports[0].specifier, "./store");
    }

    #[test]
    fn python_parser_qualifies_methods() {
        let source = "from .store import Store\nclass Wiki:\n    def open(self):\n        pass\n";
        let (symbols, imports) = parse(SourceLanguage::Python, source).unwrap();
        assert!(symbols
            .iter()
            .any(|value| value.qualified_name == "Wiki.open"));
        assert_eq!(imports[0].specifier, ".store");
    }

    #[test]
    fn declarations_inside_comments_and_docstrings_are_not_indexed() {
        let rust = "/*\npub fn imaginary() {}\n*/\npub fn real() {}\n";
        let (symbols, _) = parse(SourceLanguage::Rust, rust).unwrap();
        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            vec!["real"]
        );

        let python = "\"\"\"\ndef imaginary():\n    pass\n\"\"\"\ndef real():\n    pass\n";
        let (symbols, _) = parse(SourceLanguage::Python, python).unwrap();
        assert_eq!(
            symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            vec!["real"]
        );
    }

    #[test]
    fn multiline_imports_are_extracted_once() {
        let rust = "use crate::{\n    store::KnowledgeStore,\n    wiki::WikiService,\n};\n";
        let (_, imports) = parse(SourceLanguage::Rust, rust).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].line, 1);
        assert!(imports[0].specifier.contains("store::KnowledgeStore"));

        let typescript = "import {\n  WikiService,\n  WikiRequest,\n} from './wiki';\n";
        let (_, imports) = parse(SourceLanguage::TypeScript, typescript).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].specifier, "./wiki");
        assert_eq!(imports[0].line, 1);
    }
}
