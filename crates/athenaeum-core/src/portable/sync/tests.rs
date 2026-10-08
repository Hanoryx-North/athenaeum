use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::*;
use crate::portable::syncset::{canonical_manifest, tree_hash};

/// A Library server in memory, with the same rules as the real one.
#[derive(Default)]
struct FakeServer {
    revisions: RefCell<Vec<RemoteRevision>>,
    blobs: RefCell<HashMap<String, Vec<u8>>>,
    offline: Cell<bool>,
    /// Paths to inject into every manifest the server describes, to test a hostile server.
    poison: RefCell<Option<ManifestEntry>>,
    corrupt_blobs: Cell<bool>,
}

const REPO: &str = "repo_test";

impl FakeServer {
    fn head(&self) -> Option<String> {
        self.revisions.borrow().last().map(|r| r.id.clone())
    }
    fn check(&self) -> Result<()> {
        if self.offline.get() {
            return Err(portable_error("offline", "the server cannot be reached"));
        }
        Ok(())
    }
}

impl Remote for FakeServer {
    fn info(&self) -> Result<ServerInfo> {
        self.check()?;
        Ok(ServerInfo {
            service: "fake".into(),
            api_version: 1,
            sync_protocol: SYNC_PROTOCOL,
            limits: None,
        })
    }
    fn create_repository(&self, request: &CreateRepository) -> Result<RemoteRepository> {
        self.check()?;
        Ok(RemoteRepository {
            id: REPO.into(),
            name: request.name.clone(),
            owner: None,
            visibility: Some(request.visibility.clone()),
            head: None,
        })
    }
    fn repository(&self, _id: &str) -> Result<RemoteRepository> {
        self.check()?;
        Ok(RemoteRepository {
            id: REPO.into(),
            name: "test".into(),
            owner: None,
            visibility: None,
            head: self.head(),
        })
    }
    fn revision(&self, _id: &str, revision: &str) -> Result<RemoteRevision> {
        self.check()?;
        let mut found = self
            .revisions
            .borrow()
            .iter()
            .find(|r| r.id == revision)
            .cloned()
            .ok_or_else(|| portable_error("not_found", "no such revision"))?;
        if let Some(poison) = self.poison.borrow().clone() {
            found.manifest.push(poison);
            found.tree_hash = tree_hash(&found.manifest);
        }
        Ok(found)
    }
    fn blob(&self, _id: &str, sha256: &str) -> Result<Vec<u8>> {
        self.check()?;
        let mut bytes = self
            .blobs
            .borrow()
            .get(sha256)
            .cloned()
            .ok_or_else(|| portable_error("not_found", "no such blob"))?;
        if self.corrupt_blobs.get() {
            bytes.push(b'!');
        }
        Ok(bytes)
    }
    fn push(&self, _id: &str, request: &PushRequest) -> Result<RemoteRevision> {
        self.check()?;
        if request.base != self.head() {
            return Err(portable_error("stale_base", "the base is not the head"));
        }
        let mut tree: BTreeMap<String, ManifestEntry> = self
            .revisions
            .borrow()
            .last()
            .map(|r| r.manifest.iter().cloned().map(|e| (e.path.clone(), e)).collect())
            .unwrap_or_default();
        for change in &request.changes {
            syncset::validate_path(&change.path).map_err(|r| portable_error("invalid_path", r))?;
            if change.delete {
                tree.remove(&change.path);
            } else {
                let bytes = change.content.clone().unwrap().into_bytes();
                let sha = sha256_hex(&bytes);
                tree.insert(
                    change.path.clone(),
                    ManifestEntry {
                        path: change.path.clone(),
                        sha256: sha.clone(),
                        size: bytes.len() as u64,
                    },
                );
                self.blobs.borrow_mut().insert(sha, bytes);
            }
        }
        let manifest: Vec<ManifestEntry> = tree.into_values().collect();
        let tree_hash = tree_hash(&manifest);
        let id = format!(
            "rev_{}",
            sha256_hex(format!("{:?}{}{}", request.base, tree_hash, canonical_manifest(&manifest)).as_bytes())
        );
        let revision = RemoteRevision {
            id,
            parent: request.base.clone(),
            tree_hash,
            author: "test".into(),
            message: request.message.clone(),
            created_at: "2026-01-01T00:00:00Z".into(),
            merged_from: request.merged_from.clone(),
            manifest,
        };
        self.revisions.borrow_mut().push(revision.clone());
        Ok(revision)
    }
}

