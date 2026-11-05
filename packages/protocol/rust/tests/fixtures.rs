//! Loads the shared fixtures (`../fixtures`, the same files the TypeScript tests read) and proves the
//! Rust definitions agree with them: vectors give the same answers, and every payload deserializes
//! and serializes back to exactly what is on disk. A field added to a fixture but not to the Rust
//! type, or the reverse, fails here.

use std::fs;
use std::path::PathBuf;

use athenaeum_protocol::bridge::{
    BridgeOutcome, BridgeRequestEnvelope, BridgeResponseEnvelope, BRIDGE_ERROR_CODES,
    BRIDGE_INDEX_SCOPES, BRIDGE_METHODS, BRIDGE_REVIEW_KINDS, BRIDGE_REVIEW_SEVERITIES,
    BRIDGE_SOURCE_SCOPES,
};
use athenaeum_protocol::library::{self, *};
use athenaeum_protocol::sync::{self, *};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

fn fixture(relative: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures")
        .join(relative);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strip_nulls(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), strip_nulls(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_nulls).collect()),
        other => other.clone(),
    }
}

/// Deserialize, serialize, and require the exact JSON back.
fn roundtrip<T: DeserializeOwned + Serialize>(label: &str, value: &Value) -> T {
    let typed: T = serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{label}: {e}"));
    let back = serde_json::to_value(&typed).unwrap();
    assert_eq!(&back, value, "{label} does not round-trip");
    typed
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn as_strs(slice: &[&str]) -> Vec<String> {
    slice.iter().map(|s| s.to_string()).collect()
}

// --- a small SHA-256, so the crate needs no hashing dependency to verify tree hashes --------------

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(add);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[test]
fn the_test_sha256_matches_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let long = vec![b'a'; 1000];
    assert_eq!(
        sha256_hex(&long),
        "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
    );
}

// --- sync vectors ----------------------------------------------------------------------------------

#[test]
fn path_vectors_agree_with_typescript() {
    let paths = fixture("sync/v1/paths.json");
    for path in paths["valid"].as_array().unwrap() {
        let path = path.as_str().unwrap();
        assert_eq!(validate_path(path), Ok(()), "{path:?}");
        assert!(is_sync_path(path), "{path:?}");
    }
    for case in paths["invalid"].as_array().unwrap() {
        let path = case["path"].as_str().unwrap();
        assert!(validate_path(path).is_err(), "{path:?} ({})", case["why"]);
        assert!(!is_sync_path(path), "{path:?}");
    }
}

#[test]
fn extension_vectors_agree_with_typescript() {
    let extensions = fixture("sync/v1/extensions.json");
    for path in extensions["allowed"].as_array().unwrap() {
        assert!(is_sync_path(path.as_str().unwrap()), "{path}");
    }
    for case in extensions["refused"].as_array().unwrap() {
        assert!(
            !is_sync_path(case["path"].as_str().unwrap()),
            "{}",
            case["path"]
        );
    }
}

