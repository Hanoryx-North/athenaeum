use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_HISTORY_CONTENT_BYTES: usize = 512 * 1024;
const DEFAULT_ENTRIES_PER_FILE: usize = 10;
const MAX_ENTRIES_PER_FILE: usize = 30;
const MAX_HISTORY_STORE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_HISTORY_STORE_FILES: usize = 2_048;
static HISTORY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistoryEntry {
    pub id: String,
    pub path: String,
    pub timestamp: i64,
    pub author: String,
    pub tool: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistoryStats {
    pub bytes: u64,
    pub files: usize,
    pub entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct FileHistorySettings {
    pub enabled: bool,
    pub max_versions_per_file: usize,
}

impl Default for FileHistorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_versions_per_file: DEFAULT_ENTRIES_PER_FILE,
        }
    }
}

impl FileHistorySettings {
    fn normalized(mut self) -> Self {
        self.max_versions_per_file = self.max_versions_per_file.min(MAX_ENTRIES_PER_FILE);
        self
    }
}

fn history_settings_path(root: &Path) -> PathBuf {
    root.join(".athenaeum/history-settings.json")
}

fn validate_optional_regular_file(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(format!("{label} must be a regular file, not a symlink")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn read_history_settings(root: &Path) -> FileHistorySettings {
    let path = history_settings_path(root);
    if validate_optional_regular_file(&path, "History settings").is_err() {
        return FileHistorySettings::default();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<FileHistorySettings>(&raw).ok())
        .unwrap_or_default()
        .normalized()
}

fn project_root_for(path: &Path) -> Option<PathBuf> {
    let mut cursor = path.parent();
    while let Some(dir) = cursor {
        let metadata_dir = dir.join(".athenaeum");
        if fs::symlink_metadata(&metadata_dir)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

fn history_path(root: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
    // Separators are normalized before hashing, the same way vault paths are:
    // `\` becomes `/` on every platform, which is what `normalize_path` does in
    // athenaeum-core's anchor, git and scan modules. The recording side sees the
    // caller's path verbatim while the reading side has canonicalized it, and on
    // Windows those differ only by `\` vs `/`. Hashing the raw string gave the
    // two sides different addresses, so history was recorded and then never
    // found again. Normalizing unconditionally rather than by MAIN_SEPARATOR
    // costs a collision between `a\b.md` and `a/b.md` on Unix, where `\` is a
    // legal filename character; core accepts that same trade so that a vault
    // keeps one address on every platform it is opened on.
    let relative = relative.replace('\\', "/");
    // Fixed FNV-1a keeps history addresses stable across Rust/toolchain upgrades.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in relative.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let key = format!("{hash:016x}");
    root.join(".athenaeum/history").join(format!("{key}.json"))
}

fn history_dir(root: &Path) -> PathBuf {
    root.join(".athenaeum/history")
}

fn checked_history_file(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let store_path = history_path(root, path);
    validate_optional_regular_file(&store_path, "History file")?;
    Ok(store_path)
}

fn checked_history_dir(root: &Path, create: bool) -> Result<PathBuf, String> {
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let metadata_dir = canonical_root.join(".athenaeum");
    let metadata = fs::symlink_metadata(&metadata_dir).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("History metadata directory must be a real project directory".to_string());
    }

    let dir = metadata_dir.join("history");
    if create {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    } else if !dir.exists() {
        return Ok(dir);
    }
    let dir_metadata = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !dir_metadata.is_dir() || dir_metadata.file_type().is_symlink() {
        return Err("History directory must not be a symlink".to_string());
    }
    let canonical_dir = dir.canonicalize().map_err(|e| e.to_string())?;
    if !canonical_dir.starts_with(&metadata_dir) {
        return Err("History directory must stay inside the project".to_string());
    }
    Ok(canonical_dir)
}

fn history_files_oldest_first(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let Ok(entries) = fs::read_dir(history_dir(root)) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() || file_type.is_symlink() {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                return None;
            }
            Some((
                entry.path(),
                metadata.len(),
                metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            ))
        })
        .collect();
    files.sort_by_key(|(_, _, modified)| *modified);
    files
}

fn prune_history_store(root: &Path, protected_path: &Path) {
    let files = history_files_oldest_first(root);
    let mut total_bytes = files.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
    let mut total_files = files.len();
    for (path, bytes, _) in files {
        if total_bytes <= MAX_HISTORY_STORE_BYTES && total_files <= MAX_HISTORY_STORE_FILES {
            break;
        }
        if path == protected_path {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            total_bytes = total_bytes.saturating_sub(bytes);
            total_files = total_files.saturating_sub(1);
        }
    }
}

fn prune_entries_per_file(root: &Path, max_versions: usize) -> Result<(), String> {
    let dir = checked_history_dir(root, false)?;
    if !dir.exists() {
        return Ok(());
    }
    if max_versions == 0 {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        return Ok(());
    }
    for (path, _, _) in history_files_oldest_first(root) {
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let mut entries: Vec<FileHistoryEntry> = match serde_json::from_str(&raw) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        if entries.len() > max_versions {
            entries.drain(..entries.len() - max_versions);
            let serialized = serde_json::to_string(&entries).map_err(|e| e.to_string())?;
            fs::write(&path, serialized).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn record_file_version(path: &Path, author: &str, tool: &str) {
    let Some(root) = project_root_for(path) else {
        return;
    };
    if path.starts_with(root.join(".athenaeum")) {
        return;
    }
    let Ok(_guard) = HISTORY_LOCK.lock() else {
        return;
    };
    let settings = read_history_settings(&root);
    if !settings.enabled || settings.max_versions_per_file == 0 {
        return;
    }
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if !metadata.is_file() || metadata.len() as usize > MAX_HISTORY_CONTENT_BYTES {
        return;
    }
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(dir) = checked_history_dir(&root, true) else {
        return;
    };
    let Ok(checked_store_path) = checked_history_file(&root, path) else {
        return;
    };
    let Some(file_name) = checked_store_path.file_name().map(ToOwned::to_owned) else {
        return;
    };
    let store_path = dir.join(file_name);
    let mut entries: Vec<FileHistoryEntry> = fs::read_to_string(&store_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if entries.last().is_some_and(|entry| entry.content == content) {
        return;
    }
    entries.push(FileHistoryEntry {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().replace('\\', "/"),
        timestamp: Utc::now().timestamp_millis(),
        author: author.to_string(),
        tool: tool.to_string(),
        content,
    });
    if entries.len() > settings.max_versions_per_file {
        entries.drain(..entries.len() - settings.max_versions_per_file);
    }
    if let Ok(raw) = serde_json::to_string(&entries) {
        if fs::write(&store_path, raw).is_ok() {
            prune_history_store(&root, &store_path);
        }
    }
}

#[tauri::command]
pub async fn get_file_history_settings(
    project_path: String,
) -> Result<FileHistorySettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = checked_project_root(&project_path)?;
        Ok(read_history_settings(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_file_history_settings(
    project_path: String,
    settings: FileHistorySettings,
) -> Result<FileHistorySettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = checked_project_root(&project_path)?;
        let mut settings = settings.normalized();
        if settings.enabled && settings.max_versions_per_file == 0 {
            settings.max_versions_per_file = DEFAULT_ENTRIES_PER_FILE;
        }
        let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
        let previous = read_history_settings(&root);
        let settings_path = history_settings_path(&root);
        let had_settings = validate_optional_regular_file(&settings_path, "History settings")
            .is_ok()
            && fs::read_to_string(&settings_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<FileHistorySettings>(&raw).ok())
                .is_some();
        let raw = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        validate_optional_regular_file(&settings_path, "History settings")?;
        fs::write(settings_path, raw).map_err(|e| e.to_string())?;
        if !had_settings || settings.max_versions_per_file < previous.max_versions_per_file {
            prune_entries_per_file(&root, settings.max_versions_per_file)?;
        }
        Ok(settings)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn checked_project_root(project_path: &str) -> Result<PathBuf, String> {
    let root = Path::new(project_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(root.join(".athenaeum")).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("History project must contain .athenaeum".to_string());
    }
    Ok(root)
}

fn checked_file(project_path: &str, file_path: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = Path::new(project_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let file = Path::new(file_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !file.starts_with(&root) || file.starts_with(root.join(".athenaeum")) {
        return Err("History path must stay inside the project".to_string());
    }
    Ok((root, file))
}

#[tauri::command]
pub async fn list_file_history(
    project_path: String,
    file_path: String,
) -> Result<Vec<FileHistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, file) = checked_file(&project_path, &file_path)?;
        let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
        checked_history_dir(&root, false)?;
        let store_path = checked_history_file(&root, &file)?;
        let raw = fs::read_to_string(store_path).unwrap_or_else(|_| "[]".to_string());
        let mut entries: Vec<FileHistoryEntry> = serde_json::from_str(&raw).unwrap_or_default();
        entries.reverse();
        Ok(entries)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_file_history(
    project_path: String,
    file_path: String,
    entry_id: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, file) = checked_file(&project_path, &file_path)?;
        let content = {
            let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
            checked_history_dir(&root, false)?;
            let store_path = checked_history_file(&root, &file)?;
            let raw = fs::read_to_string(&store_path).map_err(|e| e.to_string())?;
            let mut entries: Vec<FileHistoryEntry> =
                serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let entry = entries
                .iter()
                .find(|entry| entry.id == entry_id)
                .cloned()
                .ok_or_else(|| "History entry not found".to_string())?;
            let current = fs::read_to_string(&file).map_err(|e| e.to_string())?;
            if current.len() > MAX_HISTORY_CONTENT_BYTES {
                return Err(format!(
                    "Current file is too large to back up safely before restore (maximum {} bytes)",
                    MAX_HISTORY_CONTENT_BYTES
                ));
            }
            if current != entry.content
                && entries
                    .last()
                    .is_none_or(|existing| existing.content != current)
            {
                entries.push(FileHistoryEntry {
                    id: Uuid::new_v4().to_string(),
                    path: file.to_string_lossy().replace('\\', "/"),
                    timestamp: Utc::now().timestamp_millis(),
                    author: "human".to_string(),
                    tool: "before.history.restore".to_string(),
                    content: current,
                });
                let retention = read_history_settings(&root).max_versions_per_file.max(1);
                if entries.len() > retention {
                    entries.drain(..entries.len() - retention);
                }
                let updated = serde_json::to_string(&entries).map_err(|e| e.to_string())?;
                fs::write(&store_path, updated).map_err(|e| e.to_string())?;
                prune_history_store(&root, &store_path);
            }
            fs::write(&file, &entry.content).map_err(|e| e.to_string())?;
            entry.content
        };
        Ok(content)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_file_history_stats(project_path: String) -> Result<FileHistoryStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = checked_project_root(&project_path)?;
        let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
        checked_history_dir(&root, false)?;
        let files = history_files_oldest_first(&root);
        let entries = files
            .iter()
            .filter_map(|(path, _, _)| fs::read_to_string(path).ok())
            .filter_map(|raw| serde_json::from_str::<Vec<FileHistoryEntry>>(&raw).ok())
            .map(|entries| entries.len())
            .sum();
        Ok(FileHistoryStats {
            bytes: files.iter().map(|(_, bytes, _)| *bytes).sum(),
            files: files.len(),
            entries,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_file_history(project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = checked_project_root(&project_path)?;
        let _guard = HISTORY_LOCK.lock().map_err(|e| e.to_string())?;
        let dir = checked_history_dir(&root, false)?;
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