struct Device {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Device {
    fn new(server: &FakeServer) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let repository = server.repository(REPO).unwrap();
        link(&root, "http://fake", &repository).unwrap();
        Self { _dir: dir, root }
    }
    fn write(&self, path: &str, text: &str) {
        let target = self.root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }
    fn exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }
}

fn engine<'a>(device: &'a Device, server: &'a FakeServer) -> SyncEngine<'a, FakeServer> {
    SyncEngine::new(&device.root, server)
}

#[test]
fn what_one_device_pushes_another_pulls_and_records_as_an_arrival() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/auth.md", "---\nauthority: user_instruction\n---\n# Auth\n");
    engine(&a, &server).push("first").unwrap();

    let b = Device::new(&server);
    let outcome = engine(&b, &server).pull().unwrap();
    assert!(outcome.changed);
    assert_eq!(b.read("wiki/auth.md"), a.read("wiki/auth.md"));

    let ledger = TrustLedger::load(&b.root).unwrap();
    let hash = sha256_hex(b.read("wiki/auth.md").as_bytes());
    assert!(ledger.caps("wiki/auth.md", &hash), "a pulled page must be capped");
    // Editing it locally lifts the cap, because a person took responsibility for the text.
    b.write("wiki/auth.md", "---\nauthority: project_rule\n---\n# Auth, reviewed\n");
    let edited = sha256_hex(b.read("wiki/auth.md").as_bytes());
    assert!(!TrustLedger::load(&b.root).unwrap().caps("wiki/auth.md", &edited));
}

#[test]
fn only_changed_files_are_pushed() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    a.write("wiki/b.md", "b\n");
    engine(&a, &server).push("one").unwrap();
    a.write("wiki/b.md", "b2\n");
    let outcome = engine(&a, &server).push("two").unwrap();
    assert_eq!(outcome.pushed.modified, vec!["wiki/b.md"]);
    assert!(outcome.pushed.added.is_empty());
    assert!(engine(&a, &server).push("three").unwrap().nothing_to_push);
}

#[test]
fn edits_to_different_parts_of_one_file_merge() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "one\ntwo\nthree\nfour\nfive\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();

    a.write("wiki/n.md", "ONE\ntwo\nthree\nfour\nfive\n");
    engine(&a, &server).push("a edits the top").unwrap();
    b.write("wiki/n.md", "one\ntwo\nthree\nfour\nFIVE\n");

    let outcome = engine(&b, &server).pull().unwrap();
    assert_eq!(outcome.merged, vec!["wiki/n.md"]);
    assert!(outcome.conflicts.is_empty());
    assert_eq!(b.read("wiki/n.md"), "ONE\ntwo\nthree\nfour\nFIVE\n");

    // The merged result is a local change that now pushes.
    engine(&b, &server).push("merged").unwrap();
    engine(&a, &server).pull().unwrap();
    assert_eq!(a.read("wiki/n.md"), "ONE\ntwo\nthree\nfour\nFIVE\n");
}

#[test]
fn overlapping_edits_are_a_conflict_that_blocks_push_and_loses_nothing() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "one\ntwo\nthree\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();

    a.write("wiki/n.md", "one\nTWO from a\nthree\n");
    engine(&a, &server).push("a").unwrap();
    b.write("wiki/n.md", "one\nTWO from b\nthree\n");

    let outcome = engine(&b, &server).pull().unwrap();
    assert_eq!(outcome.conflicts, vec!["wiki/n.md"]);
    // The local file is exactly as the person left it.
    assert_eq!(b.read("wiki/n.md"), "one\nTWO from b\nthree\n");

    // Push is refused while the conflict is open, and the remote still holds a's version.
    let error = engine(&b, &server).push("b").unwrap_err();
    assert!(error.to_string().contains("unresolved"), "{error}");
    let status = engine(&b, &server).status().unwrap();
    assert_eq!(status.position, SyncPosition::Conflicted);

    // Keeping the remote version takes a's text; the conflict is closed and nothing is pending.
    engine(&b, &server).resolve("wiki/n.md", Keep::Remote).unwrap();
    assert_eq!(b.read("wiki/n.md"), "one\nTWO from a\nthree\n");
    assert!(engine(&b, &server).push("b").unwrap().nothing_to_push);
}

