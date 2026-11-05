//! Remove a relationship from canonical Markdown, preserving prose and code examples.
use super::{
    frontmatter,
    index::{assign_ids, Resolver},
    set_fields, wikilink, FieldValue, PageRef, VaultIndexReport, VaultIndexer, VaultScanOptions,
    VaultSnapshot,
};
use crate::portable::{portable_error, write_atomic};
use crate::{
    error::Result,
    model::{EdgeType, KnowledgeEdge},
    store::KnowledgeStore,
};
use std::path::Path;

pub(crate) fn unlink_page_edge(
    store: &mut KnowledgeStore,
    root: &Path,
    edge: &KnowledgeEdge,
    page: &PageRef,
) -> Result<()> {
    let snapshot = VaultSnapshot::scan(root, VaultScanOptions::default())?;
    if !snapshot.complete {
        return Err(portable_error(
            "incomplete_scan",
            "Cannot disconnect a saved relationship while the vault scan is incomplete",
        ));
    }
    let ids = assign_ids(store, &snapshot, &mut VaultIndexReport::default())?;
    let resolver = Resolver::build(&snapshot, &ids);
    let source = snapshot
        .pages
        .iter()
        .find(|p| p.path == page.path)
        .ok_or_else(|| {
            portable_error(
                "invalid_request",
                "The relationship's source page is missing",
            )
        })?;
    let path = root.join(&page.path);
    let original = std::fs::read_to_string(&path)?;
    if crate::model::hash(&original) != source.content_hash {
        return Err(portable_error(
            "conflict",
            "The source page changed during disconnect; retry after rereading it",
        ));
    }
    let matches = |target: &str| resolver.page(target) == Some(&edge.to);
    let mut fields = Vec::new();
    if source
        .relations
        .iter()
        .any(|(kind, target)| *kind == edge.edge_type && matches(target))
    {
        let relations = source
            .relations
            .iter()
            .filter(|(kind, target)| *kind != edge.edge_type || !matches(target))
            .map(|(kind, target)| format!("{} [[{}]]", kind.as_str(), target))
            .collect();
        fields.push(("relations", FieldValue::List(relations)));
    }
    if edge.edge_type == EdgeType::RelatedTo && source.related.iter().any(|target| matches(target))
    {
        fields.push((
            "related",
            FieldValue::List(
                source
                    .related
                    .iter()
                    .filter(|target| !matches(target))
                    .map(|target| format!("[[{target}]]"))
                    .collect(),
            ),
        ));
    }
    if edge.edge_type == EdgeType::DerivedFrom
        && source
            .sources
            .iter()
            .any(|target| resolver.source(target) == Some(&edge.to))
    {
        fields.push((
            "sources",
            FieldValue::List(
                source
                    .sources
                    .iter()
                    .filter(|target| resolver.source(target) != Some(&edge.to))
                    .cloned()
                    .collect(),
            ),
        ));
    }
    let mut updated = if fields.is_empty() {
        original.clone()
    } else {
        set_fields(&original, &fields)
    };
    if edge.edge_type == EdgeType::References {
        let body = frontmatter::split(&updated).body;
        let body_offset = updated.len() - body.len();
        let mut replacements = Vec::new();
        for span in wikilink::prose_spans(body) {
            let offset = span.as_ptr() as usize - body.as_ptr() as usize + body_offset;
            let mut cursor = 0;
            while let Some(relative) = span[cursor..].find("[[") {
                let start = cursor + relative;
                let Some(close) = span[start + 2..].find("]]") else {
                    break;
                };
                let end = start + 2 + close + 2;
                let raw = &span[start..end];
                if !(start > 0 && span.as_bytes()[start - 1] == b'!') {
                    if let Some(link) = wikilink::extract(raw).first() {
                        if matches(&link.target) {
                            replacements.push((
                                offset + start,
                                offset + end,
                                link.alias.clone().unwrap_or_else(|| link.target.clone()),
                            ));
                        }
                    }
                }
                cursor = end;
            }
        }
        for (start, end, replacement) in replacements.into_iter().rev() {
            updated.replace_range(start..end, &replacement);
        }
    }
    if updated != original {
        write_atomic(&path, updated.as_bytes())?;
    }
    // Also remove a local duplicate; re-indexing preserves any other canonical relationship.
    store.remove_edge(&edge.id)?;
    VaultIndexer::scan_and_index(store, root, VaultScanOptions::default())?;
    if store
        .edges_of(&edge.from)?
        .iter()
        .any(|e| e.to == edge.to && e.edge_type == edge.edge_type)
    {
        return Err(portable_error(
            "invalid_request",
            "Another declaration still establishes this relationship",
        ));
    }
    Ok(())
}
