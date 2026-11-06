//! The sync protocol's shared vectors. Library holds a copy and checks the same files, so the two
//! sides cannot drift on what a path may be or what a tree hashes to.

use std::path::PathBuf;

use athenaeum_core::portable::syncset::{
    canonical_manifest, has_sync_extension, tree_hash, validate_path, ManifestEntry,
};
use serde::Deserialize;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/sync-protocol/v1")
        .join(name)
}

#[derive(Deserialize)]
struct Paths {
    valid: Vec<String>,
    invalid: Vec<Invalid>,
}

#[derive(Deserialize)]
struct Invalid {
    path: String,
    why: String,
}

#[test]
fn the_client_sync_set_matches_the_hosted_allowlist() {
    #[derive(Deserialize)]
    struct Extensions {
        allowed: Vec<String>,
        refused: Vec<Invalid>,
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/protocol/fixtures/sync/v1/extensions.json");
    let cases: Extensions = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for path in cases.allowed {
        assert!(
            validate_path(&path).is_ok() && has_sync_extension(&path),
            "{path}"
        );
    }
    for case in cases.refused {
        assert!(
            !(validate_path(&case.path).is_ok() && has_sync_extension(&case.path)),
            "{}: {}",
            case.path,
            case.why
        );
    }
}

#[test]
fn the_path_rules_match_the_shared_vectors() {
    let paths: Paths =
        serde_json::from_slice(&std::fs::read(fixture("paths.json")).unwrap()).unwrap();
    for path in &paths.valid {
        assert_eq!(validate_path(path), Ok(()), "{path:?} should be valid");
    }
    for case in &paths.invalid {
        assert!(
            validate_path(&case.path).is_err(),
            "{:?} should be refused ({})",
            case.path,
            case.why
        );
    }
}

#[derive(Deserialize)]
struct Manifests {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    entries: Vec<ManifestEntry>,
    canonical: String,
    tree_hash: String,
}

#[test]
fn canonical_manifests_and_tree_hashes_match_the_shared_vectors() {
    let manifests: Manifests =
        serde_json::from_slice(&std::fs::read(fixture("manifests.json")).unwrap()).unwrap();
    for case in manifests.cases {
        assert_eq!(
            canonical_manifest(&case.entries),
            case.canonical,
            "{}",
            case.name
        );
        assert_eq!(tree_hash(&case.entries), case.tree_hash, "{}", case.name);
    }
}