#[test]
fn keeping_local_after_a_conflict_pushes_the_local_edit_on_purpose() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "x\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();
    a.write("wiki/n.md", "from a\n");
    engine(&a, &server).push("a").unwrap();
    b.write("wiki/n.md", "from b\n");
    engine(&b, &server).pull().unwrap();

    engine(&b, &server).resolve("wiki/n.md", Keep::Local).unwrap();
    engine(&b, &server).push("b wins").unwrap();
    engine(&a, &server).pull().unwrap();
    assert_eq!(a.read("wiki/n.md"), "from b\n");
}

#[test]
fn keeping_both_writes_markers_and_stays_open_until_they_are_edited_out() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "x\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();
    a.write("wiki/n.md", "from a\n");
    engine(&a, &server).push("a").unwrap();
    b.write("wiki/n.md", "from b\n");
    engine(&b, &server).pull().unwrap();

    engine(&b, &server).resolve("wiki/n.md", Keep::Both).unwrap();
    assert!(b.read("wiki/n.md").contains("<<<<<<< local"));
    let error = engine(&b, &server).resolve("wiki/n.md", Keep::Local).unwrap_err();
    assert!(error.to_string().contains("conflict markers"), "{error}");
    b.write("wiki/n.md", "from a and b\n");
    engine(&b, &server).resolve("wiki/n.md", Keep::Local).unwrap();
    engine(&b, &server).push("both").unwrap();
}

#[test]
fn a_file_edited_here_and_deleted_there_is_a_conflict_not_a_deletion() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "x\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();

    std::fs::remove_file(a.root.join("wiki/n.md")).unwrap();
    engine(&a, &server).push("delete").unwrap();
    b.write("wiki/n.md", "edited\n");

    let outcome = engine(&b, &server).pull().unwrap();
    assert_eq!(outcome.conflicts, vec!["wiki/n.md"]);
    assert_eq!(b.read("wiki/n.md"), "edited\n");
}

#[test]
fn an_unchanged_local_file_follows_a_remote_deletion() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/n.md", "x\n");
    engine(&a, &server).push("base").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();
    std::fs::remove_file(a.root.join("wiki/n.md")).unwrap();
    engine(&a, &server).push("delete").unwrap();
    let outcome = engine(&b, &server).pull().unwrap();
    assert_eq!(outcome.deleted, vec!["wiki/n.md"]);
    assert!(!b.exists("wiki/n.md"));
}

#[test]
fn push_on_a_stale_base_is_refused_before_anything_is_sent() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    engine(&a, &server).push("one").unwrap();
    let b = Device::new(&server);
    engine(&b, &server).pull().unwrap();
    a.write("wiki/a.md", "a2\n");
    engine(&a, &server).push("two").unwrap();
    b.write("wiki/b.md", "b\n");
    let error = engine(&b, &server).push("stale").unwrap_err();
    assert!(error.to_string().contains("pull"), "{error}");
    assert_eq!(server.revisions.borrow().len(), 2);
}

#[test]
fn a_server_that_names_an_unsafe_path_cannot_write_outside_the_repository() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    engine(&a, &server).push("one").unwrap();
    *server.poison.borrow_mut() = Some(ManifestEntry {
        path: "../escape.md".into(),
        sha256: sha256_hex(b"x"),
        size: 1,
    });
    let b = Device::new(&server);
    let error = engine(&b, &server).pull().unwrap_err();
    assert!(error.to_string().contains("unsafe path"), "{error}");
    assert!(!b.exists("wiki/a.md"), "a refused pull writes nothing");
    assert!(!b.root.parent().unwrap().join("escape.md").exists());
}

