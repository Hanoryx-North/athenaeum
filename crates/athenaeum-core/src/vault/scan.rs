//! Walking a vault directory into a snapshot.
//!
//! The vault is the source of truth, so this pass never writes: it reads
//! `wiki/`, the root governance documents and `raw/sources/`, and hands the
//! result to [`super::index`]. Scanning and indexing are separate so a
//! caller can inspect what was found — or build a snapshot in a test —
//! without a database anywhere near it.

use std::path::Path;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::error::{Error, Result};

use super::page::VaultPage;
use super::{VaultScanOptions, VaultSnapshot, VaultSource};
use crate::status::SkippedFile;

/// The directory holding wiki pages.
pub(super) const WIKI_DIR: &str = "wiki";
/// The directory holding ingested source material.
pub(super) const RAW_SOURCES_DIR: &str = "raw/sources";

/// Root-level documents that are pages even though they sit outside `wiki/`.
const ROOT_PAGES: &[&str] = &["purpose.md", "schema.md"];

/// Vault scaffolding that is deliberately not a node.
///
/// `index.md` is a hand-maintained table of contents whose content the node
/// table *is*, and `log.md` is an activity journal that the event journal
/// *is*. Indexing them would add two hub nodes linking to everything, which
/// buries every real relationship in the graph under them. Both stay in the
/// vault, readable and editable; they simply are not knowledge the index
/// has anything to add to.
const SCAFFOLDING: &[&str] = &["index.md", "log.md"];

pub(super) fn scan(root: &Path, options: VaultScanOptions) -> Result<VaultSnapshot> {
    if !root.is_dir() {
        return Err(Error::InvalidValue {
            field: "vault root",
            value: root.display().to_string(),
        });
    }

    let mut snapshot = VaultSnapshot {
        pages: Vec::new(),
        sources: Vec::new(),
        warnings: Vec::new(),
        complete: true,
        skipped: Vec::new(),
        arrived: Default::default(),
    };

    let started = std::time::Instant::now();
    scan_pages(root, &options, &mut snapshot, started)?;
    scan_sources(root, &options, &mut snapshot, started)?;
    mark_arrivals(root, &mut snapshot);

    // Sorted so two scans of the same vault produce the same snapshot
    // whatever order the filesystem hands entries back in.
    snapshot.pages.sort_by(|a, b| a.path.cmp(&b.path));
    snapshot.sources.sort_by(|a, b| a.identity.cmp(&b.identity));
    Ok(snapshot)
}

/// Record which pages still hold exactly what was pulled or imported into this repository.
///
/// A ledger that cannot be read is reported and treated as empty: the failure must not hide the
/// vault, and an empty ledger is the state of every repository nothing was ever pulled into.
fn mark_arrivals(root: &Path, snapshot: &mut VaultSnapshot) {
    let ledger = match crate::portable::ledger::TrustLedger::load(root) {
        Ok(ledger) => ledger,
        Err(error) => {
            snapshot
                .warnings
                .push(format!("Could not read the trust ledger: {error}"));
            return;
        }
    };
    if ledger.is_empty() {
        return;
    }
    for page in &snapshot.pages {
        if ledger.caps(&page.path, &page.content_hash) {
            snapshot.arrived.insert(page.path.clone());
        }
    }
}

