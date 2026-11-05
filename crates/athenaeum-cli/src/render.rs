//! Turning library values into terminal output.
//!
//! Every command produces one value and renders it either as JSON or as text.
//! Both come from the same value on purpose: two separately-constructed
//! representations drift, and `--json` exists so a script can rely on seeing
//! what the human sees.

use std::collections::HashMap;
use std::path::Path;

use athenaeum_core::findings::ReviewFindingView;
use athenaeum_core::graph::{Reached, Step};
use athenaeum_core::journal::Event;
use athenaeum_core::model::NodeVersion;
use athenaeum_core::prelude::*;
use athenaeum_core::status::{IndexOutcome, SourceSummary, WorkspaceStatus};
use athenaeum_core::vault::{LintSeverity, VaultLintReport};
use serde::Serialize;

/// Emit `value` as JSON, or run `text` to render it for a human.
pub fn emit<T: Serialize>(json: bool, value: &T, text: impl FnOnce() -> Result<()>) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    } else {
        text()
    }
}

pub fn stats(stats: &Stats, root: &Path) {
    println!("nodes      {} ({} live)", stats.nodes, stats.live_nodes);
    println!("edges      {}", stats.edges);
    println!("evidence   {}", stats.evidence);
    println!("events     {}", stats.events);
    println!("schema     v{}", stats.schema_version);
    println!("location   {}", root.display());
}

/// The workspace and index state, as `athenaeum status` prints it.
///
/// `initialized: false` is rendered as a state with no numbers under it, because a column of
/// zeroes reads like a broken knowledge base rather than an absent one.
pub fn workspace_status(status: &WorkspaceStatus) {
    println!("path        {}", status.workspace_path);
    if !status.initialized {
        println!("knowledge   no knowledge base here yet; run `athenaeum init`");
        return;
    }
    if status.has_vault {
        println!(
            "knowledge   initialized, with a vault of {} page(s)",
            status.vault_pages
        );
    } else {
        println!(
            "knowledge   initialized, but there is no vault here (the repository is indexed, \
             the wiki is not)"
        );
    }
    match &status.indexed_at {
        Some(at) => println!(
            "index       {} node(s), {} edge(s), last built {at}",
            status.nodes, status.edges
        ),
        None => println!(
            "index       {} node(s), {} edge(s), never built; run `athenaeum index --scope all`",
            status.nodes, status.edges
        ),
    }
    if status.index_stale {
        println!(
            "            STALE — the files moved since it was last built; run \
             `athenaeum index --scope all`"
        );
    }
    println!(
        "review      {} proposal(s) awaiting a decision, {} finding(s)",
        status.proposals, status.findings
    );
    println!(
        "protocol    v{} (engine {})",
        status.protocol_version, status.engine_version
    );
}

/// Every file the index has read, with its hash, coverage and freshness.
///
/// `limit` is what the caller asked for, so a listing that filled its budget says so instead of
/// looking like the whole inventory.
pub fn sources(sources: &[SourceSummary], limit: u32) {
    if sources.is_empty() {
        println!(
            "nothing has been indexed from this project yet; run `athenaeum index --scope all`"
        );
        return;
    }
    for source in sources {
        // The scope is part of the identity: the same path can be a file of the code repository
        // and source material the wiki cites, and those are two records, not a duplicate.
        println!(
            "{:<10} {}  {}  {} node(s), {} anchor(s)  {}{}",
            source.scope.as_str(),
            truncate(&source.path, 56),
            human_bytes(source.bytes),
            source.nodes,
            source.anchors,
            if source.scanned {
                "scanned"
            } else {
                "not scanned"
            },
            if source.stale { ", STALE" } else { "" }
        );
        if source.sha256.is_empty() {
            println!("    sha256 not computed (the scan did not read it)");
        } else {
            println!("    sha256 {}", source.sha256);
        }
        if let Some(at) = &source.indexed_at {
            println!("    indexed {at}");
        }
    }
    if limit > 0 && sources.len() as u32 >= limit {
        println!(
            "… {} shown; there may be more, pass --limit for the rest",
            sources.len()
        );
    }
}

