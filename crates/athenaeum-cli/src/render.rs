//! Turning library values into terminal output.
//!
//! Every command produces one value and renders it either as JSON or as text.
//! Both come from the same value on purpose: two separately-constructed
//! representations drift, and `--json` exists so a script can rely on seeing
//! what the human sees.

use std::collections::HashMap;
use std::path::Path;

use athenaeum_core::graph::Reached;
use athenaeum_core::journal::Event;
use athenaeum_core::model::NodeVersion;
use athenaeum_core::prelude::*;
use serde::Serialize;

use crate::check::Problem;

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
            println!("  {marker} {} → {}", neighbor.edge_type, other.title);
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
                println!(
                    "{:indent$}{} --{}--> {} [{}]",
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

pub fn problems(problems: &[Problem]) {
    if problems.is_empty() {
        println!("no problems found");
        return;
    }
    for problem in problems {
        println!("{problem}");
    }
}

/// Shorten to `max` characters, counting characters rather than bytes so a
/// multi-byte title is not cut mid-glyph.
fn truncate(text: &str, max: usize) -> String {
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
