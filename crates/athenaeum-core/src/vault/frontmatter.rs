//! The YAML frontmatter subset the vault actually contains.
//!
//! This is not a YAML parser and must not grow into one. The vault's schema
//! (`schema.md`) specifies a flat block of scalars and string lists, and that
//! is exactly what is supported here:
//!
//! ```yaml
//! ---
//! type: concept
//! title: Chain of Thought
//! tags: [reasoning, prompting]
//! related:
//!   - "[[in-context-learning]]"
//! sources: ["wei-2022-cot.pdf"]
//! ---
//! ```
//!
//! Pulling in a full YAML implementation would buy nested documents and
//! anchors that no page uses, at the cost of a dependency the knowledge
//! library would then carry on every platform. The one concession to
//! reality is that pages are written by a model as often as by a person, so
//! two malformations models actually emit are recovered rather than
//! refused: a leading ```` ```yaml ```` fence, and an unbracketed list of
//! wikilinks (`related: [[a]], [[b]]`), which is not valid YAML at all.
//!
//! Anything this parser cannot make sense of is reported as *no
//! frontmatter*, never as an error. A page that predates the convention is
//! still a page, and refusing to index it would lose knowledge that is
//! sitting right there in the body.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One frontmatter entry. Scalars and lists stay distinct because
/// `sources: x.pdf` and `sources: [x.pdf]` mean the same thing to a reader
/// but not to a naive consumer, and the readers here ask for whichever
/// shape they need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrontmatterValue {
    Scalar(String),
    List(Vec<String>),
}

/// A parsed frontmatter block, key order normalized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Frontmatter {
    values: BTreeMap<String, FrontmatterValue>,
}

impl Frontmatter {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// A single value. A list yields its first element, so `type: [concept]`
    /// still reads as `concept` rather than silently becoming untyped.
    pub fn scalar(&self, key: &str) -> Option<&str> {
        match self.values.get(key)? {
            FrontmatterValue::Scalar(value) => Some(value.as_str()),
            FrontmatterValue::List(items) => items.first().map(String::as_str),
        }
    }

    /// A trimmed, non-empty scalar. Frontmatter written by hand is full of
    /// `title:` keys with nothing after them, and an empty string is not a
    /// title.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.scalar(key).map(str::trim).filter(|s| !s.is_empty())
    }

    /// Every value under `key`. A scalar reads as a one-element list, so
    /// `sources: a.pdf` and `sources: [a.pdf]` behave identically.
    pub fn list(&self, key: &str) -> Vec<&str> {
        match self.values.get(key) {
            Some(FrontmatterValue::List(items)) => items
                .iter()
                .map(|item| item.trim())
                .filter(|item| !item.is_empty())
                .collect(),
            Some(FrontmatterValue::Scalar(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    Vec::new()
                } else {
                    vec![trimmed]
                }
            }
            None => Vec::new(),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }
}

/// A page split into its frontmatter and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split<'a> {
    /// `None` when the page carries no recoverable frontmatter block.
    pub frontmatter: Option<Frontmatter>,
    /// Everything after the closing fence — the whole file when there was
    /// no frontmatter.
    pub body: &'a str,
}

/// Maximum lines of junk tolerated before the opening fence. Beyond this a
/// `---` is a horizontal rule in the body, not a frontmatter opener.
const MAX_PREFIX_LINES: usize = 6;

/// Split a page into frontmatter and body.
pub fn split(content: &str) -> Split<'_> {
    let Some((payload, body)) = locate(content) else {
        return Split {
            frontmatter: None,
            body: content,
        };
    };
    Split {
        frontmatter: Some(parse_block(payload)),
        body,
    }
}

