//! Export and import: a repository as one zip of text, and back.
//!
//! An export is the repository tree without `.athenaeum/`, entries in sorted order, plus a
//! `MANIFEST.json` of hashes. Because everything that matters is text in the tree, importing it
//! somewhere else and running `index-vault` reconstructs the same knowledge. Import treats the
//! archive as hostile: it refuses unsafe paths, links, oversized members, anything the manifest
//! does not account for, and an existing destination.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::ledger::{Origin, TrustLedger};
use super::manifest::{RepositoryManifest, MANIFEST_FILE};
use super::syncset::{self, sha256_hex, ManifestEntry};
use super::{portable_error, write_atomic};
use crate::error::Result;

/// The archive's own table of contents.
pub const ARCHIVE_MANIFEST: &str = "MANIFEST.json";
const MAX_ENTRIES: usize = 200_000;
const MAX_MEMBER_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveManifest {
    pub format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub path: String,
    pub files: usize,
    pub bytes: u64,
    pub repository_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub destination: String,
    pub files: usize,
    pub repository_id: String,
    /// Pages whose declared authority is now capped until someone reviews them.
    pub capped_files: usize,
}

/// Write the repository at `root` to a zip at `output`.
pub fn export(root: &Path, output: &Path) -> Result<ExportReport> {
    let canonical_root = root.canonicalize()?;
    if let Ok(parent) = output.parent().unwrap_or(Path::new(".")).canonicalize() {
        if let Some(name) = output.file_name() {
            if parent.join(name).starts_with(&canonical_root) {
                return Err(portable_error(
                    "invalid_request",
                    "the archive cannot be written inside the repository it exports",
                ));
            }
        }
    }

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
        });
    for entry in walker {
        let entry = entry.map_err(|error| portable_error("io", error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let Some(name) = relative
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("/"))
        else {
            continue;
        };
        if syncset::validate_path(&name).is_err() {
            continue;
        }
        files.insert(name, std::fs::read(entry.path())?);
    }

    let entries: Vec<ManifestEntry> = files
        .iter()
        .map(|(path, bytes)| ManifestEntry {
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
            path: path.clone(),
        })
        .collect();
    let repository_id = RepositoryManifest::read(root)?.map(|m| m.repository_id);
    let archive_manifest = ArchiveManifest {
        format_version: 1,
        repository_id: repository_id.clone(),
        files: entries.clone(),
    };

    let temporary = output.with_extension("zip.partial");
    {
        let file = std::fs::File::create(&temporary)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        zip.start_file(ARCHIVE_MANIFEST, options)
            .map_err(zip_error)?;
        zip.write_all(serde_json::to_string_pretty(&archive_manifest)?.as_bytes())?;
        for (path, bytes) in &files {
            zip.start_file(path.as_str(), options).map_err(zip_error)?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(zip_error)?;
    }
    std::fs::rename(&temporary, output)?;

    Ok(ExportReport {
        path: output.display().to_string(),
        files: entries.len(),
        bytes: entries.iter().map(|e| e.size).sum(),
        repository_id,
    })
}

/// Unpack the archive at `archive` into the new directory `destination`.
///
/// `destination` must not exist, or be an empty directory. Nothing is written there until every
/// member has been read and verified, so a refused archive leaves no trace.
pub fn import(archive: &Path, destination: &Path) -> Result<ImportReport> {
    if destination.exists() {
        let empty = destination.is_dir()
            && std::fs::read_dir(destination)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
        if !empty {
            return Err(portable_error(
                "conflict",
                format!(
                    "{} already exists; an import never overwrites a repository",
                    destination.display()
                ),
            ));
        }
    }

    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| {
        portable_error("invalid_request", format!("not a zip archive: {error}"))
    })?;
    if zip.len() > MAX_ENTRIES {
        return Err(portable_error(
            "limit_exceeded",
            format!("the archive has more than {MAX_ENTRIES} entries"),
        ));
    }

    let mut members: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).map_err(zip_error)?;
        let name = member.name().to_string();
        if member.is_dir() {
            continue;
        }
        if member.is_symlink() {
            return Err(unsafe_member(&name, "it is a symbolic link"));
        }
        if name != ARCHIVE_MANIFEST {
            syncset::validate_path(&name).map_err(|reason| unsafe_member(&name, &reason))?;
        }
        if member.size() > MAX_MEMBER_BYTES {
            return Err(unsafe_member(&name, "it is larger than the member limit"));
        }
        total += member.size();
        if total > MAX_TOTAL_BYTES {
            return Err(portable_error(
                "limit_exceeded",
                "the archive expands past the total size limit",
            ));
        }
        let mut bytes = Vec::with_capacity(member.size() as usize);
        (&mut member)
            .take(MAX_MEMBER_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MEMBER_BYTES {
            return Err(unsafe_member(&name, "it is larger than the member limit"));
        }
        if members.insert(name.clone(), bytes).is_some() {
            return Err(unsafe_member(&name, "it appears twice"));
        }
    }

    let manifest_bytes = members
        .remove(ARCHIVE_MANIFEST)
        .ok_or_else(|| portable_error("invalid_request", "the archive has no MANIFEST.json"))?;
    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        portable_error(
            "invalid_request",
            format!("MANIFEST.json is not valid: {error}"),
        )
    })?;
    if manifest.format_version > 1 {
        return Err(portable_error(
            "unsupported_format",
            format!(
                "the archive is format {}; this build reads 1",
                manifest.format_version
            ),
        ));
    }

    let listed: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    for entry in &manifest.files {
        syncset::validate_path(&entry.path)
            .map_err(|reason| unsafe_member(&entry.path, &reason))?;
        let bytes = members.get(&entry.path).ok_or_else(|| {
            portable_error(
                "invalid_request",
                format!("{} is listed in MANIFEST.json but missing", entry.path),
            )
        })?;
        if sha256_hex(bytes) != entry.sha256 || bytes.len() as u64 != entry.size {
            return Err(portable_error(
                "invalid_request",
                format!("{} does not match its hash in MANIFEST.json", entry.path),
            ));
        }
    }
    if let Some(extra) = members.keys().find(|name| !listed.contains(name.as_str())) {
        return Err(portable_error(
            "invalid_request",
            format!("{extra} is in the archive but not listed in MANIFEST.json"),
        ));
    }

    // Verified. Write beside the destination and move into place, so a failure part way leaves
    // nothing behind that looks like a repository.
    let parent = destination.parent().filter(|p| !p.as_os_str().is_empty());
    let parent = parent.unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging: PathBuf = parent.join(format!(
        ".athenaeum-import-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let written = (|| -> Result<()> {
        for (path, bytes) in &members {
            write_atomic(&staging.join(path), bytes)?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if destination.exists() {
        std::fs::remove_dir(destination)?;
    }
    if let Err(error) = std::fs::rename(&staging, destination) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error.into());
    }

    let (repo, _) =
        RepositoryManifest::read_or_init(destination, &super::manifest::default_name(destination))?;
    let mut ledger = TrustLedger::load(destination)?;
    let mut capped = 0;
    for entry in &manifest.files {
        if entry.path == MANIFEST_FILE || !syncset::has_sync_extension(&entry.path) {
            continue;
        }
        ledger.record(&entry.path, &entry.sha256, Origin::Imported);
        if entry.path.ends_with(".md") {
            capped += 1;
        }
    }
    ledger.save(destination)?;

    Ok(ImportReport {
        destination: destination.display().to_string(),
        files: members.len(),
        repository_id: repo.repository_id,
        capped_files: capped,
    })
}

