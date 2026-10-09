//! AST-backed extraction, for the languages a real parser is available for.
//!
//! Everything here reads a concrete syntax tree produced by tree-sitter rather
//! than scanning lines, and it is the only extractor in this crate that may
//! claim AST understanding: [`super::ExtractionMethod::Ast`] is set by this
//! module and by nothing else.
//!
//! The distinction is not academic. A line reader cannot tell a declaration
//! inside a comment from a real one, cannot see that `values.push(x)` and a
//! project's own `push` are different names, and cannot know that a call sits
//! in a string literal. A syntax tree knows all three because the grammar said
//! so. So this module is tried first for every supported language, and the
//! line-oriented reader in [`super::parse`] is kept for exactly two jobs: a file
//! this parser could not parse without errors, and a language with no grammar
//! here at all. Both of those outcomes are recorded as
//! [`super::ExtractionMethod::Fallback`], never as understanding.
//!
//! Nothing in here decides what a relationship *means*; it reports names,
//! spans and call sites. [`super::index`] resolves them against the rest of the
//! repository, exactly as it does for the fallback path.

use tree_sitter::{Language, Node, Parser};

use super::parse::{simplify_type, ParsedFile};
use super::{
    ExtractionMethod, RepositoryCall, RepositoryImport, RepositoryRelation, RepositorySymbol,
    SourceLanguage, SymbolKind,
};

/// The tree-sitter grammar for a language, when this build ships one.
fn grammar(language: SourceLanguage, path: Option<&str>) -> Option<Language> {
    match language {
        SourceLanguage::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        SourceLanguage::TypeScript => {
            // `.tsx` needs the JSX-aware grammar: the plain TypeScript grammar
            // reports every JSX element as a syntax error, which would send
            // every React component down the fallback path.
            if path.is_some_and(|path| path.to_ascii_lowercase().ends_with(".tsx")) {
                Some(tree_sitter_typescript::LANGUAGE_TSX.into())
            } else {
                Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
        }
        SourceLanguage::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        SourceLanguage::Python => Some(tree_sitter_python::LANGUAGE.into()),
        SourceLanguage::Go => Some(tree_sitter_go::LANGUAGE.into()),
    }
}

/// Parse `content` into a syntax tree, or `None` when the grammar cannot make
/// sense of it.
///
/// A tree with errors is treated as no tree at all. Partial understanding of a
/// file that is mid-edit is worse than the conservative fallback: the fallback
/// is labelled as such, and an ERROR node silently swallowed would not be.
pub(super) fn parse_with_tree(
    language: SourceLanguage,
    path: Option<&str>,
    content: &str,
) -> Option<ParsedFile> {
    let mut parser = Parser::new();
    parser.set_language(&grammar(language, path)?).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    if root.has_error() || root.is_error() {
        return None;
    }

    let mut out = AstFile::default();
    match language {
        SourceLanguage::Rust => rust::extract(root, content, &mut out),
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => {
            javascript::extract(root, content, &mut out)
        }
        SourceLanguage::Python => python::extract(root, content, &mut out),
        SourceLanguage::Go => go::extract(root, content, &mut out),
    }

    Some(ParsedFile {
        symbols: out.symbols,
        imports: out.imports,
        implements: out.implements,
        calls: out.calls,
        method: ExtractionMethod::Ast,
        detail: Some(format!(
            "parsed into a syntax tree ({})",
            grammar_name(language)
        )),
    })
}

fn grammar_name(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::Rust => "tree-sitter-rust",
        SourceLanguage::TypeScript => "tree-sitter-typescript",
        SourceLanguage::JavaScript => "tree-sitter-javascript",
        SourceLanguage::Python => "tree-sitter-python",
        SourceLanguage::Go => "tree-sitter-go",
    }
}