/// Find the frontmatter payload and the body that follows it.
///
/// Preference order is strict-at-the-top first, then the first `---…---`
/// pair whose opening fence is still near the top of the file.
fn locate(content: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    let mut prefix_lines = 0;

    loop {
        let rest = &content[offset..];
        let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        let line = rest[..line_end].trim_end_matches(['\r', '\n']);

        if line.trim() == "---" {
            let after_open = offset + line_end;
            return close_at(content, after_open);
        }

        // Only blank lines and a code-fence opener may precede the block;
        // any other content means this file simply has no frontmatter.
        let trimmed = line.trim();
        let tolerable = trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("```yaml")
            || trimmed.eq_ignore_ascii_case("```yml")
            || trimmed == "```"
            || trimmed == "frontmatter:";
        if !tolerable || line_end == 0 {
            return None;
        }

        prefix_lines += 1;
        if prefix_lines > MAX_PREFIX_LINES {
            return None;
        }
        offset += line_end;
        if offset >= content.len() {
            return None;
        }
    }
}

/// Scan from `start` for the closing fence, returning the payload between
/// the fences and the body after them.
fn close_at(content: &str, start: usize) -> Option<(&str, &str)> {
    let mut cursor = start;
    while cursor < content.len() {
        let rest = &content[cursor..];
        let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        let line = rest[..line_end].trim_end_matches(['\r', '\n']);
        if line.trim() == "---" {
            let payload = content[start..cursor].trim_end_matches(['\r', '\n']);
            let mut body = &content[cursor + line_end..];
            // A page whose frontmatter was wrapped in a code fence leaves a
            // stray closing fence at the head of the body. Left in place it
            // swallows the entire page into one never-closed code block.
            let body_head = body.trim_start_matches(['\r', '\n', ' ', '\t']);
            if let Some(after) = body_head.strip_prefix("```") {
                if after
                    .trim_start_matches([' ', '\t'])
                    .starts_with(['\r', '\n'])
                    || after.trim().is_empty()
                {
                    body = after.trim_start_matches(['\r', '\n', ' ', '\t']);
                }
            }
            return Some((payload, body));
        }
        cursor += line_end;
        if line_end == 0 {
            break;
        }
    }
    None
}

fn parse_block(payload: &str) -> Frontmatter {
    let mut values = BTreeMap::new();
    let lines: Vec<&str> = payload.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        i += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Only top-level keys are read; a nested map's children are skipped
        // by the continuation scan below rather than mistaken for keys.
        if line.starts_with([' ', '\t']) || trimmed.starts_with('-') {
            continue;
        }

        let Some((key, rest)) = split_key(trimmed) else {
            continue;
        };
        let key = key.to_string();
        let rest = rest.trim();

        // Block scalar: `key: |` or `key: >` followed by an indented body.
        if rest == "|" || rest == ">" || rest == "|-" || rest == ">-" {
            let mut collected = Vec::new();
            while i < lines.len()
                && (lines[i].trim().is_empty() || lines[i].starts_with([' ', '\t']))
            {
                collected.push(lines[i].trim());
                i += 1;
            }
            let joined = if rest.starts_with('>') {
                collected.join(" ")
            } else {
                collected.join("\n")
            };
            values.insert(key, FrontmatterValue::Scalar(joined.trim().to_string()));
            continue;
        }

        if !rest.is_empty() {
            values.insert(key, parse_inline(rest));
            continue;
        }

        // Bare `key:` — either a block list underneath, or an empty value.
        let mut items = Vec::new();
        while i < lines.len() {
            let candidate = lines[i];
            let candidate_trimmed = candidate.trim();
            if candidate_trimmed.is_empty() {
                i += 1;
                continue;
            }
            if !candidate.starts_with([' ', '\t']) {
                break;
            }
            match candidate_trimmed.strip_prefix('-') {
                Some(item) => {
                    items.push(unquote(item.trim()));
                    i += 1;
                }
                // An indented non-list line is a nested map; skip its body
                // rather than reading its keys as top-level ones.
                None => i += 1,
            }
        }
        values.insert(
            key,
            if items.is_empty() {
                FrontmatterValue::Scalar(String::new())
            } else {
                FrontmatterValue::List(items)
            },
        );
    }

    Frontmatter { values }
}