/// What one `athenaeum index --scope` pass did.
pub fn index_outcome(outcome: &IndexOutcome) {
    println!(
        "indexed {} vault page(s), {} repository file(s), {} symbol(s); the graph holds {} edge(s) after {} ms",
        outcome.vault_pages, outcome.repository_files, outcome.symbols, outcome.edges, outcome.duration_ms
    );
    if !outcome.skipped.is_empty() {
        println!("{} path(s) skipped:", outcome.skipped.len());
        let shown = outcome.skipped.len().min(10);
        for path in &outcome.skipped[..shown] {
            println!("  {path}");
        }
        if shown < outcome.skipped.len() {
            println!(
                "  … and {} more; `athenaeum review` names the reason for each",
                outcome.skipped.len() - shown
            );
        }
    }
    if outcome.incomplete {
        eprintln!(
            "warning: a scan budget ended a walk early; nothing was deleted on that basis, and \
             `athenaeum review` reports it as an incomplete scan"
        );
    }
}

/// Bytes at human scale, so a listing is readable without arithmetic.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn node_list(nodes: &[KnowledgeNode]) {
    if nodes.is_empty() {
        println!("(nothing yet)");
        return;
    }
    for node in nodes {
        println!(
            "{:<10} {:<40} {:<12} {}",
            node.node_type,
            truncate(&node.title, 40),
            node.status,
            node.id
        );
    }
}

pub fn search_hits(hits: &[SearchHit]) {
    for hit in hits {
        println!(
            "{:>6.2}  {:<40} {}",
            hit.score,
            truncate(&hit.node.title, 40),
            hit.node.id
        );
    }
}

/// A node with everything known about it.
pub fn node_detail(store: &KnowledgeStore, node: &KnowledgeNode) -> Result<()> {
    println!("{}", node.title);
    println!("{}", "-".repeat(node.title.chars().count().max(3)));
    println!("id         {}", node.id);
    println!("type       {}", node.node_type);
    println!("status     {}", node.status);
    println!("authority  {}", node.authority);
    println!("scope      {}", node.scope);
    println!("version    {}", node.version);
    println!("source     {}", node.provenance.source);

    if !node.summary.is_empty() {
        println!("\n{}", node.summary);
    }
    if !node.content.is_empty() {
        println!("\n{}", node.content);
    }

    let neighbors = store.neighbors(&node.id, &EdgeFilter::all())?;
    if !neighbors.is_empty() {
        println!("\nrelationships");
        for neighbor in neighbors {
            let other = store.get_node(&neighbor.node)?;
            // Proposed edges are shown but marked, never presented as though
            // they were established knowledge (§20).
            let marker = if neighbor.edge.is_traversable() {
                " "
            } else {
                "?"
            };
            // Why the relationship exists is part of it: an edge with no reason is a claim about
            // the graph, and a reader who cannot see the note cannot judge it.
            let reason = reasons(
                neighbor.edge.note.as_deref(),
                neighbor.edge.confidence.map(f64::from),
            );
            println!(
                "  {marker} {} → {}{reason}",
                neighbor.edge_type, other.title
            );
        }
    }

    let evidence = store.evidence_for(&node.id)?;
    if !evidence.is_empty() {
        println!("\nevidence");
        for item in evidence {
            println!("  {} ({})", item.citation(), item.kind);
        }
    }

    let utility = store.utility(&node.id)?;
    if utility.retrievals > 0 {
        println!(
            "\nretrieved {} time(s), used {} time(s)",
            utility.retrievals, utility.uses
        );
    }

    Ok(())
}

/// A breadth-first walk, with each step's origin named.
///
/// Named rather than implied by indentation: at depth 2 the line directly
/// above a node is usually a sibling, not its parent.
pub fn navigation(reached: &[Reached]) {
    // Every `via.from` is a node the walk already arrived at, so the titles
    // are all in hand — no need to go back to the store per step.
    let titles: HashMap<&NodeId, &str> = reached
        .iter()
        .map(|item| (&item.node.id, item.node.title.as_str()))
        .collect();

    for item in reached {
        match &item.via {
            None => println!("{} [{}]  (seed)", item.node.title, item.node.node_type),
            Some(step) => {
                let from = titles
                    .get(&step.from)
                    .copied()
                    .unwrap_or_else(|| step.from.as_str());
                let reason = reasons(step.note.as_deref(), step.confidence);
                println!(
                    "{:indent$}{} --{}--> {} [{}]{reason}",
                    "",
                    from,
                    step.edge_type,
                    item.node.title,
                    item.node.node_type,
                    indent = (item.depth as usize) * 2
                );
            }
        }
    }
}