/// What one pass over a syntax tree collected.
#[derive(Debug, Default)]
pub(super) struct AstFile {
    pub symbols: Vec<RepositorySymbol>,
    pub imports: Vec<RepositoryImport>,
    pub implements: Vec<RepositoryRelation>,
    /// Call sites by line, handed to the shared attribution pass so that the
    /// AST and fallback paths cannot disagree about which declaration a call
    /// belongs to.
    pub calls: Vec<(u32, RepositoryCall)>,
}

impl AstFile {
    /// Record one declaration.
    pub(super) fn symbol(
        &mut self,
        name: String,
        qualified_name: String,
        kind: SymbolKind,
        node: Node<'_>,
        exported: bool,
    ) -> usize {
        self.symbols.push(RepositorySymbol {
            name,
            qualified_name,
            kind,
            line_start: line_of(node.start_position().row),
            line_end: line_of(node.end_position().row),
            exported,
            calls: Vec::new(),
            test_scoped: false,
            extraction: ExtractionMethod::Ast,
        });
        self.symbols.len() - 1
    }

    pub(super) fn import(&mut self, specifier: impl Into<String>, line: u32) {
        self.imports.push(RepositoryImport {
            specifier: specifier.into(),
            line,
            target: None,
        });
    }

    pub(super) fn relation(&mut self, subject: String, object: String, line: u32) {
        if subject.is_empty() || object.is_empty() || subject == object {
            return;
        }
        self.implements.push(RepositoryRelation {
            subject,
            object,
            line,
        });
    }

    pub(super) fn call(&mut self, name: &str, on_receiver: bool, node: Node<'_>) {
        if name.is_empty() || CALL_KEYWORDS.contains(&name) {
            return;
        }
        self.calls.push((
            line_of(node.start_position().row),
            RepositoryCall {
                name: name.to_string(),
                on_receiver,
            },
        ));
    }
}

/// Words that are syntax rather than a name being invoked.
const CALL_KEYWORDS: &[&str] = &[
    "if", "for", "while", "match", "switch", "catch", "return", "new", "await", "typeof", "sizeof",
    "assert", "with", "delete", "in", "of", "do", "else", "not", "and", "or", "is", "as", "yield",
    "print", "select", "go", "defer", "range", "make", "func", "class", "def", "lambda",
];

/// One-based line, which is what the rest of the crate and every editor uses.
pub(super) fn line_of(row: usize) -> u32 {
    row as u32 + 1
}

/// The text of a node, empty when it is not valid UTF-8 in this source.
pub(super) fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    node.utf8_text(content.as_bytes()).unwrap_or("")
}

/// The text of an optional field.
pub(super) fn field_text<'a>(node: Node<'_>, field: &str, content: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field)
        .map(|child| text(child, content))
}

/// The last identifier of a dotted or `::`-separated name.
pub(super) fn last_segment(name: &str) -> &str {
    name.rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(name)
}

/// Whether a node is wrapped in an `export` (JavaScript/TypeScript).
pub(super) fn is_exported(node: Node<'_>, _content: &str) -> bool {
    let mut cursor = node.parent();
    while let Some(parent) = cursor {
        match parent.kind() {
            "export_statement" => return true,
            "class_body" | "statement_block" | "program" => return false,
            _ => {}
        }
        cursor = parent.parent();
    }
    false
}

/// Whether a node carries a Rust `#[cfg(test)]` attribute.
pub(super) fn has_cfg_test(node: Node<'_>, content: &str) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(previous) = sibling {
        match previous.kind() {
            "attribute_item" => {
                if text(previous, content).contains("cfg(test)") {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        sibling = previous.prev_sibling();
    }
    false
}

/// Whether a Rust item is visible outside its module.
pub(super) fn rust_visible(node: Node<'_>) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == "visibility_modifier")
}

/// Pull a quoted string out of a node's text.
pub(super) fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    for quote in ['"', '\'', '`'] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[quote.len_utf8()..trimmed.len() - quote.len_utf8()].to_string();
        }
    }
    trimmed.to_string()
}

