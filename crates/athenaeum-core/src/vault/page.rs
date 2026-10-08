//! Reading one markdown page into the values the indexer needs.
//!
//! Everything here is a pure function of a path and a string, so the whole
//! of the vault's interpretation — what a page is called, what type it
//! claims to be, what it links to, what it rests on — is testable without
//! touching a filesystem.

use crate::model::{hash, Authority};

use super::frontmatter::{self, Frontmatter};
use super::wikilink;

/// Longest summary this will produce, in characters. Long enough to be a
/// useful one-line answer, short enough that a hundred of them still fit in
/// a retrieval budget (§28: summaries at L1, full content only on request).
const SUMMARY_LIMIT: usize = 240;

/// A wiki page, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultPage {
    /// Vault-relative path with forward slashes, e.g. `wiki/concepts/x.md`.
    /// This is the page's identity unless it declares one: the file is the
    /// source of truth, so the index keys off where the file is, not what it
    /// currently calls itself.
    pub path: String,
    /// An `id:` the page declared, when it is a valid one (see
    /// [`is_valid_declared_id`]). A declared id outlives moving the file;
    /// whether it is *honoured* is the indexer's call, because that depends
    /// on every other page and on what the store already holds.
    pub declared_id: Option<String>,
    /// The file stem, which is what `[[wikilinks]]` normally name.
    pub slug: String,
    pub title: String,
    /// The vault's own vocabulary (`concept`, `entity`, `source`, …) rather
    /// than the knowledge model's. Translated in
    /// [`super::index`], where the mapping can be read in one place.
    pub page_type: String,
    /// True when `type:` was declared rather than inferred from the path.
    pub type_declared: bool,
    /// An `authority:` the page declared, when it declared a valid one.
    pub authority: Option<Authority>,
    /// An `origin:` the page declared — `deep-research`, `web-clip` — for
    /// the parts of the UI that mark where a page came from.
    pub origin: Option<String>,
    pub tags: Vec<String>,
    pub summary: String,
    /// The body, frontmatter removed.
    pub body: String,
    /// SHA-256 of the whole file as read.
    pub content_hash: String,
    /// Distinct `[[wikilink]]` targets in the body, first-seen order.
    pub links: Vec<String>,
    /// Distinct `related:` targets from the frontmatter.
    pub related: Vec<String>,
    /// Distinct `sources:` identities from the frontmatter, each relative to
    /// `raw/sources/`.
    pub sources: Vec<String>,
}

impl VaultPage {
    /// Parse `content` as the page at vault-relative `path`.
    pub fn parse(path: &str, content: &str) -> Self {
        let split = frontmatter::split(content);
        let fm = split.frontmatter.unwrap_or_default();
        let body = split.body;
        let slug = slug_of(path);

        let (page_type, type_declared) = match fm.text("type").map(normalize_type) {
            Some(declared) if !declared.is_empty() => (declared, true),
            _ => (type_from_path(path), false),
        };

        VaultPage {
            title: title_of(&fm, body, &slug),
            summary: summary_of(&fm, body),
            declared_id: fm
                .text("id")
                .filter(|raw| is_valid_declared_id(raw))
                .map(str::to_string),
            authority: fm.text("authority").and_then(|raw| raw.trim().parse().ok()),
            origin: fm
                .text("origin")
                .map(normalize_marker)
                .filter(|o| !o.is_empty()),
            tags: dedup(fm.list("tags").into_iter().map(str::to_string)),
            links: wikilink::targets(body),
            related: dedup(
                fm.list("related")
                    .into_iter()
                    .filter_map(link_target)
                    .collect::<Vec<_>>(),
            ),
            sources: dedup(
                fm.list("sources")
                    .into_iter()
                    .map(source_identity)
                    .filter(|value| !value.is_empty()),
            ),
            content_hash: hash(content),
            body: body.to_string(),
            page_type,
            type_declared,
            slug,
            path: path.to_string(),
        }
    }
}

/// Whether `id` may be declared in a page's frontmatter.
///
/// `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`, and never starting with `vault_`,
/// which is the prefix of ids the indexer derives from paths. Keeping the
/// two spaces apart is what guarantees a declared id cannot collide with a
/// derived one.
pub fn is_valid_declared_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    id.len() <= 128
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !id.starts_with("vault_")
}

/// The file stem of a vault-relative path.
pub fn slug_of(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .to_string()
}