#[test]
fn canonical_manifests_and_tree_hashes_agree_with_typescript() {
    let manifests = fixture("sync/v1/manifests.json");
    for case in manifests["cases"].as_array().unwrap() {
        let entries: Vec<ManifestEntry> = serde_json::from_value(case["entries"].clone()).unwrap();
        let canonical = canonical_manifest(&entries);
        assert_eq!(
            canonical,
            case["canonical"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(
            sha256_hex(canonical.as_bytes()),
            case["treeHash"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn canonical_revisions_and_revision_ids_agree_with_typescript() {
    let revisions = fixture("sync/v1/revisions.json");
    for case in revisions["cases"].as_array().unwrap() {
        let f = &case["fields"];
        let text = |key: &str| f[key].as_str();
        let canonical = canonical_revision(
            text("parent"),
            text("treeHash").unwrap(),
            text("author").unwrap(),
            text("message").unwrap(),
            text("createdAt").unwrap(),
            text("mergedFrom"),
        );
        assert_eq!(
            canonical,
            case["canonical"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        let id = format!("rev_{}", sha256_hex(canonical.as_bytes()));
        assert_eq!(id, case["revisionId"].as_str().unwrap(), "{}", case["name"]);
        assert_eq!(library::validate_revision_id(&id), Ok(()));
    }
    for bad in [
        "",
        "rev_",
        "rev_ABCDEF",
        &format!("rev_{}", "A".repeat(64)),
        &format!("rev_{}", "a".repeat(63)),
        &format!("abc_{}", "a".repeat(64)),
    ] {
        assert!(library::validate_revision_id(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn wire_fixtures_round_trip() {
    let wire = fixture("sync/v1/wire.json");
    let info: ServerInfo = roundtrip("serverInfo", &wire["serverInfo"]);
    assert!(info.compatible());
    assert_eq!(info.limits, Some(SyncLimits::default()));

    let manifest: RepositoryManifest = roundtrip("manifest", &wire["manifest"]);
    assert!(manifest.format_supported());
    assert_eq!(validate_repository_id(&manifest.repository_id), Ok(()));

    let create: CreateRepository = roundtrip("createRepository", &wire["createRepository"]);
    assert_eq!(create.id.as_deref(), Some("repo_example"));
    let without_id: CreateRepository = roundtrip(
        "createRepositoryWithoutId",
        &wire["createRepositoryWithoutId"],
    );
    assert_eq!(without_id.id, None);
    assert!(
        !serde_json::to_string(&without_id)
            .unwrap()
            .contains("\"id\""),
        "an absent id must be omitted, never null"
    );

    for id in strings(&wire["repositoryIds"]["valid"]) {
        assert_eq!(validate_repository_id(&id), Ok(()), "{id}");
    }
    for id in strings(&wire["repositoryIds"]["invalid"]) {
        assert!(validate_repository_id(&id).is_err(), "{id:?}");
    }

    let push: PushRequest = roundtrip("push", &wire["push"]);
    assert_eq!(push.base, None);
    for change in &push.changes {
        assert!(is_sync_path(&change.path), "{}", change.path);
        match (&change.content, change.delete) {
            (Some(content), false) => assert_eq!(
                validate_content(content.as_bytes(), SyncLimits::default()),
                Ok(())
            ),
            (None, true) => {}
            other => panic!("a change is a write or a delete, got {other:?}"),
        }
    }

    let stale: ApiError = roundtrip("staleBase", &wire["staleBase"]);
    assert_eq!(stale.error.code, "stale_base");
    assert!(SYNC_ERROR_CODES.contains(&stale.error.code.as_str()));
    assert_eq!(stale.error.details.as_ref().unwrap()["head"], "rev_new");
}

#[test]
fn path_builders_match_the_route_fixture() {
    let routes = fixture("library/v1/routes.json");
    let mut checked = 0;
    for case in routes["sync"].as_array().unwrap() {
        let args = strings(&Value::Array(
            case["args"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a.is_string())
                .cloned()
                .collect(),
        ));
        let expected = case["path"].as_str().unwrap();
        // Only the builders this crate has, and only for ids that need no percent-encoding.
        let actual = match (case["route"].as_str().unwrap(), args.as_slice()) {
            ("repositories", []) => repositories_path().to_string(),
            ("repository", [id]) if !id.contains('/') => repository_path(id),
            ("revision", [id, rev]) => revision_path(id, rev),
            ("blob", [id, sha]) => blob_path(id, sha),
            ("push", [id]) => push_path(id),
            _ => continue,
        };
        assert_eq!(actual, expected);
        checked += 1;
    }
    assert!(
        checked >= 5,
        "expected the five Rust path builders to be exercised, got {checked}"
    );
}

// --- vocabularies ----------------------------------------------------------------------------------

#[test]
fn vocabularies_match_the_shared_files() {
    let bridge = fixture("bridge/v1/vocabulary.json");
    assert_eq!(
        bridge["protocolVersion"],
        athenaeum_protocol::BRIDGE_PROTOCOL_VERSION
    );
    assert_eq!(strings(&bridge["methods"]), as_strs(BRIDGE_METHODS));
    assert_eq!(strings(&bridge["errorCodes"]), as_strs(BRIDGE_ERROR_CODES));
    assert_eq!(
        strings(&bridge["reviewKinds"]),
        as_strs(BRIDGE_REVIEW_KINDS)
    );
    assert_eq!(
        strings(&bridge["reviewSeverities"]),
        as_strs(BRIDGE_REVIEW_SEVERITIES)
    );
    assert_eq!(
        strings(&bridge["indexScopes"]),
        as_strs(BRIDGE_INDEX_SCOPES)
    );
    assert_eq!(
        strings(&bridge["sourceScopes"]),
        as_strs(BRIDGE_SOURCE_SCOPES)
    );

    let vocabulary = fixture("library/v1/vocabulary.json");
    assert_eq!(strings(&vocabulary["tokenScopes"]), as_strs(TOKEN_SCOPES));
    assert_eq!(vocabulary["tokenSecretPrefix"], TOKEN_SECRET_PREFIX);
    assert_eq!(
        strings(&vocabulary["repositoryVisibilities"]),
        as_strs(REPOSITORY_VISIBILITIES)
    );
    assert_eq!(
        strings(&vocabulary["repositoryRoles"]),
        as_strs(REPOSITORY_ROLES)
    );
    assert_eq!(
        strings(&vocabulary["collaboratorRoles"]),
        as_strs(COLLABORATOR_ROLES)
    );
    assert_eq!(
        strings(&vocabulary["repositorySorts"]),
        as_strs(REPOSITORY_SORTS)
    );
    assert_eq!(
        strings(&vocabulary["devicePollErrorCodes"]),
        as_strs(DEVICE_POLL_ERROR_CODES)
    );
    assert_eq!(
        strings(&vocabulary["syncErrorCodes"]),
        as_strs(SYNC_ERROR_CODES)
    );
    let statuses = vocabulary["errorStatus"].as_object().unwrap();
    assert_eq!(statuses.len(), LIBRARY_ERROR_STATUS.len());
    for (code, status) in statuses {
        assert_eq!(
            error_status(code),
            Some(status.as_u64().unwrap() as u16),
            "{code}"
        );
    }
}

// --- bridge ----------------------------------------------------------------------------------------

#[test]
fn bridge_requests_round_trip_and_cover_every_method() {
    let mut seen = Vec::new();
    for file in [
        "bridge/v1/requests.json",
        "bridge/v1/portable-requests.json",
    ] {
        for (name, value) in fixture(file).as_object().unwrap() {
            let envelope: BridgeRequestEnvelope =
                serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
            // Optional request fields serialize as null where the fixture omits them; the contract
            // treats null and absent alike, so compare without nulls but never without keys.
            let back = serde_json::to_value(&envelope).unwrap();
            assert_eq!(
                strip_nulls(&back),
                strip_nulls(value),
                "{name} does not round-trip"
            );
            assert_eq!(
                envelope.protocol_version,
                athenaeum_protocol::BRIDGE_PROTOCOL_VERSION
            );
            assert_eq!(envelope.request.method(), name, "fixture key is the method");
            assert!(BRIDGE_METHODS.contains(&envelope.request.method()));
            seen.push(envelope.request.method().to_string());
        }
    }
    seen.sort();
    let mut expected = as_strs(BRIDGE_METHODS);
    expected.sort();
    assert_eq!(seen, expected);
}

#[test]
fn bridge_responses_round_trip_with_known_methods_and_codes() {
    let responses = fixture("bridge/v1/responses.json");
    let mut ok_methods = Vec::new();
    for (name, value) in responses.as_object().unwrap() {
        let envelope: BridgeResponseEnvelope = roundtrip(name, value);
        match &envelope.outcome {
            BridgeOutcome::Ok { response } => {
                assert!(
                    BRIDGE_METHODS.contains(&response.method.as_str()),
                    "{name}: {}",
                    response.method
                );
                ok_methods.push(response.method.clone());
            }
            BridgeOutcome::Error { error } => assert!(
                BRIDGE_ERROR_CODES.contains(&error.code.as_str()),
                "{name}: {}",
                error.code
            ),
        }
    }
    ok_methods.sort();
    ok_methods.dedup();
    let mut expected = as_strs(BRIDGE_METHODS);
    expected.sort();
    assert_eq!(
        ok_methods, expected,
        "every method has at least one ok response fixture"
    );
}

// --- Library API -----------------------------------------------------------------------------------

#[test]
fn library_examples_round_trip() {
    let e = fixture("library/v1/examples.json");

    let info: ApiInfo = roundtrip("info", &e["info"]);
    assert!(info.compatible());
    assert_eq!(info.limits.sync, SyncLimits::default());

    let _: PublicUser = roundtrip("users.publicUser", &e["users"]["publicUser"]);
    for key in [
        "repository",
        "privateRepositoryForkOfPublic",
        "anonymousView",
    ] {
        let repo: RepositoryView = roundtrip(key, &e[key]);
        assert!(REPOSITORY_ROLES.contains(&repo.role.as_str()));
        assert!(REPOSITORY_VISIBILITIES.contains(&repo.visibility.as_str()));
    }
    let page: Page<RepositoryView> = roundtrip("repositoryPage", &e["repositoryPage"]);
    assert_eq!(page.items.len() as u64, page.total);

    let revision: RevisionView = roundtrip("revision", &e["revision"]);
    assert_eq!(library::validate_revision_id(&revision.id), Ok(()));
    assert_eq!(
        sha256_hex(canonical_manifest(revision.manifest.as_ref().unwrap()).as_bytes()),
        revision.tree_hash
    );
    let list: RevisionList = roundtrip("revisionList", &e["revisionList"]);
    assert_eq!(
        list.revisions[0].parent.as_deref(),
        Some(list.revisions[1].id.as_str())
    );
    assert!(
        list.revisions.iter().all(|r| r.manifest.is_none()),
        "the list route omits manifests"
    );

    let _: TreeListing = roundtrip("tree.root", &e["tree"]["root"]);
    let empty: TreeListing = roundtrip("tree.empty", &e["tree"]["empty"]);
    assert_eq!(empty.revision, None);
    let file: FileContent = roundtrip("file", &e["file"]);
    assert_eq!(sha256_hex(file.content.as_bytes()), file.sha256);
    assert_eq!(file.content.len() as u64, file.size);
    let comparison: Comparison = roundtrip("comparison", &e["comparison"]);
    assert_eq!(
        comparison.additions,
        comparison.files.iter().map(|f| f.additions).sum::<u64>()
    );

    let pushed: RevisionView = roundtrip("push.response", &e["push"]["response"]);
    assert_eq!(pushed.manifest.as_ref().unwrap().len(), 1);
    let _: RevisionView = roundtrip("push.responseNoChange", &e["push"]["responseNoChange"]);
    for key in ["request", "requestWithDelete", "mergeRequest"] {
        let push: PushRequest = roundtrip(&format!("push.{key}"), &e["push"][key]);
        assert!(push.changes.iter().all(|c| is_sync_path(&c.path)));
    }

    for (name, envelope) in e["errors"].as_object().unwrap() {
        let error: ApiError = roundtrip(&format!("errors.{name}"), envelope);
        assert!(
            error_status(&error.error.code).is_some(),
            "{name}: {}",
            error.error.code
        );
        assert!(error.request_id.is_some());
    }
    let stale: ApiError = roundtrip("errors.staleBase", &e["errors"]["staleBase"]);
    assert_eq!(error_status(&stale.error.code), Some(409));

    let _: TokenView = roundtrip("tokens.view", &e["tokens"]["view"]);
    let _: TokenView = roundtrip("tokens.unrestricted", &e["tokens"]["unrestricted"]);
    let created: CreatedToken = roundtrip("tokens.created", &e["tokens"]["created"]);
    assert!(created.token.starts_with(TOKEN_SECRET_PREFIX));

    let start: DeviceCode = roundtrip("device.start", &e["device"]["start"]);
    assert!(start
        .verification_uri_complete
        .unwrap()
        .contains(&start.user_code));
    let grant: DeviceTokenGrant = roundtrip("device.grant", &e["device"]["grant"]);
    assert_eq!(grant.token_type, "bearer");
    for (code, envelope) in e["device"]["pollErrors"].as_object().unwrap() {
        let error: ApiError = roundtrip(&format!("device.pollErrors.{code}"), envelope);
        assert_eq!(&error.error.code, code);
        assert!(DEVICE_POLL_ERROR_CODES.contains(&code.as_str()));
        assert_eq!(error_status(code), Some(400));
    }

    let stats: AdminStats = roundtrip("admin.stats", &e["admin"]["stats"]);
    assert_eq!(
        stats.repositories,
        stats.public_repositories + stats.private_repositories
    );
    let users: Page<AdminUser> = roundtrip("admin.users", &e["admin"]["users"]);
    assert_eq!(users.items.len(), 2);
    let repos: Page<AdminRepository> = roundtrip("admin.repositories", &e["admin"]["repositories"]);
    assert_eq!(repos.items.len(), 2);
    let taken_down: AdminRepository =
        roundtrip("admin.takedownResponse", &e["admin"]["takedownResponse"]);
    assert_eq!(taken_down.visibility, "private");
    let refused: ApiError = roundtrip("admin.publishRefused", &e["admin"]["publishRefused"]);
    assert_eq!(refused.error.code, "forbidden");
    assert_eq!(error_status("forbidden"), Some(403));
}

// the unused import guard: `sync` is used through the glob, `library` by path
#[allow(dead_code)]
fn _modules_are_public() {
    let _ = (sync::API_PREFIX, library::API_SERVICE_NAME);
}