/// One supertype name per entry of a heritage clause.
///
/// A grammar hands the clause back as written — `BaseService`, or
/// `AuthProvider, Disposable`, or `extends Base<T>` — so the keyword, the comma
/// list and the type parameters are peeled off here rather than carried into
/// the graph as if they were part of a name.
pub(super) fn split_supertypes(clause: &str) -> Vec<String> {
    // Type arguments are not supertypes, and they are full of commas, so they
    // are removed before anything is split on one.
    let mut without_arguments = String::with_capacity(clause.len());
    let mut depth = 0_u32;
    for character in clause.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => without_arguments.push(character),
            _ => {}
        }
    }

    let mut names = Vec::new();
    for part in without_arguments.split([',', '{', '}']) {
        let mut current = String::new();
        for token in part.split_whitespace() {
            // The clauses are syntax: `extends Base`, `implements A, B`.
            if matches!(token, "extends" | "implements") {
                push_supertype(&mut names, &mut current);
                continue;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(token);
        }
        push_supertype(&mut names, &mut current);
    }
    names
}

/// Keep one accumulated name if it actually names something.
fn push_supertype(names: &mut Vec<String>, current: &mut String) {
    let name = simplify_type(current.trim());
    current.clear();
    if !name.is_empty() && !name.contains('=') && !name.contains(' ') {
        names.push(name);
    }
}

/// Rust: items, `impl` blocks, `use` declarations and call sites.
mod rust {
    use super::*;

    pub(super) fn extract(root: Node<'_>, content: &str, out: &mut AstFile) {
        let mut test_modules: Vec<usize> = Vec::new();
        let mut stack: Vec<(Node<'_>, Option<String>)> = vec![(root, None)];

        while let Some((node, owner)) = stack.pop() {
            let mut next_owner = owner.clone();
            match node.kind() {
                "impl_item" => {
                    let subject = field_text(node, "type", content)
                        .map(simplify_type)
                        .unwrap_or_default();
                    if let Some(trait_name) = field_text(node, "trait", content) {
                        out.relation(
                            subject.clone(),
                            simplify_type(trait_name),
                            line_of(node.start_position().row),
                        );
                    }
                    next_owner = Some(subject);
                }
                "use_declaration" => {
                    if let Some(argument) = field_text(node, "argument", content) {
                        out.import(
                            argument.split_whitespace().collect::<Vec<_>>().join(" "),
                            line_of(node.start_position().row),
                        );
                    }
                }
                "mod_item" => {
                    if let Some(name) = field_text(node, "name", content) {
                        let name = name.to_string();
                        let index = out.symbol(
                            name.clone(),
                            name.clone(),
                            SymbolKind::Module,
                            node,
                            rust_visible(node),
                        );
                        if has_cfg_test(node, content) {
                            test_modules.push(index);
                        }
                        // `mod name;` declares a file this one depends on;
                        // `mod name { … }` is the file, inline.
                        if node.child_by_field_name("body").is_none() {
                            out.import(format!("mod:{name}"), line_of(node.start_position().row));
                        }
                    }
                }
                "struct_item" | "enum_item" | "trait_item" | "type_item" | "union_item"
                | "function_item" | "function_signature_item" | "const_item" | "static_item" => {
                    record_item(node, content, out, owner.as_deref());
                }
                "call_expression" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        record_call(function, content, out, node);
                    }
                }
                _ => {}
            }
            for child in node
                .children(&mut node.walk())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                stack.push((child, next_owner.clone()));
            }
        }

        mark_test_modules(&mut out.symbols, &test_modules);
    }

    fn record_item(node: Node<'_>, content: &str, out: &mut AstFile, owner: Option<&str>) {
        let Some(name) = field_text(node, "name", content) else {
            return;
        };
        let name = name.to_string();
        let (kind, qualified) = match node.kind() {
            "struct_item" | "union_item" => (SymbolKind::Struct, name.clone()),
            "enum_item" => (SymbolKind::Enum, name.clone()),
            "trait_item" => (SymbolKind::Trait, name.clone()),
            "type_item" => (SymbolKind::Type, name.clone()),
            "const_item" | "static_item" => (SymbolKind::Constant, name.clone()),
            _ => match owner {
                // A function inside an `impl` block is that type's method, and
                // is named so. Anything else is a free function.
                Some(owner) => (SymbolKind::Method, format!("{owner}::{name}")),
                None => (SymbolKind::Function, name.clone()),
            },
        };
        out.symbol(name, qualified, kind, node, rust_visible(node));
    }

    fn record_call(function: Node<'_>, content: &str, out: &mut AstFile, site: Node<'_>) {
        match function.kind() {
            "identifier" => out.call(text(function, content), false, site),
            "field_expression" => {
                if let Some(field) = field_text(function, "field", content) {
                    out.call(field, true, site);
                }
            }
            "scoped_identifier" => {
                if let Some(name) = field_text(function, "name", content) {
                    out.call(name, true, site);
                }
            }
            "generic_function" => {
                if let Some(inner) = function.child_by_field_name("function") {
                    record_call(inner, content, out, site);
                }
            }
            _ => {}
        }
    }

    /// Everything declared inside a `#[cfg(test)]` module is test code,
    /// including the module itself — Rust keeps its tests in the file they
    /// cover, so nothing file-level could have told us (§19, §29).
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
}