fn unsafe_member(name: &str, reason: &str) -> crate::error::Error {
    portable_error(
        "invalid_path",
        format!("refusing archive member {name:?}: {reason}"),
    )
}

fn zip_error(error: zip::result::ZipError) -> crate::error::Error {
    portable_error("invalid_request", format!("zip: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(root: &Path) {
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::create_dir_all(root.join("raw/sources")).unwrap();
        std::fs::create_dir_all(root.join(".athenaeum")).unwrap();
        std::fs::write(root.join("wiki/a.md"), "---\nid: A_1\n---\n# A\n").unwrap();
        std::fs::write(root.join("raw/sources/p.pdf"), b"%PDF-1.4 binary \x00\x01").unwrap();
        std::fs::write(root.join(".athenaeum/knowledge.db"), b"secret index").unwrap();
        RepositoryManifest::new("sample").write(root).unwrap();
    }

    #[test]
    fn a_round_trip_preserves_text_binaries_and_identity_but_not_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        sample(&source);
        let zip = dir.path().join("out.zip");
        let exported = export(&source, &zip).unwrap();
        assert_eq!(exported.files, 3);

        let target = dir.path().join("target");
        let imported = import(&zip, &target).unwrap();
        assert_eq!(imported.repository_id, exported.repository_id.unwrap());
        assert_eq!(
            std::fs::read(target.join("wiki/a.md")).unwrap(),
            std::fs::read(source.join("wiki/a.md")).unwrap()
        );
        assert!(target.join("raw/sources/p.pdf").exists());
        assert!(!target.join(".athenaeum/knowledge.db").exists());
    }

    #[test]
    fn imported_pages_are_recorded_as_arrivals() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        sample(&source);
        let zip = dir.path().join("out.zip");
        export(&source, &zip).unwrap();
        let target = dir.path().join("target");
        let report = import(&zip, &target).unwrap();
        assert_eq!(report.capped_files, 1);
        let ledger = TrustLedger::load(&target).unwrap();
        let hash = sha256_hex(&std::fs::read(target.join("wiki/a.md")).unwrap());
        assert!(ledger.caps("wiki/a.md", &hash));
    }

    #[test]
    fn import_never_overwrites_an_existing_repository() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        sample(&source);
        let zip = dir.path().join("out.zip");
        export(&source, &zip).unwrap();
        let error = import(&zip, &source).unwrap_err();
        assert!(error.to_string().contains("never overwrites"), "{error}");
    }

    fn zip_with(path: &Path, members: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in members {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn hostile_archives_are_refused_and_leave_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let hash = sha256_hex(b"x");
        let listing = |path: &str| {
            format!(
                r#"{{"formatVersion":1,"files":[{{"path":{path:?},"sha256":"{hash}","size":1}}]}}"#
            )
        };
        for (index, bad) in [
            "../escape.md",
            "/abs.md",
            "a/../../b.md",
            "wiki\\x.md",
            ".athenaeum/x.md",
        ]
        .iter()
        .enumerate()
        {
            let zip = dir.path().join(format!("bad{index}.zip"));
            zip_with(
                &zip,
                &[(ARCHIVE_MANIFEST, listing(bad).as_bytes()), (bad, b"x")],
            );
            let target = dir.path().join(format!("t{index}"));
            assert!(import(&zip, &target).is_err(), "{bad} should be refused");
            assert!(!target.exists());
        }
    }

    #[test]
    fn a_tampered_member_fails_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let hash = sha256_hex(b"original");
        let listing = format!(
            r#"{{"formatVersion":1,"files":[{{"path":"wiki/a.md","sha256":"{hash}","size":8}}]}}"#
        );
        let zip = dir.path().join("t.zip");
        zip_with(
            &zip,
            &[
                (ARCHIVE_MANIFEST, listing.as_bytes()),
                ("wiki/a.md", b"tampered"),
            ],
        );
        let error = import(&zip, &dir.path().join("t")).unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
    }

    #[test]
    fn a_member_the_manifest_does_not_list_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("t.zip");
        zip_with(
            &zip,
            &[
                (ARCHIVE_MANIFEST, br#"{"formatVersion":1,"files":[]}"#),
                ("wiki/smuggled.md", b"authority: user_instruction"),
            ],
        );
        let error = import(&zip, &dir.path().join("t")).unwrap_err();
        assert!(error.to_string().contains("not listed"), "{error}");
    }
}