/// The type a page's location implies, for pages written before `type:` was
/// a convention — or by a person who simply did not add it.
///
/// `wiki/concepts/x.md` is a concept because of where it sits. That is the
/// whole point of the schema's directory table, and it means a vault that
/// predates the frontmatter convention still indexes correctly instead of
/// arriving as a pile of untyped observations.
fn type_from_path(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if slug_of(&lower) == "overview" {
        return "overview".to_string();
    }
    match lower.strip_prefix("wiki/") {
        Some(rest) => match rest.split_once('/') {
            // `wiki/entities/openai.md` → `entity`. Directories are plural
            // by convention and types are singular.
            Some((dir, _)) if !dir.is_empty() => singular(dir),
            // Directly inside `wiki/` with no subdirectory: the location
            // says nothing, so nothing is claimed.
            _ => String::new(),
        },
        // A governance document at the vault root — `purpose.md`,
        // `schema.md` — where the filename is the type.
        None => slug_of(&lower),
    }
}

/// `entities` → `entity`, `concepts` → `concept`.
///
/// Only the two English plural forms the schema's directory table actually
/// uses. Words that are already singular — and the ones whose plural looks
/// like a singular ending in `s` — are left alone.
fn singular(dir: &str) -> String {
    // Directory names the schema already writes in the singular.
    if matches!(dir, "synthesis" | "methodology" | "thesis" | "research") {
        return dir.to_string();
    }
    if let Some(stem) = dir.strip_suffix("ies") {
        return format!("{stem}y");
    }
    match dir.strip_suffix('s') {
        Some(stem) if !stem.is_empty() && !stem.ends_with('s') => stem.to_string(),
        _ => dir.to_string(),
    }
}

fn normalize_type(raw: &str) -> String {
    normalize_marker(raw)
}

/// Fold a short frontmatter marker to the lowercase, punctuation-free form
/// the rest of the pipeline matches on. Anything that is not a letter,
/// digit or hyphen is dropped, so a marker can be stored inside a
/// colon-separated provenance detail without ambiguity.
fn normalize_marker(raw: &str) -> String {
    raw.trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn title_of(fm: &Frontmatter, body: &str, slug: &str) -> String {
    if let Some(title) = fm.text("title") {
        return title.to_string();
    }
    if let Some(heading) = first_heading(body) {
        return heading;
    }
    // `chain-of-thought` → `chain of thought`. Not title-cased: guessing at
    // capitalization produces worse titles than the author's own words.
    slug.replace(['-', '_'], " ").trim().to_string()
}

fn first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let heading = rest.trim().trim_end_matches('#').trim();
            if !heading.is_empty() {
                return Some(plain_text(heading));
            }
        }
    }
    None
}

/// A page's one-or-two-sentence summary: a declared `summary:`, else the
/// first real paragraph of the body.
fn summary_of(fm: &Frontmatter, body: &str) -> String {
    if let Some(declared) = fm.text("summary").or_else(|| fm.text("description")) {
        return truncate(&plain_text(declared));
    }
    truncate(&first_paragraph(body))
}

fn first_paragraph(body: &str) -> String {
    let mut collected: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut in_comment = false;

    for line in body.lines() {
        let trimmed = line.trim();

        if in_comment {
            if let Some(rest) = trimmed.split_once("-->") {
                in_comment = false;
                if !rest.1.trim().is_empty() {
                    collected.push(plain_text(rest.1.trim()));
                }
            }
            continue;
        }
        if trimmed.starts_with("<!--") {
            in_comment = !trimmed.contains("-->");
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            if !collected.is_empty() {
                break;
            }
            continue;
        }
        // Headings, tables, images and horizontal rules are structure, not
        // prose; skip past them to the first sentence a reader would read.
        if trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || trimmed.starts_with("---")
            || trimmed.starts_with("![")
        {
            continue;
        }

        let text = plain_text(strip_leading_marker(trimmed));
        if !text.is_empty() {
            collected.push(text);
        }
    }

    collected.join(" ")
}

/// Strip a list bullet, blockquote marker or numbered-list prefix.
fn strip_leading_marker(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("> ") {
        return rest.trim_start();
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return rest.trim_start();
        }
    }
    line
}