/// A hash of how the vault's files look from the outside: which exist, how long each is and when
/// it was last written.
///
/// It covers exactly the files [`scan`] would read — the root governance pages, `wiki/` minus
/// scaffolding, and `raw/sources/` — and reads none of them, only their metadata. That is the whole
/// point: it answers "has the vault changed since it was indexed?" in a directory walk rather than
/// the full read-and-hash a reconcile costs, which is what lets a reader check on every call.
///
/// It is a heuristic and says so. An edit that leaves both the length and the modification time
/// untouched is invisible to it, and on a filesystem with coarse timestamps two same-length edits
/// inside one tick can look identical. Nanosecond timestamps make that a non-event on the
/// filesystems the app ships on, and a missed edit is repaired by the next explicit index.
pub(super) fn fingerprint(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut feed = |relative: &str, metadata: &std::fs::Metadata| {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_nanos());
        hasher.update(format!("{relative}\0{}\0{modified}\n", metadata.len()));
    };

    for name in ROOT_PAGES {
        if let Ok(metadata) = std::fs::metadata(root.join(name)) {
            if metadata.is_file() {
                feed(name, &metadata);
            }
        }
    }

    // The trust ledger decides pages' effective authority, so a change to it is a change to the
    // index even when no page was touched.
    if let Ok(metadata) = std::fs::metadata(crate::portable::ledger::ledger_path(root)) {
        feed("\0trust-ledger", &metadata);
    }

    let wiki = root.join(WIKI_DIR);
    if wiki.is_dir() {
        for entry in WalkDir::new(&wiki)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| !hidden(entry))
        {
            let entry = entry.map_err(walk_error)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(relative) = relative_path(root, entry.path()) else {
                continue;
            };
            if !relative.to_ascii_lowercase().ends_with(".md") || is_scaffolding(&relative) {
                continue;
            }
            feed(&relative, &entry.metadata().map_err(walk_error)?);
        }
    }

    let sources = root.join("raw").join("sources");
    if sources.is_dir() {
        for entry in WalkDir::new(&sources)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| !hidden(entry))
        {
            let entry = entry.map_err(walk_error)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(identity) = relative_path(&sources, entry.path()) else {
                continue;
            };
            feed(
                &format!("{RAW_SOURCES_DIR}/{identity}"),
                &entry.metadata().map_err(walk_error)?,
            );
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn scan_pages(
    root: &Path,
    options: &VaultScanOptions,
    snapshot: &mut VaultSnapshot,
    started: std::time::Instant,
) -> Result<()> {
    let mut total_bytes = 0_u64;

    for name in ROOT_PAGES {
        let path = root.join(name);
        if path.is_file() {
            read_page(&path, name, options, &mut total_bytes, snapshot, started)?;
        }
    }

    let wiki = root.join(WIKI_DIR);
    if !wiki.is_dir() {
        return Ok(());
    }

    let entries = WalkDir::new(&wiki)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !hidden(entry));

    for entry in entries {
        if budget_spent(options, started, snapshot) {
            break;
        }
        let entry = entry.map_err(walk_error)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(relative) = relative_path(root, entry.path()) else {
            snapshot.warnings.push(format!(
                "Skipped a path that is not valid UTF-8: {}",
                entry.path().display()
            ));
            snapshot.skipped.push(SkippedFile::new(
                entry.path().display().to_string(),
                "not_utf8",
                "the path is not valid UTF-8, so it cannot be named in the index",
            ));
            snapshot.complete = false;
            continue;
        };
        if !relative.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        if is_scaffolding(&relative) {
            continue;
        }
        if snapshot.pages.len() >= options.max_pages {
            snapshot.warnings.push(format!(
                "Stopped after the configured {} page limit",
                options.max_pages
            ));
            snapshot
                .skipped
                .push(SkippedFile::new(&relative, "budget", "the page limit was reached"));
            snapshot.complete = false;
            break;
        }
        if !read_page(
            entry.path(),
            &relative,
            options,
            &mut total_bytes,
            snapshot,
            started,
        )? {
            break;
        }
    }

    Ok(())
}

/// Read one page. Returns false when a budget was exhausted and scanning
/// should stop rather than continue with a partial view.
fn read_page(
    path: &Path,
    relative: &str,
    options: &VaultScanOptions,
    total_bytes: &mut u64,
    snapshot: &mut VaultSnapshot,
    started: std::time::Instant,
) -> Result<bool> {
    if budget_spent(options, started, snapshot) {
        return Ok(false);
    }
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > options.max_page_bytes {
        snapshot.warnings.push(format!(
            "Skipped {relative}: {} bytes exceeds the per-page limit",
            metadata.len()
        ));
        snapshot.skipped.push(SkippedFile::new(
            relative,
            "too_large",
            format!("{} bytes exceeds the per-page limit", metadata.len()),
        ));
        snapshot.complete = false;
        return Ok(true);
    }
    if total_bytes.saturating_add(metadata.len()) > options.max_total_bytes {
        snapshot.warnings.push(format!(
            "Stopped before {relative}: the configured total byte limit was reached"
        ));
        snapshot.skipped.push(SkippedFile::new(
            relative,
            "budget",
            "the total byte budget was reached before this page",
        ));
        snapshot.complete = false;
        return Ok(false);
    }

    let bytes = std::fs::read(path)?;
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(_) => {
            snapshot
                .warnings
                .push(format!("Skipped {relative}: not valid UTF-8"));
            snapshot.skipped.push(SkippedFile::new(
                relative,
                "not_utf8",
                "the page is not valid UTF-8 text",
            ));
            snapshot.complete = false;
            return Ok(true);
        }
    };
    *total_bytes += metadata.len();
    snapshot.pages.push(VaultPage::parse(relative, &content));
    Ok(true)
}

