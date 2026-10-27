//! Edit the desktop's real Tauri store, preserving fields this CLI does not know.
//! The desktop must be closed for writes: its plugin-store holds an in-memory copy.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use athenaeum_core::portable::portable_error;
use athenaeum_core::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use crate::Outcome;

#[derive(Args)]
pub struct ConfigArgs {
    /// Override the desktop app-state.json path. Also WIKI_SETTINGS_FILE.
    #[arg(long, global = true)]
    file: Option<PathBuf>,
    /// Include secret values in reads and exports. Default: redact them.
    #[arg(long, global = true)]
    reveal: bool,
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show the actual settings file path and whether it exists.
    Path,
    /// List saved settings. Tokens, keys, passwords, and custom headers are redacted.
    List,
    /// Read a dotted key or JSON Pointer, e.g. llmConfig.model or /providerConfigs/openai/model.
    Get { key: String },
    /// Save a value (JSON when valid, otherwise a string). Creates missing objects.
    Set {
        key: String,
        #[arg(required_unless_present_any = ["stdin", "value_file"], conflicts_with_all = ["stdin", "value_file"])]
        value: Option<String>,
        /// Read the value from standard input instead of the command line.
        #[arg(long, conflicts_with = "value_file")]
        stdin: bool,
        /// Read the value from a UTF-8 file.
        #[arg(long)]
        value_file: Option<PathBuf>,
    },
    /// Remove one saved value so the app can use its default.
    Unset { key: String },
    /// Export settings. Secrets stay redacted unless --reveal was given.
    Export { output: PathBuf },
    /// Merge a JSON object into saved settings. Redacted secret placeholders are ignored.
    Import { input: PathBuf },
}

pub fn path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("WIKI_SETTINGS_FILE").map(PathBuf::from))
    {
        return Ok(path);
    }
    let home = || {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|p| p.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|p| p.join(".local/share")))
    };
    base.map(|p| p.join("com.hanoryx.athenaeum/app-state.json"))
        .ok_or_else(|| {
            portable_error(
                "not_configured",
                "Cannot locate desktop settings; pass --file or set WIKI_SETTINGS_FILE",
            )
        })
}

fn read(path: &Path) -> Result<Value> {
    let value: Value = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.into()),
    };
    if !value.is_object() {
        return Err(portable_error(
            "invalid_config",
            "Settings must be a JSON object; refusing to overwrite this file",
        ));
    }
    Ok(value)
}

fn parts(key: &str) -> Result<Vec<String>> {
    if key.is_empty() {
        return Err(portable_error(
            "invalid_request",
            "A settings key is required",
        ));
    }
    if let Some(pointer) = key.strip_prefix('/') {
        let mut parts = Vec::new();
        for part in pointer.split('/') {
            let mut chars = part.chars();
            let mut decoded = String::new();
            while let Some(c) = chars.next() {
                if c != '~' {
                    decoded.push(c);
                    continue;
                }
                decoded.push(match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => {
                        return Err(portable_error(
                            "invalid_request",
                            "Invalid JSON Pointer escape; use ~0 for ~ and ~1 for /",
                        ))
                    }
                });
            }
            parts.push(decoded);
        }
        Ok(parts)
    } else {
        let parts = key.split('.').map(str::to_string).collect::<Vec<_>>();
        if parts.iter().any(String::is_empty) {
            return Err(portable_error(
                "invalid_request",
                "Empty dotted key; use a JSON Pointer for literal dots",
            ));
        }
        Ok(parts)
    }
}

fn secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "authorization",
        "customheaders",
        "cookie",
    ]
    .iter()
    .any(|part| key.contains(part))
}

pub(crate) fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if secret(key) && !value.is_null() {
                            json!("[REDACTED]")
                        } else {
                            redact(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
        other => other.clone(),
    }
}

fn locate_mut<'a>(
    value: &'a mut Value,
    parts: &[String],
    create: bool,
) -> Result<&'a mut Map<String, Value>> {
    let mut cursor = value;
    for part in &parts[..parts.len() - 1] {
        if !cursor.is_object() {
            return Err(portable_error(
                "invalid_request",
                format!("{part} is inside a scalar or array; replace the containing value instead"),
            ));
        }
        if create {
            cursor = cursor
                .as_object_mut()
                .unwrap()
                .entry(part)
                .or_insert_with(|| json!({}));
        } else {
            cursor = cursor
                .get_mut(part)
                .ok_or_else(|| portable_error("not_found", format!("No setting {part}")))?;
        }
    }
    cursor
        .as_object_mut()
        .ok_or_else(|| portable_error("invalid_request", "Parent setting is not an object"))
}

