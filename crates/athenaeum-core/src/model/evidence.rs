//! Evidence — the answer to "why do we believe this?" (buildplan §15).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::anchor::SourceAnchor;
use super::enums::EvidenceKind;
use crate::id::{EvidenceId, NodeId};

/// A single justification attached to a node.
///
/// Large material is not duplicated here: the excerpt is content-addressed
/// into the blob store and referenced by hash (§59), so ten nodes citing the
/// same file store one copy of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub id: EvidenceId,
    /// The claim this supports.
    pub node: NodeId,
    pub kind: EvidenceKind,
    /// Where in the source this lives, when it lives in source at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<SourceAnchor>,
    /// SHA-256 of the excerpt in the blob store, when one was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_hash: Option<String>,
    /// Short inline quotation, kept in the row so that listing evidence does
    /// not require touching the blob store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// Human note about what this evidence establishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Excerpts longer than this go to the blob store instead of the row.
pub const INLINE_EXCERPT_LIMIT: usize = 2_000;

impl Evidence {
    pub fn new(node: NodeId, kind: EvidenceKind) -> Self {
        Self {
            id: EvidenceId::new(),
            node,
            kind,
            anchor: None,
            blob_hash: None,
            excerpt: None,
            note: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_id(mut self, id: impl Into<EvidenceId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_anchor(mut self, anchor: SourceAnchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    pub fn with_excerpt(mut self, excerpt: impl Into<String>) -> Self {
        self.excerpt = Some(excerpt.into());
        self
    }

    pub fn with_blob(mut self, hash: impl Into<String>) -> Self {
        self.blob_hash = Some(hash.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// True when the excerpt is too big to sit comfortably in a row and
    /// should be written to the blob store instead.
    pub fn excerpt_needs_blob(&self) -> bool {
        self.excerpt
            .as_ref()
            .is_some_and(|e| e.len() > INLINE_EXCERPT_LIMIT)
    }

    /// A one-line citation suitable for a knowledge brief.
    pub fn citation(&self) -> String {
        match &self.anchor {
            Some(a) => match (&a.symbol, a.line_start) {
                (Some(sym), _) => format!("{}:{}", a.file, sym),
                (None, Some(line)) => format!("{}:{}", a.file, line),
                (None, None) => a.file.clone(),
            },
            None => self
                .note
                .clone()
                .or_else(|| self.excerpt.clone())
                .unwrap_or_else(|| self.kind.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_prefer_the_symbol_then_the_line() {
        let node = NodeId::new();
        let with_symbol = Evidence::new(node.clone(), EvidenceKind::Symbol).with_anchor(
            SourceAnchor::file("src/auth/oauth.ts").with_symbol("OAuthCallback.handle"),
        );
        assert_eq!(
            with_symbol.citation(),
            "src/auth/oauth.ts:OAuthCallback.handle"
        );

        let with_line = Evidence::new(node.clone(), EvidenceKind::SourceFile)
            .with_anchor(SourceAnchor::file("src/auth/oauth.ts").with_lines(42, 50));
        assert_eq!(with_line.citation(), "src/auth/oauth.ts:42");

        let anchorless =
            Evidence::new(node, EvidenceKind::UserStatement).with_note("user said so in session 3");
        assert_eq!(anchorless.citation(), "user said so in session 3");
    }

    #[test]
    fn long_excerpts_are_flagged_for_the_blob_store() {
        let node = NodeId::new();
        let short = Evidence::new(node.clone(), EvidenceKind::SourceFile).with_excerpt("fn x() {}");
        assert!(!short.excerpt_needs_blob());
        let long = Evidence::new(node, EvidenceKind::SourceFile)
            .with_excerpt("x".repeat(INLINE_EXCERPT_LIMIT + 1));
        assert!(long.excerpt_needs_blob());
    }
}