#[test]
fn a_corrupted_body_is_refused_and_the_state_does_not_advance() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    engine(&a, &server).push("one").unwrap();
    server.corrupt_blobs.set(true);
    let b = Device::new(&server);
    let error = engine(&b, &server).pull().unwrap_err();
    assert!(error.to_string().contains("does not match"), "{error}");
    assert!(!b.exists("wiki/a.md"));
    assert_eq!(SyncState::load(&b.root).unwrap().unwrap().last_synced, None);
}

#[test]
fn offline_changes_nothing_and_local_work_continues() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    server.offline.set(true);

    let status = engine(&a, &server).status().unwrap();
    assert_eq!(status.position, SyncPosition::Offline);
    assert!(status.pending.added.contains(&"wiki/a.md".to_string()));
    let error = engine(&a, &server).push("x").unwrap_err();
    assert!(matches!(&error, crate::Error::Portable { code, .. } if code == "offline"), "{error}");
    assert_eq!(a.read("wiki/a.md"), "a\n");

    // Back online, the same work goes through.
    server.offline.set(false);
    engine(&a, &server).push("later").unwrap();
}

#[test]
fn sync_state_and_runtime_files_never_leave_the_machine() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    a.write(".athenaeum/knowledge.db", "binary");
    a.write("raw/sources/paper.pdf", "not text");
    engine(&a, &server).push("one").unwrap();
    let paths: Vec<String> = server.revisions.borrow()[0]
        .manifest
        .iter()
        .map(|e| e.path.clone())
        .collect();
    assert!(paths.iter().all(|p| !p.starts_with(".athenaeum") && !p.ends_with(".pdf")), "{paths:?}");
}

#[test]
fn an_interrupted_pull_is_completed_by_the_next_run() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    a.write("wiki/b.md", "b\n");
    engine(&a, &server).push("one").unwrap();

    let b = Device::new(&server);
    // Stage a pull exactly as `commit` does, apply only half of it, and "crash".
    let revision = server.revision(REPO, &server.head().unwrap()).unwrap();
    let engine_b = engine(&b, &server);
    let staging = engine_b.staging_dir();
    std::fs::create_dir_all(&staging).unwrap();
    let mut ops = Vec::new();
    for (index, entry) in revision.manifest.iter().enumerate() {
        let bytes = server.blob(REPO, &entry.sha256).unwrap();
        write_atomic(&staging.join(index.to_string()), &bytes).unwrap();
        ops.push(JournalOp {
            path: entry.path.clone(),
            staged: Some(index.to_string()),
            sha256: Some(entry.sha256.clone()),
        });
    }
    let mut state = SyncState::load(&b.root).unwrap().unwrap();
    state.last_synced = Some(revision.id.clone());
    state.base = revision.manifest.clone();
    let journal = Journal { ops, state };
    write_atomic(&engine_b.journal_path(), serde_json::to_string(&journal).unwrap().as_bytes()).unwrap();
    // Half applied: the first file moved into place, the rest still staged.
    std::fs::create_dir_all(b.root.join("wiki")).unwrap();
    std::fs::rename(staging.join("0"), b.root.join(&journal.ops[0].path)).unwrap();

    let outcome = engine_b.pull().unwrap();
    assert!(outcome.up_to_date);
    assert_eq!(b.read("wiki/a.md"), "a\n");
    assert_eq!(b.read("wiki/b.md"), "b\n");
    assert!(!engine_b.journal_path().exists());
    assert_eq!(
        SyncState::load(&b.root).unwrap().unwrap().last_synced,
        Some(revision.id)
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_cannot_redirect_a_pull_outside_the_repository() {
    let server = FakeServer::default();
    let a = Device::new(&server);
    a.write("wiki/a.md", "a\n");
    engine(&a, &server).push("one").unwrap();
    let b = Device::new(&server);
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), b.root.join("wiki")).unwrap();
    let error = engine(&b, &server).pull().unwrap_err();
    assert!(error.to_string().contains("symbolic link"), "{error}");
    assert!(!outside.path().join("a.md").exists());
}