/// Reduce markdown to the words a reader would say out loud: wikilinks and
/// links become their display text, emphasis and inline code lose their
/// punctuation.
fn plain_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(open) = rest.find("[[") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            rest = &rest[open..];
            break;
        };
        let inner = &after[..close];
        let shown = inner
            .split_once('|')
            .map(|(_, alias)| alias)
            .unwrap_or(inner)
            .split('#')
            .next()
            .unwrap_or(inner);
        out.push_str(shown.trim());
        rest = &after[close + 2..];
    }
    out.push_str(rest);

    let out = out.replace("**", "").replace('`', "");
    out.trim_start_matches('!').trim().to_string()
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= SUMMARY_LIMIT {
        return text.to_string();
    }
    let cut: String = text.chars().take(SUMMARY_LIMIT).collect();
    let boundary = cut.rfind(' ').unwrap_or(cut.len());
    format!(
        "{}…",
        cut[..boundary].trim_end_matches([',', ';', ':', ' '])
    )
}

/// `[[target|alias]]` in a frontmatter list → `target`. Bare strings are
/// accepted too: `related: [chain-of-thought]` is common and unambiguous.
fn link_target(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(['"', '\'']).trim();
    let inner = trimmed
        .strip_prefix("[[")
        .and_then(|rest| rest.strip_suffix("]]"))
        .unwrap_or(trimmed);
    let target = inner
        .split('|')
        .next()
        .unwrap_or(inner)
        .split('#')
        .next()
        .unwrap_or(inner)
        .trim();
    (!target.is_empty()).then(|| target.to_string())
}

/// Normalize a `sources:` entry to an identity relative to `raw/sources/`.
///
/// Pages in the wild write this three ways — bare (`paper.pdf`), vault
/// relative (`raw/sources/paper.pdf`), and occasionally absolute — and all
/// three name the same file.
fn source_identity(raw: &str) -> String {
    let normalized = raw.trim().trim_matches(['"', '\'']).replace('\\', "/");
    let trimmed = normalized.trim();
    const MARKER: &str = "raw/sources/";
    let identity = match trimmed.to_ascii_lowercase().find(MARKER) {
        Some(index) => &trimmed[index + MARKER.len()..],
        None => trimmed,
    };
    identity.trim_start_matches('/').trim().to_string()
}