pub fn history(node: &KnowledgeNode, versions: &[NodeVersion]) {
    println!("{} (currently v{})", node.title, node.version);
    if versions.is_empty() {
        println!("  no earlier revisions");
    }
    for version in versions {
        println!(
            "  v{} {}  {}",
            version.version,
            version.recorded_at.format("%Y-%m-%d %H:%M"),
            version.reason.as_deref().unwrap_or("(no reason recorded)")
        );
    }
}

pub fn journal(events: &[Event]) {
    for event in events {
        println!(
            "{}  {:<22} {}",
            event.at.format("%Y-%m-%d %H:%M:%S"),
            event.kind,
            event.subject.as_deref().unwrap_or("")
        );
    }
}

/// The outcome of a vault indexing pass.
///
/// Written so the common case reads as one line — a pass over an unchanged
/// vault should say "nothing changed" and stop, not print five zeroes.
pub fn vault_index(report: &VaultIndexReport, show_unresolved: bool) {
    println!(
        "vault: {} page(s), {} source(s), {} link(s) resolved",
        report.pages, report.sources, report.links_resolved
    );
    if report.is_no_op() {
        println!("index: already up to date");
    } else {
        println!(
            "nodes: {} created, {} updated, {} removed",
            report.nodes_created, report.nodes_updated, report.nodes_removed
        );
        println!(
            "edges: {} created, {} removed",
            report.edges_created, report.edges_removed
        );
        println!(
            "evidence: {} attached, {} updated, {} removed",
            report.evidence_attached, report.evidence_updated, report.evidence_removed
        );
    }

    if report.links_unresolved > 0 {
        println!(
            "{} link(s) name no page in the vault",
            report.links_unresolved
        );
        let shown = if show_unresolved {
            report.unresolved.len()
        } else {
            report.unresolved.len().min(5)
        };
        for link in &report.unresolved[..shown] {
            println!("  {} → [[{}]]", link.from, link.target);
        }
        if shown < report.unresolved.len() {
            println!(
                "  … and {} more; pass --show-unresolved for all of them",
                report.unresolved.len() - shown
            );
        }
    }

    if !report.scan_complete {
        eprintln!("warning: the scan was truncated, so nothing was removed");
    }
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
}

/// The review queue, as `athenaeum review` prints it.
///
/// The finding type is the contract's own — the same value `--json` serializes and the bridge
/// returns — so a person and a script cannot be shown different queues. `limit` is what the
/// caller asked for: a queue that filled its budget says so rather than looking complete.
pub fn review_findings(findings: &[ReviewFindingView], limit: u32) {
    if findings.is_empty() {
        println!("nothing needs a decision");
        return;
    }
    let blocking = findings.iter().filter(|finding| finding.blocking).count();
    println!(
        "{} finding(s), {} blocking; `athenaeum review --json` is the same queue for a script",
        findings.len(),
        blocking
    );
    for finding in findings {
        let mark = if finding.blocking { "!" } else { "-" };
        if finding.title.is_empty() {
            println!("{mark} {} ({})", finding.kind, finding.severity);
        } else {
            println!(
                "{mark} {} ({})  {}",
                finding.kind, finding.severity, finding.title
            );
        }
        if !finding.detail.is_empty() {
            println!("    {}", finding.detail);
        }
        if let Some(path) = &finding.source_path {
            println!("    source: {path}");
        }
        if !finding.nodes.is_empty() {
            println!("    nodes: {}", joined_ids(&finding.nodes));
        }
        if !finding.evidence.is_empty() {
            println!("    evidence: {}", joined_ids(&finding.evidence));
        }
        println!("    confidence: {:.2}", finding.confidence);
    }
    if limit > 0 && findings.len() as u32 >= limit {
        println!(
            "… {} shown; the queue may be longer, pass --limit for the rest",
            findings.len()
        );
    }
}