fn merge(target: &mut Value, incoming: &Value) {
    if incoming == "[REDACTED]" {
        return;
    }
    if let (Some(target), Some(incoming)) = (target.as_object_mut(), incoming.as_object()) {
        for (key, value) in incoming {
            merge(target.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *target = incoming.clone();
    }
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn private_file(path: &Path) -> Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn write(path: &Path, value: &Value) -> Result<()> {
    let temporary = path.with_extension(format!("wiki-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary, metadata.permissions())?;
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn run(json_out: bool, args: &ConfigArgs) -> Result<Outcome> {
    let path = path(args.file.as_deref())?;
    if matches!(args.command, ConfigCommand::Path) {
        let report = json!({"path": path, "exists": path.is_file()});
        crate::emit(json_out, &report, || {
            println!("{}", path.display());
            Ok(())
        })?;
        return Ok(Outcome::Ok);
    }
    let mutating = matches!(
        args.command,
        ConfigCommand::Set { .. } | ConfigCommand::Unset { .. } | ConfigCommand::Import { .. }
    );
    let _lock = if mutating {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let lock = path.with_extension("wiki-lock");
        private_file(&lock).map_err(|error| portable_error("settings_locked", format!("Cannot lock {}: {error}. If a previous wiki process was interrupted, remove that lock file.", lock.display())))?;
        Some(Lock(lock))
    } else {
        None
    };
    let mut value = read(&path)?;
    let answer = match &args.command {
        ConfigCommand::Path => json!({"path": path, "exists": path.is_file()}),
        ConfigCommand::List => {
            if args.reveal {
                value.clone()
            } else {
                redact(&value)
            }
        }
        ConfigCommand::Get { key } => {
            let parts = parts(key)?;
            let mut found = &value;
            for part in &parts {
                found = found
                    .get(part)
                    .ok_or_else(|| portable_error("not_found", format!("No setting {key}")))?;
            }
            if args.reveal {
                found.clone()
            } else if parts.iter().any(|p| secret(p)) {
                json!("[REDACTED]")
            } else {
                redact(found)
            }
        }
        ConfigCommand::Set {
            key,
            value: input,
            stdin,
            value_file,
        } => {
            let text = if *stdin {
                let mut text = String::new();
                std::io::stdin()
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_string(&mut text)?;
                text
            } else if let Some(file) = value_file {
                fs::read_to_string(file)?
            } else {
                input.clone().unwrap_or_default()
            };
            if text.len() > 16 * 1024 * 1024 {
                return Err(portable_error(
                    "invalid_request",
                    "Setting value exceeds 16 MiB",
                ));
            }
            let input = serde_json::from_str(&text)
                .unwrap_or_else(|_| Value::String(text.trim_end_matches(['\r', '\n']).into()));
            let parts = parts(key)?;
            locate_mut(&mut value, &parts, true)?.insert(parts.last().unwrap().clone(), input);
            json!({"saved": true, "key": key, "path": path, "applies": "next desktop launch"})
        }
        ConfigCommand::Unset { key } => {
            let parts = parts(key)?;
            let removed = locate_mut(&mut value, &parts, false)?
                .remove(parts.last().unwrap())
                .is_some();
            json!({"removed": removed, "key": key, "path": path})
        }
        ConfigCommand::Export { output } => {
            let exported = if args.reveal {
                value.clone()
            } else {
                redact(&value)
            };
            let mut file = private_file(output)?;
            file.write_all(format!("{}\n", serde_json::to_string_pretty(&exported)?).as_bytes())?;
            json!({"exported": output, "redacted": !args.reveal})
        }
        ConfigCommand::Import { input } => {
            if !input.is_file() {
                return Err(portable_error(
                    "not_found",
                    format!("Import file does not exist: {}", input.display()),
                ));
            }
            let incoming = read(input)?;
            merge(&mut value, &incoming);
            json!({"saved": true, "path": path, "applies": "next desktop launch"})
        }
    };
    if mutating {
        // Preserve the first original. Malformed JSON was already refused before this point.
        if path.is_file() {
            let backup = path.with_extension("json.wiki-backup");
            match private_file(&backup) {
                Ok(mut file) => {
                    file.write_all(&fs::read(&path)?)?;
                    file.sync_all()?;
                }
                Err(athenaeum_core::Error::Io(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        write(&path, &value)?;
    }
    crate::emit(json_out, &answer, || {
        println!("{}", serde_json::to_string_pretty(&answer)?);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}