fn dedup(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_that_looks_like_frontmatter_is_not_frontmatter() {
        // A page with no `title:` in its frontmatter, whose body happens to
        // contain a line beginning `title:`. Reading that as the page's
        // title made the index believe a page existed under a name nobody
        // gave it, and review items for genuinely missing pages resolved
        // themselves against it.
        let page = VaultPage::parse(
            "wiki/entities/other.md",
            "---
type: entity
---

# Real Heading

Some text.
title: Attention Mechanism
",
        );

        assert_eq!(page.title, "Real Heading");
    }

    #[test]
    fn reads_a_conventional_page() {
        let page = VaultPage::parse(
            "wiki/concepts/chain-of-thought.md",
            "---\ntype: concept\ntitle: Chain of Thought\ntags: [reasoning]\nrelated: [\"[[scaling]]\"]\nsources: [\"wei-2022.pdf\"]\n---\n\n# Chain of Thought\n\nPrompting a model to show its working improves multi-step reasoning.\n\nSee also [[in-context-learning]].\n",
        );
        assert_eq!(page.slug, "chain-of-thought");
        assert_eq!(page.title, "Chain of Thought");
        assert_eq!(page.page_type, "concept");
        assert!(page.type_declared);
        assert_eq!(page.tags, vec!["reasoning"]);
        assert_eq!(page.related, vec!["scaling"]);
        assert_eq!(page.sources, vec!["wei-2022.pdf"]);
        assert_eq!(page.links, vec!["in-context-learning"]);
        assert_eq!(
            page.summary,
            "Prompting a model to show its working improves multi-step reasoning."
        );
    }

    #[test]
    fn a_page_that_predates_the_convention_still_gets_a_type_and_a_title() {
        // No frontmatter at all: the directory names the type and the
        // heading names the page. Refusing this page would lose knowledge
        // that is sitting right there on disk.
        let page = VaultPage::parse(
            "wiki/entities/openai.md",
            "# OpenAI\n\nAn AI research company.\n",
        );
        assert_eq!(page.page_type, "entity");
        assert!(!page.type_declared);
        assert_eq!(page.title, "OpenAI");
        assert_eq!(page.summary, "An AI research company.");
        assert_eq!(page.authority, None);
    }

    #[test]
    fn a_page_with_neither_frontmatter_nor_heading_falls_back_to_its_filename() {
        let page = VaultPage::parse("wiki/notes/some-loose_note.md", "Just a sentence.\n");
        assert_eq!(page.title, "some loose note");
        assert_eq!(page.page_type, "note");
    }

    #[test]
    fn directory_types_are_singularized_except_where_the_schema_is_not() {
        assert_eq!(
            VaultPage::parse("wiki/entities/a.md", "").page_type,
            "entity"
        );
        assert_eq!(
            VaultPage::parse("wiki/concepts/a.md", "").page_type,
            "concept"
        );
        assert_eq!(VaultPage::parse("wiki/queries/a.md", "").page_type, "query");
        assert_eq!(
            VaultPage::parse("wiki/findings/a.md", "").page_type,
            "finding"
        );
        // These directory names are already singular in the schema.
        assert_eq!(
            VaultPage::parse("wiki/synthesis/a.md", "").page_type,
            "synthesis"
        );
        assert_eq!(
            VaultPage::parse("wiki/methodology/a.md", "").page_type,
            "methodology"
        );
        assert_eq!(VaultPage::parse("wiki/thesis/a.md", "").page_type, "thesis");
    }

    #[test]
    fn overview_is_recognized_wherever_it_sits() {
        assert_eq!(
            VaultPage::parse("wiki/overview.md", "").page_type,
            "overview"
        );
    }

    #[test]
    fn a_declared_type_beats_the_directory() {
        let page = VaultPage::parse(
            "wiki/concepts/gpt-4.md",
            "---\ntype: entity\n---\n# GPT-4\n",
        );
        assert_eq!(page.page_type, "entity");
        assert!(page.type_declared);
    }

    #[test]
    fn a_declared_authority_is_carried_and_a_nonsense_one_is_ignored() {
        let claimed = VaultPage::parse(
            "wiki/concepts/a.md",
            "---\nauthority: project_decision\n---\nBody.\n",
        );
        assert_eq!(claimed.authority, Some(Authority::ProjectDecision));

        let nonsense = VaultPage::parse(
            "wiki/concepts/a.md",
            "---\nauthority: extremely\n---\nBody.\n",
        );
        assert_eq!(nonsense.authority, None);
    }

    #[test]
    fn source_identities_are_normalized_to_the_raw_sources_root() {
        let page = VaultPage::parse(
            "wiki/sources/a.md",
            "---\nsources: [\"raw/sources/papers/wei.pdf\", \"notes.md\", \"/home/u/p/raw/sources/x.pdf\"]\n---\nBody.\n",
        );
        assert_eq!(page.sources, vec!["papers/wei.pdf", "notes.md", "x.pdf"]);
    }

    #[test]
    fn summary_skips_headings_comments_and_code() {
        let page = VaultPage::parse(
            "wiki/concepts/a.md",
            "# Heading\n\n<!-- a note to self -->\n\n```rust\nfn main() {}\n```\n\nThe first real sentence.\n",
        );
        assert_eq!(page.summary, "The first real sentence.");
    }

    #[test]
    fn summary_reads_wikilinks_as_their_display_text() {
        let page = VaultPage::parse(
            "wiki/concepts/a.md",
            "Built on [[chain-of-thought|CoT]] and [[scaling]].\n",
        );
        assert_eq!(page.summary, "Built on CoT and scaling.");
    }

    #[test]
    fn summary_is_truncated_on_a_word_boundary() {
        let long = "word ".repeat(200);
        let page = VaultPage::parse("wiki/concepts/a.md", &long);
        assert!(page.summary.chars().count() <= SUMMARY_LIMIT + 1);
        assert!(page.summary.ends_with('…'));
        assert!(!page.summary.contains("wor…"));
    }

    #[test]
    fn a_declared_summary_wins_over_the_body() {
        let page = VaultPage::parse(
            "wiki/concepts/a.md",
            "---\nsummary: The declared one.\n---\n\nThe body one.\n",
        );
        assert_eq!(page.summary, "The declared one.");
    }

    #[test]
    fn a_bullet_list_still_yields_a_summary() {
        let page = VaultPage::parse("wiki/concepts/a.md", "- first point\n- second point\n");
        assert_eq!(page.summary, "first point second point");
    }

    #[test]
    fn parsing_is_stable_so_re_indexing_an_unchanged_page_is_a_no_op() {
        let content = "---\ntype: concept\n---\n# A\n\nBody with [[b]] and [[c]] and [[b]].\n";
        let first = VaultPage::parse("wiki/concepts/a.md", content);
        let second = VaultPage::parse("wiki/concepts/a.md", content);
        assert_eq!(first, second);
        assert_eq!(first.links, vec!["b", "c"]);
    }
}
