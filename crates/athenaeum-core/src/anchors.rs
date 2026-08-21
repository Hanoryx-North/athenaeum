//! Search and persistence for source-anchor relocation (buildplan §16/§79).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{EvidenceId, NodeId};
use crate::model::{hash, AnchorResolution, NodeStatus, SourceAnchor};
use crate::repository::{
    region_by_lines, validate_relative_path, RepositoryScanOptions, RepositorySnapshot,
    RepositorySymbol,
};
use crate::store::KnowledgeStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorRelocation {
    pub resolution: AnchorResolution,
    pub anchor: SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRelocation {
    pub evidence: EvidenceId,
    pub node: NodeId,
    pub resolution: AnchorResolution,
    pub from_file: String,
    pub to_file: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorRelocationReport {
    pub exact: usize,
    pub relocated_by_symbol: usize,
    pub relocated_by_fingerprint: usize,
    pub stale: usize,
    pub results: Vec<EvidenceRelocation>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct AnchorRelocator {
    root: std::path::PathBuf,
    snapshot: RepositorySnapshot,
    contents: HashMap<String, String>,
}

impl AnchorRelocator {
    pub fn scan(root: impl AsRef<Path>, mut options: RepositoryScanOptions) -> Result<Self> {
        options.max_commits = 0;
        let root = root.as_ref().to_path_buf();
        let snapshot = RepositorySnapshot::scan(&root, options)?;
        let mut contents = HashMap::new();
        for file in &snapshot.files {
            let content = std::fs::read_to_string(root.join(&file.path))?;
            if hash(&content) != file.content_hash {
                return Err(Error::InvalidValue {
                    field: "repository snapshot",
                    value: format!("{} changed after it was scanned", file.path),
                });
            }
            contents.insert(file.path.clone(), content);
        }
        Ok(Self {
            root,
            snapshot,
            contents,
        })
    }

    pub fn resolve(&self, anchor: &SourceAnchor) -> Result<AnchorRelocation> {
        validate_relative_path(&anchor.file, "anchor file")?;
        let original_path = self.root.join(&anchor.file);
        let original_content = match std::fs::read_to_string(&original_path) {
            Ok(content) => Some(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };

        if let Some(content) = original_content.as_deref() {
            if anchor_matches_exactly(anchor, content) {
                return Ok(AnchorRelocation {
                    resolution: AnchorResolution::Exact,
                    anchor: anchor.clone(),
                });
            }
        }

        if let Some(symbol) = &anchor.symbol {
            let matches = self.symbol_matches(symbol);
            let same_file = matches
                .iter()
                .filter(|(file, _)| file.path == anchor.file)
                .collect::<Vec<_>>();
            if same_file.len() == 1 {
                let (file, found) = *same_file[0];
                let content = self.content(&file.path)?;
                let region =
                    region_by_lines(content, found.line_start, found.line_end).unwrap_or_default();
                let resolution = if anchor.content_hash.is_none()
                    && anchor.region_hash.is_none()
                    && anchor
                        .line_start
                        .is_none_or(|line| line == found.line_start)
                {
                    AnchorResolution::Exact
                } else {
                    AnchorResolution::RelocatedBySymbol
                };
                return Ok(AnchorRelocation {
                    resolution,
                    anchor: relocated_anchor(anchor, &file.path, found, region, content),
                });
            }
            if self.snapshot.complete && matches.len() == 1 {
                let (file, found) = matches[0];
                let content = self.content(&file.path)?;
                let region =
                    region_by_lines(content, found.line_start, found.line_end).unwrap_or_default();
                let relocated = relocated_anchor(anchor, &file.path, found, region, content);
                return Ok(AnchorRelocation {
                    resolution: AnchorResolution::RelocatedBySymbol,
                    anchor: relocated,
                });
            }
        }

        if let Some(relocated) = self.find_fingerprint(anchor)? {
            return Ok(relocated);
        }

        self.require_complete()?;

        Ok(AnchorRelocation {
            resolution: AnchorResolution::Stale,
            anchor: anchor.clone(),
        })
    }

    pub fn relocate_store(&self, store: &mut KnowledgeStore) -> Result<AnchorRelocationReport> {
        let evidence = store.anchored_evidence()?;
        let resolved = evidence
            .into_iter()
            .filter_map(|item| item.anchor.clone().map(|anchor| (item, anchor)))
            .map(|(item, anchor)| {
                self.resolve(&anchor)
                    .map(|relocated| (item, anchor, relocated))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut report = AnchorRelocationReport {
            warnings: self.snapshot.warnings.clone(),
            ..AnchorRelocationReport::default()
        };
        let mut outcomes = HashMap::<NodeId, Vec<AnchorResolution>>::new();

        for (item, anchor, relocated) in resolved {
            match relocated.resolution {
                AnchorResolution::Exact => report.exact += 1,
                AnchorResolution::RelocatedBySymbol => report.relocated_by_symbol += 1,
                AnchorResolution::RelocatedByFingerprint => {
                    report.relocated_by_fingerprint += 1;
                }
                AnchorResolution::Stale => report.stale += 1,
            }
            if relocated.resolution.is_resolved() && relocated.anchor != anchor {
                store.update_evidence_anchor(&item.id, relocated.anchor.clone())?;
            }
            if relocated.resolution != AnchorResolution::Exact {
                report.results.push(EvidenceRelocation {
                    evidence: item.id.clone(),
                    node: item.node.clone(),
                    resolution: relocated.resolution,
                    from_file: anchor.file,
                    to_file: relocated.anchor.file.clone(),
                });
            }
            outcomes
                .entry(item.node)
                .or_default()
                .push(relocated.resolution);
        }

        for (node_id, resolutions) in outcomes {
            let node = store.get_node(&node_id)?;
            if !matches!(
                node.status,
                NodeStatus::Valid | NodeStatus::Stale | NodeStatus::StaleRequiresReview
            ) {
                continue;
            }
            let status = aggregate_status(&resolutions);
            store.set_node_status(&node_id, status)?;
        }

        Ok(report)
    }

    fn symbol_matches(
        &self,
        requested: &str,
    ) -> Vec<(&crate::repository::RepositoryFile, &RepositorySymbol)> {
        self.snapshot
            .files
            .iter()
            .flat_map(|file| {
                file.symbols
                    .iter()
                    .filter(move |symbol| symbol.qualified_name == requested)
                    .map(move |symbol| (file, symbol))
            })
            .collect()
    }

    fn find_fingerprint(&self, anchor: &SourceAnchor) -> Result<Option<AnchorRelocation>> {
        let (Some(region_hash), Some(start), Some(end)) = (
            anchor.region_hash.as_deref(),
            anchor.line_start,
            anchor.line_end,
        ) else {
            return Ok(None);
        };
        if start == 0 || end < start {
            return Ok(None);
        }
        let line_count = end - start + 1;
        let mut matches = Vec::new();

        for file in &self.snapshot.files {
            let content = self.content(&file.path)?;
            let total_lines = content.lines().count() as u32;
            if total_lines < line_count {
                continue;
            }
            for candidate_start in 1..=total_lines - line_count + 1 {
                let candidate_end = candidate_start + line_count - 1;
                let Some(region) = region_by_lines(content, candidate_start, candidate_end) else {
                    continue;
                };
                if hash(region) == region_hash {
                    matches.push((
                        file.path.clone(),
                        candidate_start,
                        candidate_end,
                        region.to_string(),
                    ));
                }
            }
        }

        let same_file = matches
            .iter()
            .enumerate()
            .filter(|(_, (file, _, _, _))| file == &anchor.file)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let selected = if same_file.len() == 1 {
            same_file[0]
        } else {
            self.require_complete()?;
            if matches.len() != 1 {
                return Ok(None);
            }
            0
        };
        let (file, line_start, line_end, region) = matches.swap_remove(selected);
        let content = self.content(&file)?;
        if matches
            .iter()
            .any(|(other_file, _, _, _)| other_file == &file)
        {
            return Ok(None);
        }
        let mut relocated = anchor.clone();
        relocated.file = file;
        relocated.line_start = Some(line_start);
        relocated.line_end = Some(line_end);
        relocated = relocated.fingerprint(&region, content);
        Ok(Some(AnchorRelocation {
            resolution: AnchorResolution::RelocatedByFingerprint,
            anchor: relocated,
        }))
    }

    fn content(&self, file: &str) -> Result<&str> {
        self.contents
            .get(file)
            .map(String::as_str)
            .ok_or_else(|| Error::InvalidValue {
                field: "repository snapshot",
                value: file.to_string(),
            })
    }

    fn require_complete(&self) -> Result<()> {
        if self.snapshot.complete {
            Ok(())
        } else {
            Err(Error::IncompleteRepositoryScan {
                operation: "relocate an unresolved anchor",
            })
        }
    }
}

fn anchor_matches_exactly(anchor: &SourceAnchor, content: &str) -> bool {
    if anchor
        .content_hash
        .as_deref()
        .is_some_and(|expected| expected == hash(content))
    {
        return true;
    }
    if let (Some(expected), Some(start), Some(end)) = (
        anchor.region_hash.as_deref(),
        anchor.line_start,
        anchor.line_end,
    ) {
        if region_by_lines(content, start, end).is_some_and(|region| hash(region) == expected) {
            return true;
        }
    }
    anchor.content_hash.is_none() && anchor.region_hash.is_none() && anchor.symbol.is_none()
}

fn relocated_anchor(
    original: &SourceAnchor,
    file: &str,
    symbol: &RepositorySymbol,
    region: &str,
    content: &str,
) -> SourceAnchor {
    let mut relocated = original.clone();
    relocated.file = file.to_string();
    relocated.symbol = Some(symbol.qualified_name.clone());
    relocated.line_start = Some(symbol.line_start);
    relocated.line_end = Some(symbol.line_end);
    relocated.fingerprint(region, content)
}

fn aggregate_status(resolutions: &[AnchorResolution]) -> NodeStatus {
    if resolutions
        .iter()
        .all(|resolution| *resolution == AnchorResolution::Stale)
    {
        NodeStatus::StaleRequiresReview
    } else if resolutions.iter().any(|resolution| {
        matches!(
            resolution,
            AnchorResolution::Stale | AnchorResolution::RelocatedBySymbol
        )
    }) {
        NodeStatus::Stale
    } else {
        NodeStatus::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Evidence, EvidenceKind, KnowledgeNode, NodeType};

    fn options() -> RepositoryScanOptions {
        RepositoryScanOptions {
            max_commits: 0,
            ..RepositoryScanOptions::default()
        }
    }

    #[test]
    fn resolves_exact_symbol_and_fingerprint_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let original = "export class OAuthCallback {\n  handle() {}\n}\n";
        std::fs::write(dir.path().join("oauth.ts"), original).unwrap();
        let exact = SourceAnchor::file("oauth.ts")
            .with_symbol("OAuthCallback.handle")
            .with_lines(2, 2)
            .fingerprint("  handle() {}", original);
        let relocator = AnchorRelocator::scan(dir.path(), options()).unwrap();
        assert_eq!(
            relocator.resolve(&exact).unwrap().resolution,
            AnchorResolution::Exact
        );

        let changed = "// moved\nexport class OAuthCallback {\n  handle() { return; }\n}\n";
        std::fs::write(dir.path().join("oauth.ts"), changed).unwrap();
        let relocator = AnchorRelocator::scan(dir.path(), options()).unwrap();
        let by_symbol = relocator.resolve(&exact).unwrap();
        assert_eq!(by_symbol.resolution, AnchorResolution::RelocatedBySymbol);
        assert_eq!(by_symbol.anchor.line_start, Some(3));

        std::fs::write(dir.path().join("oauth.ts"), "// removed\n").unwrap();
        std::fs::write(
            dir.path().join("callback.ts"),
            "const prefix = true;\n  handle() {}\n",
        )
        .unwrap();
        let relocator = AnchorRelocator::scan(dir.path(), options()).unwrap();
        let by_fingerprint = relocator.resolve(&exact).unwrap();
        assert_eq!(
            by_fingerprint.resolution,
            AnchorResolution::RelocatedByFingerprint
        );
        assert_eq!(by_fingerprint.anchor.file, "callback.ts");
        assert_eq!(by_fingerprint.anchor.line_start, Some(2));
    }

    #[test]
    fn unresolved_and_escaping_anchors_fail_safely() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source.rs"), "fn replacement() {}\n").unwrap();
        let relocator = AnchorRelocator::scan(dir.path(), options()).unwrap();
        let stale = SourceAnchor::file("missing.rs")
            .with_symbol("gone")
            .with_lines(1, 1)
            .fingerprint("fn gone() {}", "fn gone() {}\n");
        assert_eq!(
            relocator.resolve(&stale).unwrap().resolution,
            AnchorResolution::Stale
        );
        let escaping = SourceAnchor::file("../outside.rs");
        assert!(matches!(
            relocator.resolve(&escaping),
            Err(Error::InvalidValue {
                field: "anchor file",
                ..
            })
        ));
    }

    #[test]
    fn an_incomplete_scan_never_turns_missing_search_results_into_staleness() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
        let relocator = AnchorRelocator::scan(
            dir.path(),
            RepositoryScanOptions {
                max_files: 1,
                max_commits: 0,
                ..RepositoryScanOptions::default()
            },
        )
        .unwrap();
        let anchor = SourceAnchor::file("missing.rs")
            .with_symbol("moved")
            .with_lines(1, 1)
            .fingerprint("fn moved() {}", "fn moved() {}\n");
        assert!(matches!(
            relocator.resolve(&anchor),
            Err(Error::IncompleteRepositoryScan { .. })
        ));
    }

    #[test]
    fn relocation_persists_new_anchors_and_updates_node_staleness() {
        let dir = tempfile::tempdir().unwrap();
        let old = "export class Wiki {\n  open() {}\n}\n";
        std::fs::write(dir.path().join("wiki.ts"), old).unwrap();
        let anchor = SourceAnchor::file("wiki.ts")
            .with_symbol("Wiki.open")
            .with_lines(2, 2)
            .fingerprint("  open() {}", old);
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Fact, "Wiki opens nodes"))
            .unwrap();
        let evidence = store
            .attach_evidence(
                Evidence::new(node.id.clone(), EvidenceKind::Symbol).with_anchor(anchor),
            )
            .unwrap();

        std::fs::write(
            dir.path().join("wiki.ts"),
            "export class Wiki {\n  open() { return true; }\n}\n",
        )
        .unwrap();
        let report = AnchorRelocator::scan(dir.path(), options())
            .unwrap()
            .relocate_store(&mut store)
            .unwrap();
        assert_eq!(report.relocated_by_symbol, 1);
        assert_eq!(store.get_node(&node.id).unwrap().status, NodeStatus::Stale);
        assert_ne!(
            store
                .find_evidence(&evidence.id)
                .unwrap()
                .unwrap()
                .anchor
                .unwrap()
                .region_hash,
            evidence.anchor.unwrap().region_hash
        );
    }

    #[test]
    fn symbol_only_anchors_are_exact_and_enriched_when_the_symbol_has_not_moved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wiki.ts"),
            "export class Wiki {\n  open() {}\n}\n",
        )
        .unwrap();
        let mut store = KnowledgeStore::open_in_memory().unwrap();
        let node = store
            .create_node(KnowledgeNode::new(NodeType::Fact, "Wiki opens nodes"))
            .unwrap();
        let evidence = store
            .attach_evidence(
                Evidence::new(node.id.clone(), EvidenceKind::Symbol)
                    .with_anchor(SourceAnchor::file("wiki.ts").with_symbol("Wiki.open")),
            )
            .unwrap();

        let report = AnchorRelocator::scan(dir.path(), options())
            .unwrap()
            .relocate_store(&mut store)
            .unwrap();
        assert_eq!(report.exact, 1);
        assert_eq!(store.get_node(&node.id).unwrap().status, NodeStatus::Valid);
        let enriched = store
            .find_evidence(&evidence.id)
            .unwrap()
            .unwrap()
            .anchor
            .unwrap();
        assert_eq!(enriched.line_start, Some(2));
        assert!(enriched.region_hash.is_some());
        assert!(enriched.content_hash.is_some());
    }
}