fn scan_sources(
    root: &Path,
    options: &VaultScanOptions,
    snapshot: &mut VaultSnapshot,
    started: std::time::Instant,
) -> Result<()> {
    let sources_root = root.join("raw").join("sources");
    if !sources_root.is_dir() {
        return Ok(());
    }

    let entries = WalkDir::new(&sources_root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !hidden(entry));

    for entry in entries {
        if budget_spent(options, started, snapshot) {
            break;
        }
        let entry = entry.map_err(walk_error)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(identity) = relative_path(&sources_root, entry.path()) else {
            snapshot.warnings.push(format!(
                "Skipped a source path that is not valid UTF-8: {}",
                entry.path().display()
            ));
            snapshot.skipped.push(SkippedFile::new(
                entry.path().display().to_string(),
                "not_utf8",
                "the source path is not valid UTF-8, so it cannot be cited",
            ));
            snapshot.complete = false;
            continue;
        };
        if snapshot.sources.len() >= options.max_sources {
            snapshot.warnings.push(format!(
                "Stopped after the configured {} source limit",
                options.max_sources
            ));
            snapshot.skipped.push(SkippedFile::new(
                format!("{RAW_SOURCES_DIR}/{identity}"),
                "budget",
                "the source limit was reached",
            ));
            snapshot.complete = false;
            break;
        }

        // Only the metadata is read. A source is a 200MB PDF as often as a
        // text file, and re-hashing every one on every index would make
        // reconciling cost as much as rebuilding — which is the thing this
        // whole design exists to avoid.
        let metadata = entry.metadata().map_err(walk_error)?;
        snapshot.sources.push(VaultSource {
            path: format!("{RAW_SOURCES_DIR}/{identity}"),
            identity,
            bytes: metadata.len(),
            mtime_ns: crate::status::modification_time(&metadata),
        });
    }

    Ok(())
}

fn is_scaffolding(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    SCAFFOLDING
        .iter()
        .any(|scaffold| name.eq_ignore_ascii_case(scaffold))
}

/// Vault-relative path with forward slashes, so a knowledge base built on
/// Windows resolves unchanged on macOS and Linux.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut out = String::new();
    for component in relative.components() {
        let part = component.as_os_str().to_str()?;
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    (!out.is_empty()).then_some(out)
}

/// Skip dot-directories: `.obsidian`, `.athenaeum` and `.git` are
/// configuration and derived data, not knowledge.
fn hidden(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
}

/// True when the wall-clock ceiling has been reached, recording why once.
fn budget_spent(
    options: &VaultScanOptions,
    started: std::time::Instant,
    snapshot: &mut VaultSnapshot,
) -> bool {
    if options.max_duration_ms == 0
        || started.elapsed().as_millis() as u64 <= options.max_duration_ms
    {
        return false;
    }
    if snapshot.complete {
        snapshot.warnings.push(format!(
            "Stopped after the configured {} ms time budget",
            options.max_duration_ms
        ));
        snapshot.skipped.push(SkippedFile::new(
            "",
            "budget",
            "the time budget was reached before the vault ended",
        ));
        snapshot.complete = false;
    }
    true
}

fn walk_error(error: walkdir::Error) -> Error {
    match error.into_io_error() {
        Some(io) => Error::Io(io),
        None => Error::InvalidValue {
            field: "vault scan",
            value: "directory traversal failed".to_string(),
        },
    }
}