fn joined_ids<T: std::fmt::Display>(values: &[T]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn vault_lint(report: &VaultLintReport) {
    if report.findings.is_empty() {
        println!("{} page(s) linted, nothing to report", report.pages);
    } else {
        println!(
            "{} page(s) linted, {} finding(s)",
            report.pages,
            report.findings.len()
        );
    }

    for finding in &report.findings {
        let mark = match finding.severity {
            LintSeverity::Warning => "!",
            LintSeverity::Info => "-",
        };
        println!("{mark} {} — {}", finding.page, finding.detail);
        if let Some(target) = &finding.suggested_target {
            println!("    link to: {target}");
        }
        if let Some(source) = &finding.suggested_source {
            println!("    link from: {source}");
        }
    }

    if report.broken_links_truncated {
        eprintln!("warning: too many broken links to list; fix these and lint again");
    }
    if !report.scan_complete {
        eprintln!("warning: the scan was truncated, so this is not the whole vault");
    }
}

/// The chain of relationships that connects two nodes, as a reader would
/// explain it: each line names the relationship and where it led.
pub fn path(
    store: &KnowledgeStore,
    from: &KnowledgeNode,
    to: &KnowledgeNode,
    steps: Option<&[Step]>,
) -> Result<()> {
    let Some(steps) = steps else {
        println!(
            "no path from {:?} to {:?} within the search depth",
            from.title, to.title
        );
        return Ok(());
    };
    if steps.is_empty() {
        println!("{:?} is {:?}", from.title, to.title);
        return Ok(());
    }

    println!("{} hop(s):", steps.len());
    println!("{}", from.title);
    // A step records the edge and the node it was traversed *from*, so the
    // node each one arrived at is named by the next step's origin — and for
    // the last one, by the destination itself.
    for (index, step) in steps.iter().enumerate() {
        let arrived_id = match steps.get(index + 1) {
            Some(next) => &next.from,
            None => &to.id,
        };
        let arrived = store.get_node(arrived_id)?.title;
        println!(
            "  --{}-> {}{}",
            step.edge_type,
            arrived,
            reasons(step.note.as_deref(), step.confidence)
        );
    }
    Ok(())
}

pub fn review(report: &ReviewReport) {
    if report.findings.is_empty() {
        println!("{} node(s) reviewed, nothing to report", report.nodes);
        return;
    }
    println!(
        "{} node(s) reviewed, {} finding(s)",
        report.nodes,
        report.findings.len()
    );
    let counts = report.counts();
    println!(
        "  {}",
        counts
            .iter()
            .map(|(kind, count)| format!("{kind} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut current = None;
    for finding in &report.findings {
        if current != Some(finding.kind) {
            println!();
            println!(
                "{} {}",
                if finding.kind.is_blocking() { "!" } else { "-" },
                finding.kind
            );
            current = Some(finding.kind);
        }
        if finding.title.is_empty() {
            println!("    {} — {}", finding.node, finding.problem);
        } else {
            println!("    {} — {}", finding.title, finding.problem);
        }
    }
}

/// Shorten to `max` characters, counting characters rather than bytes so a
/// multi-byte title is not cut mid-glyph.
pub fn truncate(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    // Take one more than the budget allows: whether a further character
    // exists is what distinguishes "fits exactly" from "was cut short",
    // and it costs no more than reading `max + 1` characters either way.
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_none() {
        return head;
    }
    let mut out: String = head.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The reason a relationship exists, as one line's tail.
///
/// The engine's own note, verbatim — paraphrasing it here would make the reason something this
/// renderer wrote rather than what the edge says — and the confidence only when the edge carries
/// one. Absent both, the line ends where it always did.
fn reasons(note: Option<&str>, confidence: Option<f64>) -> String {
    let mut tail = String::new();
    if let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) {
        tail.push_str("  (");
        tail.push_str(note);
        tail.push(')');
    }
    if let Some(confidence) = confidence {
        tail.push_str(&format!("  [confidence {confidence:.2}]"));
    }
    tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(truncate("oauth", 40), "oauth");
        // Exactly at the limit still fits: nothing was left out.
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn long_text_is_ellipsized_within_budget() {
        let out = truncate("abcdefghij", 5);
        assert_eq!(out, "abcd…");
        assert_eq!(out.chars().count(), 5);
    }

    #[test]
    fn multibyte_titles_are_cut_on_character_boundaries() {
        // Byte-based truncation would panic or produce invalid UTF-8 here.
        let out = truncate("知識グラフのノード", 4);
        assert_eq!(out.chars().count(), 4);
        assert!(out.ends_with('…'));
    }
}
