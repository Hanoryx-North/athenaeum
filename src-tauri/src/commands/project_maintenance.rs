use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;

fn safe_relative(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}



#[tauri::command]
pub async fn export_project_archive(
    project_path: String,
    destination: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        export_project_archive_inner(project_path, destination)
    })
    .await
    .map_err(|error| format!("Project export task failed: {error}"))?
}

fn resolve_export_destination(root: &Path, output: &Path) -> Result<PathBuf, String> {
    let resolved = if output.exists() {
        output.canonicalize().map_err(|e| e.to_string())?
    } else {
        let parent = output
            .parent()
            .ok_or_else(|| "Export destination must have a parent directory".to_string())?;
        let filename = output
            .file_name()
            .ok_or_else(|| "Export destination must be a file path".to_string())?;
        parent
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join(filename)
    };
    if resolved.starts_with(root) {
        return Err("Export destination must be outside the project directory".into());
    }
    Ok(resolved)
}

fn export_project_archive_inner(project_path: String, destination: String) -> Result<(), String> {
    if !Path::new(&project_path).is_absolute() || !Path::new(&destination).is_absolute() {
        return Err("Project and archive paths must be absolute".into());
    }
    let root = PathBuf::from(&project_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    // Use the same canonical destination that passed containment validation.
    // Reusing the unresolved input would separate the checked path from the
    // path opened for writing and retain avoidable traversal/TOCTOU surface.
    let output = resolve_export_destination(&root, &PathBuf::from(destination))?;
    let file = File::create(&output).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(|error| format!("Failed to enumerate project: {error}"))?;
        if entry.path() == root || entry.file_type().is_symlink() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&root)
            .map_err(|e| e.to_string())?;
        let name = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            zip.add_directory(format!("{name}/"), options)
                .map_err(|e| e.to_string())?;
        } else {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            let mut source = File::open(entry.path()).map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut zip).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn import_project_archive(
    archive_path: String,
    destination: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        import_project_archive_inner(archive_path, destination)
    })
    .await
    .map_err(|error| format!("Project import task failed: {error}"))?
}

fn import_project_archive_inner(
    archive_path: String,
    destination: String,
) -> Result<String, String> {
    if !Path::new(&archive_path).is_absolute() || !Path::new(&destination).is_absolute() {
        return Err("Archive and destination paths must be absolute".into());
    }
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("Project archive contains too many entries".into());
    }
    let mut expanded = 0u64;
    let mut has_project_index = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!(
                "Archive contains an unsupported symbolic link: {}",
                entry.name()
            ));
        }
        let rel = Path::new(entry.name());
        if !safe_relative(rel) {
            return Err(format!("Unsafe archive path: {}", entry.name()));
        }
        has_project_index |= rel == Path::new("wiki/index.md") && !entry.is_dir();
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_ARCHIVE_BYTES {
            return Err("Project archive exceeds 4 GB expanded limit".into());
        }
    }
    if !has_project_index {
        return Err("Archive is not an Athenaeum project (wiki/index.md is missing)".into());
    }
    let root = PathBuf::from(destination);
    if root.exists()
        && fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("Import destination must be empty".into());
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let rel = Path::new(entry.name());
        let target = root.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = File::create(target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
    }
    Ok(root.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildIndexResult {
    pub pages: usize,
    pub groups: usize,
}

fn frontmatter_value(content: &str, key: &str) -> Option<String> {
    let normalized = content.replace("\r\n", "\n");
    let body = normalized.strip_prefix("---\n")?.split_once("\n---")?.0;
    body.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['\"', '\'']).to_string())
        })
        .filter(|value| !value.is_empty())
}

#[tauri::command]
pub async fn rebuild_wiki_index(project_path: String) -> Result<RebuildIndexResult, String> {
    tauri::async_runtime::spawn_blocking(move || rebuild_wiki_index_inner(project_path))
        .await
        .map_err(|error| format!("Index rebuild task failed: {error}"))?
}

fn rebuild_wiki_index_inner(project_path: String) -> Result<RebuildIndexResult, String> {
    let wiki = PathBuf::from(project_path).join("wiki");
    let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for entry in WalkDir::new(&wiki).follow_links(false) {
        let entry = entry.map_err(|error| format!("Failed to enumerate wiki pages: {error}"))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|v| v.to_str()) != Some("md")
        {
            continue;
        }
        let stem = entry
            .path()
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if matches!(
            stem.to_ascii_lowercase().as_str(),
            "index" | "overview" | "log"
        ) {
            continue;
        }
        let content = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
        let kind = frontmatter_value(&content, "type").unwrap_or_else(|| "other".into());
        let title = frontmatter_value(&content, "title").unwrap_or_else(|| stem.to_string());
        let target = entry
            .path()
            .strip_prefix(&wiki)
            .map_err(|e| e.to_string())?
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        groups.entry(kind).or_default().push((target, title));
    }
    for pages in groups.values_mut() {
        pages.sort_by_key(|page| page.1.to_lowercase());
    }
    let count = groups.values().map(Vec::len).sum();
    let mut output = String::from("# Wiki Index\n\n");
    for (kind, pages) in &groups {
        output.push_str(&format!("## {}\n\n", kind));
        for (slug, title) in pages {
            output.push_str(&format!("- [[{}|{}]]\n", slug, title));
        }
        output.push('\n');
    }
    let index_path = wiki.join("index.md");
    let temporary_path = wiki.join(".index.md.rebuild.tmp");
    let mut file = File::create(&temporary_path).map_err(|e| e.to_string())?;
    file.write_all(output.as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    if index_path.exists() {
        fs::remove_file(&index_path).map_err(|e| e.to_string())?;
    }
    fs::rename(&temporary_path, &index_path).map_err(|e| e.to_string())?;
    Ok(RebuildIndexResult {
        pages: count,
        groups: groups.len(),
    })
}