/// Split `key: value` at the first colon that is followed by whitespace or
/// end of line, so `title: Source: notes.pdf` keeps its inner colon.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let mut search = 0;
    while let Some(offset) = line[search..].find(':') {
        let index = search + offset;
        let after = &line[index + 1..];
        if after.is_empty() || after.starts_with([' ', '\t']) {
            let key = line[..index].trim();
            if key.is_empty() || key.contains(' ') {
                return None;
            }
            return Some((key, after));
        }
        search = index + 1;
    }
    None
}

fn parse_inline(raw: &str) -> FrontmatterValue {
    // An unbracketed wikilink list is tried first: it also opens with `[`,
    // so a flow-sequence reading would strip one bracket off each end and
    // leave two half-links behind.
    if raw.starts_with("[[") {
        let links = split_wikilink_flow(raw);
        if !links.is_empty() {
            return FrontmatterValue::List(links);
        }
    }
    if let Some(inner) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return FrontmatterValue::List(split_flow(inner));
    }
    FrontmatterValue::Scalar(unquote(raw))
}

/// Split a flow sequence body on commas that sit outside quotes.
fn split_flow(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in inner.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => {
                quote = None;
                current.push(c);
            }
            (Some(_), c) => current.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(ch);
                current.push(ch);
            }
            (None, ',') => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            (None, c) => current.push(c),
        }
    }
    items.push(current);

    items
        .into_iter()
        .map(|item| unquote(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

/// `[[a]], [[b]]` and `[[a]] [[b]]` — a shape models emit often enough that
/// refusing it would drop real relationships on the floor.
fn split_wikilink_flow(raw: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let target = after[..close].trim();
        // A bracket inside the target means this was never a wikilink list
        // — a nested flow sequence, most likely.
        if target.contains('[') || target.contains(']') {
            return Vec::new();
        }
        if !target.is_empty() {
            links.push(format!("[[{target}]]"));
        }
        rest = &after[close + 2..];
    }
    // Only treat this as a list when nothing but links and separators is
    // left over, so a sentence mentioning `[[x]]` stays a scalar.
    if rest.trim_matches([' ', ',', '\t']).is_empty() {
        links
    } else {
        Vec::new()
    }
}

fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    for quote in ['"', '\''] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            let inner = &trimmed[1..trimmed.len() - 1];
            return if quote == '"' {
                inner
                    .replace("\\\"", "\"")
                    .replace("\\n", "\n")
                    .replace("\\\\", "\\")
            } else {
                inner.replace("''", "'")
            };
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_schema_documented_shape() {
        let split = split(
            "---\ntype: concept\ntitle: Chain of Thought\ntags: [reasoning, prompting]\nrelated:\n  - \"[[in-context-learning]]\"\n  - \"[[scaling]]\"\nsources: [\"wei-2022-cot.pdf\"]\n---\n\n# Chain of Thought\n\nBody.\n",
        );
        let fm = split.frontmatter.expect("frontmatter");
        assert_eq!(fm.text("type"), Some("concept"));
        assert_eq!(fm.text("title"), Some("Chain of Thought"));
        assert_eq!(fm.list("tags"), vec!["reasoning", "prompting"]);
        assert_eq!(
            fm.list("related"),
            vec!["[[in-context-learning]]", "[[scaling]]"]
        );
        assert_eq!(fm.list("sources"), vec!["wei-2022-cot.pdf"]);
        assert!(split.body.starts_with("\n# Chain of Thought"));
    }

    #[test]
    fn a_page_without_frontmatter_keeps_its_whole_body() {
        let split = split("# Just a heading\n\nNo YAML here.\n");
        assert!(split.frontmatter.is_none());
        assert_eq!(split.body, "# Just a heading\n\nNo YAML here.\n");
    }

    #[test]
    fn a_horizontal_rule_deep_in_the_body_is_not_frontmatter() {
        let content = "# Title\n\nParagraph one.\n\n---\n\nParagraph two.\n";
        assert!(split(content).frontmatter.is_none());
    }

    #[test]
    fn a_title_may_contain_a_colon() {
        let fm = split("---\ntitle: \"Source: notes.pdf\"\ntype: source\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.text("title"), Some("Source: notes.pdf"));
        assert_eq!(fm.text("type"), Some("source"));
    }

    #[test]
    fn an_unbracketed_wikilink_list_is_recovered() {
        // Not valid YAML, and emitted by models often enough that refusing
        // it would silently drop real relationships.
        let fm = split("---\nrelated: [[alpha]], [[beta]]\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.list("related"), vec!["[[alpha]]", "[[beta]]"]);
    }

    #[test]
    fn a_sentence_mentioning_a_wikilink_stays_a_scalar() {
        let fm = split("---\nnote: [[alpha]] is the important one\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.text("note"), Some("[[alpha]] is the important one"));
    }

    #[test]
    fn a_code_fenced_block_is_recovered_and_its_closing_fence_removed() {
        let split = split("```yaml\n---\ntype: entity\n---\n```\n# Heading\n");
        assert_eq!(split.frontmatter.unwrap().text("type"), Some("entity"));
        assert_eq!(split.body, "# Heading\n");
    }

    #[test]
    fn empty_and_missing_values_read_as_absent() {
        let fm = split("---\ntitle:\ntags: []\nrelated:\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.text("title"), None);
        assert!(fm.list("tags").is_empty());
        assert!(fm.list("related").is_empty());
        assert!(fm.contains_key("title"));
        assert_eq!(fm.text("nothing"), None);
    }

    #[test]
    fn a_scalar_reads_as_a_one_element_list() {
        let fm = split("---\nsources: only.pdf\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.list("sources"), vec!["only.pdf"]);
    }

    #[test]
    fn nested_maps_are_skipped_rather_than_flattened() {
        let fm = split(
            "---\ntype: source\nmeta:\n  author: Wei\n  year: 2022\ntitle: Paper\n---\nbody\n",
        )
        .frontmatter
        .unwrap();
        assert_eq!(fm.text("type"), Some("source"));
        assert_eq!(fm.text("title"), Some("Paper"));
        // The nested keys must not leak into the top level.
        assert_eq!(fm.text("author"), None);
        assert_eq!(fm.text("year"), None);
    }

    #[test]
    fn block_scalars_join_their_lines() {
        let fm = split("---\nsummary: |\n  first line\n  second line\ntype: concept\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.text("summary"), Some("first line\nsecond line"));
        assert_eq!(fm.text("type"), Some("concept"));
    }

    #[test]
    fn quoted_commas_do_not_split_a_flow_list() {
        let fm = split("---\nauthors: [\"Wei, Jason\", \"Zhou, Denny\"]\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.list("authors"), vec!["Wei, Jason", "Zhou, Denny"]);
    }

    #[test]
    fn windows_line_endings_parse_the_same_way() {
        let fm = split("---\r\ntype: entity\r\ntitle: Openai\r\n---\r\nbody\r\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.text("type"), Some("entity"));
        assert_eq!(fm.text("title"), Some("Openai"));
    }

    #[test]
    fn an_unclosed_block_is_not_frontmatter() {
        let split = split("---\ntype: concept\n\n# Heading without a closing fence\n");
        assert!(split.frontmatter.is_none());
    }

    #[test]
    fn keys_are_reported_for_a_frontmatter_panel() {
        let fm = split("---\ntype: concept\ntitle: X\n---\nbody\n")
            .frontmatter
            .unwrap();
        assert_eq!(fm.keys().collect::<Vec<_>>(), vec!["title", "type"]);
        assert!(!fm.is_empty());
    }
}
