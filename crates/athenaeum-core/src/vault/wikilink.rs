//! Extracting `[[wikilink]]` targets from a page body.
//!
//! A wikilink is the vault's only relationship syntax, so this is where the
//! deterministic half of the knowledge graph comes from. No model is
//! involved in this pass and none ever should be: an edge derived here is
//! read straight out of the markdown, which is what lets it be stored as
//! [`crate::model::ProvenanceSource::Deterministic`] rather than as a
//! proposal awaiting review.
//!
//! Two things are deliberately *not* links:
//!
//! * `![[embed]]` — Obsidian's embed syntax, which points at an image or an
//!   attachment rather than asserting a relationship between pages.
//! * anything inside a fenced code block or an inline code span, so a page
//!   documenting the syntax does not sprout edges to pages named `target`.

/// One `[[target|alias]]` occurrence, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// The link target with any `#heading` and `|alias` removed, trimmed.
    pub target: String,
    /// The heading fragment, when the link pointed into a section.
    pub heading: Option<String>,
    /// The display text, when the author gave one.
    pub alias: Option<String>,
}

/// Every wikilink in `body`, in document order, including repeats.
pub fn extract(body: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    for span in prose_spans(body) {
        collect(span, &mut links);
    }
    links
}

/// The distinct link targets in `body`, first occurrence order preserved.
///
/// Order matters: it is what makes the set of edges written for a page
/// stable across runs, and a stable write order is what keeps re-indexing
/// an unchanged vault a genuine no-op.
pub fn targets(body: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();
    for link in extract(body) {
        if seen.insert(link.target.clone()) {
            ordered.push(link.target);
        }
    }
    ordered
}

fn collect(text: &str, links: &mut Vec<WikiLink>) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find("[[") {
        let open = cursor + offset;
        // `![[…]]` is an embed, not a relationship.
        let embed = open > 0 && bytes[open - 1] == b'!';
        let after = &text[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let inner = &after[..close];
        cursor = open + 2 + close + 2;

        if embed || inner.contains('[') || inner.contains('\n') {
            continue;
        }
        if let Some(link) = parse_inner(inner) {
            links.push(link);
        }
    }
}

fn parse_inner(inner: &str) -> Option<WikiLink> {
    let (before_alias, alias) = match inner.split_once('|') {
        Some((target, alias)) => (target, Some(alias.trim().to_string())),
        None => (inner, None),
    };
    let (target, heading) = match before_alias.split_once('#') {
        Some((target, heading)) => (target, Some(heading.trim().to_string())),
        None => (before_alias, None),
    };

    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    Some(WikiLink {
        target: target.to_string(),
        heading: heading.filter(|h| !h.is_empty()),
        alias: alias.filter(|a| !a.is_empty()),
    })
}

/// Split a body into the spans that are prose rather than code.
///
/// Handles fenced blocks (``` and ~~~, with any info string) and inline
/// code spans of any backtick run length, which is the same rule
/// CommonMark uses.
fn prose_spans(body: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut in_fence: Option<(char, usize)> = None;
    let mut pending_start: Option<usize> = None;
    let mut offset = 0;

    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        // A fence indented four or more spaces is an indented code block's
        // content, not a fence.
        let fence = if indent < 4 {
            fence_marker(trimmed)
        } else {
            None
        };

        match (in_fence, fence) {
            (None, Some((ch, len))) => {
                if let Some(open) = pending_start.take() {
                    spans.push(&body[open..start]);
                }
                in_fence = Some((ch, len));
            }
            (Some((open_ch, open_len)), Some((ch, len)))
                if ch == open_ch && len >= open_len && trimmed.trim_end().len() == len =>
            {
                in_fence = None;
            }
            (Some(_), _) => {}
            (None, None) => {
                if pending_start.is_none() {
                    pending_start = Some(start);
                }
            }
        }
    }
    if let Some(open) = pending_start {
        spans.push(&body[open..]);
    }

    spans
        .into_iter()
        .flat_map(strip_inline_code)
        .filter(|span| !span.is_empty())
        .collect()
}

fn fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|c| *c == first).count();
    if len < 3 {
        return None;
    }
    // An info string may not contain a backtick, which is what stops
    // ``` `a` and `b` ``` from being read as a fence opener.
    if first == '`' && trimmed[len..].contains('`') {
        return None;
    }
    Some((first, len))
}

/// Drop inline code spans, returning the prose between them.
fn strip_inline_code(span: &str) -> Vec<&str> {
    if !span.contains('`') {
        return vec![span];
    }
    let bytes = span.as_bytes();
    let mut out = Vec::new();
    let mut prose_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let after = i + run;
        // A closing run of exactly the same length ends the span; an
        // unmatched opener is literal text and is left alone.
        match find_run(&bytes[after..], run) {
            Some(rel) => {
                out.push(&span[prose_start..i]);
                i = after + rel + run;
                prose_start = i;
            }
            None => i = after,
        }
    }
    out.push(&span[prose_start..]);
    out
}

/// Offset of the next backtick run of exactly `len`.
fn find_run(bytes: &[u8], len: usize) -> Option<usize> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        if run == len {
            return Some(i);
        }
        i += run;
    }
    None
}
