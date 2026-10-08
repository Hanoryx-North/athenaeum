//! Reading one markdown page into the values the indexer needs.
//!
//! Everything here is a pure function of a path and a string, so the whole
//! of the vault's interpretation — what a page is called, what type it
//! claims to be, what it links to, what it rests on — is testable without
//! touching a filesystem.

use crate::model::{hash, Authority, EdgeType, EvidenceKind, Scope};

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
    /// A `scope:` the page declared, when it declared a valid one. Pages that say nothing are
    /// project scope, as they always were.
    pub scope: Option<Scope>,
    /// Typed relationships the page states: `relations: ["depends_on [[retry-policy]]"]`. A
    /// `related:` entry says two pages are related; this says how.
    pub relations: Vec<(EdgeType, String)>,
    /// Evidence the page carries with it, so an accepted claim keeps its justification when it
    /// moves to another machine.
    pub evidence: Vec<DeclaredEvidence>,
}

/// One `evidence:` entry: a JSON object on one line.
///
/// ```yaml
/// evidence:
///   - '{"kind":"source_file","file":"src/auth.rs","symbol":"validate","excerpt":"…"}'
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeclaredEvidence {
    #[serde(default)]
    pub kind: EvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
            scope: fm.text("scope").and_then(|raw| raw.trim().parse().ok()),
            relations: relations_of(&fm),
            evidence: fm
                .list("evidence")
                .into_iter()
                .filter_map(|raw| serde_json::from_str::<DeclaredEvidence>(raw).ok())
                .take(MAX_DECLARED_EVIDENCE)
                .collect(),
            content_hash: hash(content),
            body: body.to_string(),
            page_type,
            type_declared,
            slug,
            path: path.to_string(),
        }
    }
}

/// More than this many `evidence:` entries on one page is a page trying to flood the index.
const MAX_DECLARED_EVIDENCE: usize = 50;

/// `relations:` entries, each `<edge_type> <target>` where the target is written like a `related:`
/// entry. Entries naming an edge type the model does not have are ignored.
fn relations_of(fm: &Frontmatter) -> Vec<(EdgeType, String)> {
    let mut seen = std::collections::HashSet::new();
    fm.list("relations")
        .into_iter()
        .filter_map(|raw| {
            let (kind, target) = raw.trim().split_once(char::is_whitespace)?;
            let edge_type = kind.trim().parse::<EdgeType>().ok()?;
            let target = link_target(target)?;
            Some((edge_type, target))
        })
        .filter(|(edge_type, target)| seen.insert((edge_type.as_str(), target.clone())))
        .collect()
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
