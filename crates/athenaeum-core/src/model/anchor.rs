//! Source anchors — the link from a claim back to the code that justifies it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A reference into source that is meant to survive the code moving.
///
/// Buildplan §16: a path and a line number alone are worthless after the next
/// refactor. Each additional locator here is a fallback for when the previous
/// one stops matching.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAnchor {
    /// Repository-relative path. Always stored with forward slashes so a
    /// graph created on Windows resolves on macOS and vice versa.
    pub file: String,
    /// Fully-qualified where possible, e.g. `OAuthCallback.handle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
    /// Hash of just the anchored region. Survives edits elsewhere in the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_hash: Option<String>,
    /// Hash of the whole file at the time of anchoring. A cheap way to know
    /// nothing has changed at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// The commit the anchor was taken against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl SourceAnchor {
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            file: normalize_path(path.into()),
            ..Default::default()
        }
    }

    pub fn with_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = Some(symbol.into());
        self
    }

    pub fn with_lines(mut self, start: u32, end: u32) -> Self {
        self.line_start = Some(start);
        self.line_end = Some(end);
        self
    }

    pub fn with_commit(mut self, commit: impl Into<String>) -> Self {
        self.commit = Some(commit.into());
        self
    }

    /// Record fingerprints for the anchored region and the whole file.
    pub fn fingerprint(mut self, region: &str, whole_file: &str) -> Self {
        self.region_hash = Some(hash(region));
        self.content_hash = Some(hash(whole_file));
        self
    }

    /// How strongly this anchor pins down a location, used to decide which of
    /// several anchors to try first during relocation.
    pub fn specificity(&self) -> u8 {
        let mut score = 0;
        if !self.file.is_empty() {
            score += 1;
        }
        if self.symbol.is_some() {
            score += 2;
        }
        if self.region_hash.is_some() {
            score += 2;
        }
        if self.line_start.is_some() {
            score += 1;
        }
        score
    }
}

/// The outcome of trying to find an anchor again after the code moved (§16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorResolution {
    /// Everything still matches; nothing moved.
    Exact,
    /// The file changed but the named symbol was found again.
    RelocatedBySymbol,
    /// The symbol is gone but an identical region was found by fingerprint.
    RelocatedByFingerprint,
    /// Nothing matched. The claim is not necessarily wrong — but a human or
    /// a verifier model has to look at it before it is trusted again.
    Stale,
}

impl AnchorResolution {
    /// True when the anchor still points at real, identified source.
    pub fn is_resolved(&self) -> bool {
        !matches!(self, AnchorResolution::Stale)
    }
}

/// SHA-256, lowercase hex. Also the addressing scheme for evidence blobs.
pub fn hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Backslashes to forward slashes, so anchors written by the Windows build
/// resolve unchanged on macOS and Linux.
fn normalize_path(path: String) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_are_normalized_on_the_way_in() {
        let a = SourceAnchor::file(r"src\auth\oauth.ts");
        assert_eq!(a.file, "src/auth/oauth.ts");
    }

    #[test]
    fn more_locators_means_more_specific() {
        let bare = SourceAnchor::file("src/auth/oauth.ts");
        let precise = SourceAnchor::file("src/auth/oauth.ts")
            .with_symbol("OAuthCallback.handle")
            .with_lines(10, 40)
            .fingerprint("fn handle() {}", "whole file");
        assert!(precise.specificity() > bare.specificity());
    }

    #[test]
    fn hashing_is_stable_and_content_sensitive() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
        assert_eq!(hash("abc").len(), 64);
    }

    #[test]
    fn only_stale_counts_as_unresolved() {
        assert!(AnchorResolution::Exact.is_resolved());
        assert!(AnchorResolution::RelocatedBySymbol.is_resolved());
        assert!(!AnchorResolution::Stale.is_resolved());
    }
}