/// TypeScript and JavaScript: declarations, heritage clauses, imports, calls.
mod javascript {
    use super::*;

    pub(super) fn extract(root: Node<'_>, content: &str, out: &mut AstFile) {
        let mut stack: Vec<(Node<'_>, Option<String>)> = vec![(root, None)];

        while let Some((node, owner)) = stack.pop() {
            let mut next_owner = owner.clone();
            match node.kind() {
                "class_declaration" | "abstract_class_declaration" | "class" => {
                    if let Some(name) = field_text(node, "name", content) {
                        let name = name.to_string();
                        out.symbol(
                            name.clone(),
                            name.clone(),
                            SymbolKind::Class,
                            node,
                            is_exported(node, content),
                        );
                        record_heritage(node, content, out, &name);
                        next_owner = Some(name);
                    }
                }
                "interface_declaration" => {
                    if let Some(name) = field_text(node, "name", content) {
                        let name = name.to_string();
                        out.symbol(
                            name.clone(),
                            name.clone(),
                            SymbolKind::Interface,
                            node,
                            is_exported(node, content),
                        );
                        record_heritage(node, content, out, &name);
                        next_owner = Some(name);
                    }
                }
                "type_alias_declaration" => {
                    if let Some(name) = field_text(node, "name", content) {
                        out.symbol(
                            name.to_string(),
                            name.to_string(),
                            SymbolKind::Type,
                            node,
                            is_exported(node, content),
                        );
                    }
                }
                "enum_declaration" => {
                    if let Some(name) = field_text(node, "name", content) {
                        out.symbol(
                            name.to_string(),
                            name.to_string(),
                            SymbolKind::Enum,
                            node,
                            is_exported(node, content),
                        );
                    }
                }
                "function_declaration" | "generator_function_declaration" => {
                    record_function(node, content, out, owner.as_deref());
                }
                "method_definition" => {
                    if let Some(name) = field_text(node, "name", content) {
                        if name != "constructor" {
                            let qualified = match owner.as_deref() {
                                Some(owner) => format!("{owner}.{name}"),
                                None => name.to_string(),
                            };
                            out.symbol(
                                name.to_string(),
                                qualified,
                                SymbolKind::Method,
                                node,
                                is_exported(node, content),
                            );
                        }
                    }
                }
                "lexical_declaration" | "variable_declaration" => {
                    record_const_function(node, content, out);
                }
                "import_statement" | "export_statement" => {
                    if let Some(source) = field_text(node, "source", content) {
                        out.import(unquote(source), line_of(node.start_position().row));
                    }
                }
                "call_expression" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        record_call(function, content, out, node);
                    }
                }
                "new_expression" => {
                    if let Some(constructor) = node.child_by_field_name("constructor") {
                        out.call(last_segment(text(constructor, content)), false, node);
                    }
                }
                _ => {}
            }
            for child in node
                .children(&mut node.walk())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                stack.push((child, next_owner.clone()));
            }
        }
    }

    fn record_function(node: Node<'_>, content: &str, out: &mut AstFile, owner: Option<&str>) {
        let Some(name) = field_text(node, "name", content) else {
            return;
        };
        let name = name.to_string();
        let qualified = match owner {
            Some(owner) => format!("{owner}.{name}"),
            None => name.clone(),
        };
        out.symbol(
            name,
            qualified,
            SymbolKind::Function,
            node,
            is_exported(node, content),
        );
    }

    /// `const name = () => …`, which is how most modern JavaScript declares a
    /// function. Only an arrow or function value counts; a plain constant is
    /// not a declaration of something callable.
    fn record_const_function(node: Node<'_>, content: &str, out: &mut AstFile) {
        for declarator in node.children(&mut node.walk()) {
            if declarator.kind() != "variable_declarator" {
                continue;
            }
            let Some(name) = field_text(declarator, "name", content) else {
                continue;
            };
            let Some(value) = declarator.child_by_field_name("value") else {
                continue;
            };
            if !matches!(
                value.kind(),
                "arrow_function" | "function" | "function_expression"
            ) {
                continue;
            }
            out.symbol(
                name.to_string(),
                name.to_string(),
                SymbolKind::Function,
                declarator,
                is_exported(node, content),
            );
        }
    }

    /// `extends X implements A, B` — the supertypes a class or interface
    /// announces, taken from the syntax tree's own clauses.
    ///
    /// One relationship per supertype, named plainly: `extends` and
    /// `implements` are syntax, not part of a name, and a comma list is several
    /// supertypes rather than one strange one.
    fn record_heritage(node: Node<'_>, content: &str, out: &mut AstFile, subject: &str) {
        let line = line_of(node.start_position().row);
        let mut stack = node.children(&mut node.walk()).collect::<Vec<_>>();
        stack.reverse();
        while let Some(child) = stack.pop() {
            match child.kind() {
                "class_heritage" | "extends_clause" | "implements_clause"
                | "extends_type_clause" => {
                    for inner in child.named_children(&mut child.walk()) {
                        if inner.kind().ends_with("comment") {
                            continue;
                        }
                        for name in split_supertypes(text(inner, content)) {
                            out.relation(subject.to_string(), name, line);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn record_call(function: Node<'_>, content: &str, out: &mut AstFile, site: Node<'_>) {
        match function.kind() {
            "identifier" => {
                // `require('./x')` is an import, not a call.
                if text(function, content) == "require" {
                    if let Some(arguments) = site.child_by_field_name("arguments") {
                        for argument in arguments.named_children(&mut arguments.walk()) {
                            if argument.kind() == "string" {
                                out.import(
                                    unquote(text(argument, content)),
                                    line_of(site.start_position().row),
                                );
                            }
                        }
                    }
                    return;
                }
                out.call(text(function, content), false, site);
            }
            "member_expression" => {
                if let Some(property) = field_text(function, "property", content) {
                    out.call(property, true, site);
                }
            }
            _ => {}
        }
    }
}

/// Python: classes, functions, base classes, imports and calls.
mod python {
    use super::*;

    pub(super) fn extract(root: Node<'_>, content: &str, out: &mut AstFile) {
        let mut stack: Vec<(Node<'_>, Option<String>)> = vec![(root, None)];

        while let Some((node, owner)) = stack.pop() {
            let mut next_owner = owner.clone();
            match node.kind() {
                "class_definition" => {
                    if let Some(name) = field_text(node, "name", content) {
                        let name = name.to_string();
                        let exported = !name.starts_with('_');
                        out.symbol(name.clone(), name.clone(), SymbolKind::Class, node, exported);
                        record_bases(node, content, out, &name);
                        next_owner = Some(name);
                    }
                }
                "function_definition" => {
                    if let Some(name) = field_text(node, "name", content) {
                        let name = name.to_string();
                        let (kind, qualified) = match owner.as_deref() {
                            Some(owner) => (SymbolKind::Method, format!("{owner}.{name}")),
                            None => (SymbolKind::Function, name.clone()),
                        };
                        let exported = !name.starts_with('_');
                        out.symbol(name, qualified, kind, node, exported);
                    }
                }
                "import_statement" => {
                    for child in node.named_children(&mut node.walk()) {
                        if matches!(child.kind(), "dotted_name" | "aliased_import") {
                            let specifier = match child.kind() {
                                "aliased_import" => field_text(child, "name", content)
                                    .unwrap_or_default()
                                    .to_string(),
                                _ => text(child, content).to_string(),
                            };
                            if !specifier.is_empty() {
                                out.import(specifier, line_of(node.start_position().row));
                            }
                        }
                    }
                }
                "import_from_statement" => {
                    if let Some(module) = field_text(node, "module_name", content) {
                        out.import(
                            module.split_whitespace().collect::<Vec<_>>().join(""),
                            line_of(node.start_position().row),
                        );
                    }
                }
                "call" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        record_call(function, content, out, node);
                    }
                }
                _ => {}
            }
            for child in node
                .children(&mut node.walk())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                stack.push((child, next_owner.clone()));
            }
        }
    }

    /// Base classes: everything in the superclass list except `object`, a
    /// keyword argument such as `metaclass=`, and unpacking.
    fn record_bases(node: Node<'_>, content: &str, out: &mut AstFile, subject: &str) {
        let Some(bases) = node.child_by_field_name("superclasses") else {
            return;
        };
        let line = line_of(node.start_position().row);
        for base in bases.named_children(&mut bases.walk()) {
            if matches!(
                base.kind(),
                "keyword_argument" | "dictionary_splat" | "list_splat"
            ) {
                continue;
            }
            let name = simplify_type(text(base, content));
            if name.is_empty() || name == "object" {
                continue;
            }
            out.relation(subject.to_string(), name, line);
        }
    }

    fn record_call(function: Node<'_>, content: &str, out: &mut AstFile, site: Node<'_>) {
        match function.kind() {
            "identifier" => out.call(text(function, content), false, site),
            "attribute" => {
                if let Some(attribute) = field_text(function, "attribute", content) {
                    out.call(attribute, true, site);
                }
            }
            _ => {}
        }
    }
}

/// Go: functions, methods, types, imports and calls.
mod go {
    use super::*;

    pub(super) fn extract(root: Node<'_>, content: &str, out: &mut AstFile) {
        let mut stack: Vec<Node<'_>> = vec![root];

        while let Some(node) = stack.pop() {
            match node.kind() {
                "function_declaration" => record(node, content, out, SymbolKind::Function),
                "method_declaration" => record(node, content, out, SymbolKind::Method),
                "type_spec" => record(node, content, out, SymbolKind::Type),
                "const_spec" | "var_spec" => record(node, content, out, SymbolKind::Constant),
                "import_spec" => {
                    if let Some(path) = field_text(node, "path", content) {
                        out.import(unquote(path), line_of(node.start_position().row));
                    }
                }
                "call_expression" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        match function.kind() {
                            "identifier" => out.call(text(function, content), false, node),
                            "selector_expression" => {
                                if let Some(field) = field_text(function, "field", content) {
                                    out.call(field, true, node);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            for child in node
                .children(&mut node.walk())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                stack.push(child);
            }
        }
    }

    fn record(node: Node<'_>, content: &str, out: &mut AstFile, kind: SymbolKind) {
        let Some(name) = field_text(node, "name", content) else {
            return;
        };
        let name = name.to_string();
        // Go's visibility is the first letter's case, and nothing else.
        let exported = name.chars().next().is_some_and(char::is_uppercase);
        out.symbol(name.clone(), name, kind, node, exported);
    }
}
