//! Desktop transport for Athenaeum Agent and the optional Harness integration.
//!
//! Small Node drivers adapt native app-server or the public Harness SDK.
//! This module only owns the app-to-driver process and relays notifications;
//! it contains no agent loop, tool execution, or model code.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use uuid::Uuid;

const DRIVER_NAME: &str = "harness-driver.mjs";
const DEFAULT_PROFILE: &str = "athenaeum";
const STDERR_TAIL_LIMIT: usize = 80;
/// The per-launch profile patch that registers a Settings-chosen route, under the app data dir.
const ROUTE_PATCH_FILE: &str = "harness-chat-route.cordis.yml";
/// The only variables a Settings key may be placed in: the one the generated patch names, and
/// the one the harness's own DeepSeek adapter reads. Anything else (PATH, DSH_HOME, ...) is refused.
const ROUTE_KEY_ENVS: [&str; 2] = ["ATHENAEUM_LLM_API_KEY", "DEEPSEEK_API_KEY"];

/// The chat agent's model route as Settings describes it; see `src/lib/harness-route.ts`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRoute {
    provider: String,
    model: String,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    pi_provider: Option<Value>,
}

impl HarnessRoute {
    fn validate(&self) -> Result<(), String> {
        let route_id = !self.provider.is_empty()
            && self
                .provider
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !route_id {
            return Err(format!(
                "Chat route \"{}\" is not a harness route id.",
                self.provider
            ));
        }
        if self.model.trim().is_empty() {
            return Err("Chat route has no model.".to_string());
        }
        if let Some(name) = &self.api_key_env {
            if !ROUTE_KEY_ENVS.contains(&name.as_str()) {
                return Err(format!("Chat route may not set the {name} variable."));
            }
        }
        Ok(())
    }

    /// What decides whether a running runtime still serves this route. The key is reduced to
    /// a digest so a changed key restarts the runtime without the identity holding the key.
    fn identity(&self) -> String {
        let key = self
            .api_key
            .as_deref()
            .map(|key| format!("{:x}", Sha256::digest(key.as_bytes())))
            .unwrap_or_default();
        json!([
            self.provider,
            self.model,
            self.api_key_env,
            self.pi_provider,
            key
        ])
        .to_string()
    }
}

#[derive(Default)]
pub struct HarnessChatState {
    runtime: StdMutex<Option<Arc<HarnessRuntime>>>,
}

struct HarnessRuntime {
    workspace: PathBuf,
    /// [`HarnessRoute::identity`] of the route it was launched with; empty for the defaults.
    route: String,
    stdin: StdMutex<ChildStdin>,
    child: StdMutex<Child>,
    pending: PendingRequests,
    serial: AtomicU64,
}

type PendingRequests = Arc<StdMutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Deserialize)]
struct InstallManifest {
    #[serde(default)]
    node: Option<PathBuf>,
    #[serde(default)]
    dsh_home: Option<PathBuf>,
    #[serde(default)]
    profile: Option<String>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessStatus {
    workspace: String,
    provider: String,
    model: String,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessTurnResult {
    session_id: String,
    final_response: String,
}

impl HarnessChatState {
    fn runtime_for(
        &self,
        app: &AppHandle,
        workspace: &Path,
        route: Option<&HarnessRoute>,
    ) -> Result<Arc<HarnessRuntime>, String> {
        if let Some(route) = route {
            route.validate()?;
        }
        let identity = route.map(HarnessRoute::identity).unwrap_or_default();
        let workspace = workspace
            .canonicalize()
            .map(plain_path)
            .map_err(|error| format!("Could not open the selected project workspace: {error}"))?;
        if !workspace.is_dir() {
            return Err("The selected project workspace is not a directory.".to_string());
        }

        let mut slot = self
            .runtime
            .lock()
            .map_err(|_| "The Harness client state is unavailable.".to_string())?;
        if let Some(runtime) = slot.as_ref() {
            if runtime.workspace == workspace && runtime.route == identity && runtime.is_running() {
                return Ok(runtime.clone());
            }
            if runtime.has_pending_turn() {
                return Err(
                    "A Harness turn is still running. Stop it before switching projects or models."
                        .to_string(),
                );
            }
            runtime.shutdown();
            *slot = None;
        }

        let runtime = Arc::new(HarnessRuntime::launch(app, workspace, route, identity)?);
        *slot = Some(runtime.clone());
        Ok(runtime)
    }

    fn current_runtime(&self) -> Option<Arc<HarnessRuntime>> {
        self.runtime.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn shutdown(&self) {
        if let Ok(mut slot) = self.runtime.lock() {
            if let Some(runtime) = slot.take() {
                runtime.shutdown();
            }
        }
    }
}

impl HarnessRuntime {
    fn launch(
        app: &AppHandle,
        workspace: PathBuf,
        route: Option<&HarnessRoute>,
        identity: String,
    ) -> Result<Self, String> {
        let mut command = if route.is_some_and(|route| route.provider == "athenaeum-native") {
            native_command(app, &workspace, route)?
        } else {
            let (manifest_path, prefix, manifest) = locate_install()?;
            let dsh_home = manifest
                .dsh_home
                .as_ref()
                .map(|path| resolve_from(&prefix, path))
                .unwrap_or_else(|| prefix.join("dsh-home"));
            let profile = manifest.profile.as_deref().unwrap_or(DEFAULT_PROFILE);
            let profile_dir = dsh_home.join("profiles").join(profile);
            let sdk_client = profile_dir
                .join("node_modules")
                .join("@deepseek-ai")
                .join("dsh-sdk-client")
                .join("lib")
                .join("index.js");
            if !sdk_client.is_file() {
                return Err(format!(
                "The installed Athenaeum Agent is missing its Harness SDK client. Re-run its installer, then retry. ({})",
                sdk_client.display()
            ));
            }
            let dsh_bin = prefix
                .join("runtime")
                .join("node_modules")
                .join("@deepseek-ai")
                .join("dsh")
                .join("lib")
                .join("bin.js");
            if !dsh_bin.is_file() {
                return Err(format!(
                "The installed Athenaeum Agent has no usable Harness CLI runtime. Re-run its installer. ({})",
                dsh_bin.display()
            ));
            }

            let node = manifest
            .node
            .as_ref()
            .map(|path| resolve_from(&prefix, path))
            .filter(|path| path.is_file())
            .or_else(|| private_node_path(&prefix).filter(|path| path.is_file()))
            .ok_or_else(|| {
                format!(
                    "The installed Athenaeum Agent has no usable Node runtime. Re-run its installer. ({})",
                    manifest_path.display()
                )
            })?;
            let driver = app
                .path()
                .resource_dir()
                .ok()
                .map(|directory| plain_path(directory.join(DRIVER_NAME)))
                .filter(|path| path.is_file())
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("resources")
                        .join(DRIVER_NAME)
                });
            if !driver.is_file() {
                return Err(format!(
                    "The packaged Harness driver is missing: {}",
                    driver.display()
                ));
            }

            let mut command = Command::new(&node);
            command
                .arg(&driver)
                .arg(&sdk_client)
                .arg(&dsh_home)
                .arg(profile)
                .arg(&dsh_bin)
                .current_dir(&workspace)
                .env("DSH_HOME", &dsh_home)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            prepend_agent_path(&mut command, &prefix);
            if let Some(route) = route {
                apply_route(app, &mut command, route)?;
            }

            command
        };

        let mut child = command.spawn().map_err(|error| {
            format!("Could not start the installed Athenaeum Agent runtime: {error}")
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Could not open the Harness driver input.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Could not open the Harness driver output.".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Could not open the Harness driver diagnostics.".to_string())?;
        let pending = Arc::new(StdMutex::new(HashMap::new()));
        let stderr_tail = Arc::new(StdMutex::new(VecDeque::new()));

        let pending_reader = pending.clone();
        let stderr_reader = stderr_tail.clone();
        let event_app = app.clone();
        thread::Builder::new()
            .name("athenaeum-harness-stdout".to_string())
            .spawn(move || {
                read_driver_output(stdout, &event_app, &pending_reader, &stderr_reader);
            })
            .map_err(|error| format!("Could not start the Harness event reader: {error}"))?;

        let stderr_tail_reader = stderr_tail.clone();
        thread::Builder::new()
            .name("athenaeum-harness-stderr".to_string())
            .spawn(move || read_driver_stderr(stderr, &stderr_tail_reader))
            .map_err(|error| format!("Could not start the Harness diagnostics reader: {error}"))?;

        Ok(Self {
            workspace,
            route: identity,
            stdin: StdMutex::new(stdin),
            child: StdMutex::new(child),
            pending,
            serial: AtomicU64::new(0),
        })
    }

    fn next_request_id(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{}-{}",
            self.serial.fetch_add(1, Ordering::Relaxed),
            Uuid::new_v4()
        )
    }

    async fn request(&self, request: Value) -> Result<Value, String> {
        let request_id = request
            .get("requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Harness request has no id.".to_string())?
            .to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| "The Harness response queue is unavailable.".to_string())?
            .insert(request_id.clone(), sender);
        if let Err(error) = self.send(&request) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&request_id);
            }
            return Err(error);
        }
        receiver
            .await
            .map_err(|_| "The Harness runtime stopped before replying.".to_string())?
    }

    fn send(&self, request: &Value) -> Result<(), String> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| "The Harness driver input is unavailable.".to_string())?;
        serde_json::to_writer(&mut *stdin, request)
            .map_err(|error| format!("Could not encode the Harness request: {error}"))?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("Could not send a request to the Harness driver: {error}"))
    }

    fn is_running(&self) -> bool {
        self.child
            .lock()
            .map(|mut child| {
                child
                    .try_wait()
                    .map(|status| status.is_none())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    fn has_pending_turn(&self) -> bool {
        self.pending
            .lock()
            .map(|pending| !pending.is_empty())
            .unwrap_or(false)
    }

    fn shutdown(&self) {
        if self.is_running() {
            let active_runs = self
                .pending
                .lock()
                .map(|pending| {
                    pending
                        .keys()
                        .filter(|request_id| request_id.starts_with("ui-"))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for run_id in active_runs {
                let request = json!({
                    "type": "cancel",
                    "requestId": self.next_request_id("cancel"),
                    "runId": run_id,
                });
                let _ = self.send(&request);
            }
            let request =
                json!({ "type": "shutdown", "requestId": self.next_request_id("shutdown") });
            let _ = self.send(&request);
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        if let Ok(mut child) = self.child.lock() {
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(40)),
                    Err(_) => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for HarnessRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_driver_output(
    stdout: std::process::ChildStdout,
    app: &AppHandle,
    pending: &PendingRequests,
    stderr_tail: &Arc<StdMutex<VecDeque<String>>>,
) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match message.get("kind").and_then(Value::as_str) {
            Some("notification") => {
                let _ = app.emit("harness-notification", &message);
            }
            Some(kind) => {
                let Some(request_id) = message.get("requestId").and_then(Value::as_str) else {
                    continue;
                };
                let sender = pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(request_id));
                let Some(sender) = sender else { continue };
                let result = match kind {
                    "ready" | "result" | "cancel-accepted" | "approval-accepted" | "shutdown" => {
                        Ok(message)
                    }
                    "cancelled" => Ok(json!({ "cancelled": true })),
                    "error" | "fatal" => Err(message
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("The Harness runtime reported an error.")
                        .to_string()),
                    _ => continue,
                };
                let _ = sender.send(result);
            }
            None => continue,
        }
    }

    let tail = stderr_tail
        .lock()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    let message = if tail.is_empty() {
        "The Harness driver stopped unexpectedly.".to_string()
    } else {
        format!("The Harness driver stopped unexpectedly.\n{tail}")
    };
    if let Ok(mut pending) = pending.lock() {
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(message.clone()));
        }
    }
}

fn read_driver_stderr(stderr: std::process::ChildStderr, tail: &Arc<StdMutex<VecDeque<String>>>) {
    for line in BufReader::new(stderr).lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        if let Ok(mut lines) = tail.lock() {
            lines.push_back(line);
            while lines.len() > STDERR_TAIL_LIMIT {
                lines.pop_front();
            }
        }
    }
}

/// Launch the source-built Athenaeum runtime as a headless service for our own UI.
pub(crate) fn native_agent_binary() -> Result<PathBuf, String> {
    let prefix = default_prefix()?;
    let executable = if cfg!(windows) {
        "athenaeum-agent.exe"
    } else {
        "athenaeum-agent"
    };
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("ATHENAEUM_AGENT_BIN") {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| "ATHENAEUM_AGENT_BIN does not name a file.".to_string());
    }
    // Development uses the one checkout's native runtime build.
    if cfg!(debug_assertions) {
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../target/native-runtime/debug")
                .join(executable),
        );
    }
    candidates.push(prefix.join("bin").join(executable));
    candidates.into_iter().find(|path| path.is_file()).ok_or(
        "Athenaeum Agent is not installed. Build or install its native runtime, or select the optional Harness integration."
    ).map_err(str::to_string)
}

fn native_command(
    app: &AppHandle,
    workspace: &Path,
    route: Option<&HarnessRoute>,
) -> Result<Command, String> {
    let prefix = default_prefix()?;
    let binary = native_agent_binary()?;
    let node = private_node_path(&prefix)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("node"));
    let driver = app
        .path()
        .resource_dir()
        .ok()
        .map(|path| plain_path(path.join("native-agent-driver.mjs")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/native-agent-driver.mjs")
        });
    let mut command = Command::new(node);
    command
        .arg(driver)
        .arg(plain_path(binary))
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cli) = crate::cli_tools::locate_cli() {
        command.env("ATHENAEUM_BIN", cli);
    }
    if let Some(route) = route {
        if route.model != "native-configuration" {
            command.env("ATHENAEUM_AGENT_MODEL", &route.model);
        }
        // An explicitly keyless Settings route must not inherit a different provider's key.
        command.env(
            "ATHENAEUM_LLM_API_KEY",
            route.api_key.as_deref().unwrap_or(""),
        );
        if let Some(base) = route
            .pi_provider
            .as_ref()
            .and_then(|p| p.get("baseURL"))
            .and_then(Value::as_str)
        {
            command.env("ATHENAEUM_AGENT_BASE_URL", base);
        }
    }
    if let Some(api) = route
        .and_then(|r| r.pi_provider.as_ref())
        .and_then(|p| p.get("api"))
        .and_then(Value::as_str)
    {
        command.env("ATHENAEUM_AGENT_API", api);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Ok(command)
}

fn locate_install() -> Result<(PathBuf, PathBuf, InstallManifest), String> {
    let manifest_path = if let Some(path) = env::var_os("ATHENAEUM_AGENT_INSTALL_JSON") {
        PathBuf::from(path)
    } else {
        default_prefix()?.join("install.json")
    };
    let raw = fs::read_to_string(&manifest_path).map_err(|_| {
        format!(
            "Athenaeum Agent is not installed. Install it with the Athenaeum Agent installer, then retry. ({})",
            manifest_path.display()
        )
    })?;
    let manifest: InstallManifest = serde_json::from_str(&raw)
        .map_err(|error| format!("The Athenaeum Agent install record is invalid: {error}"))?;
    let prefix = manifest_path
        .parent()
        .ok_or_else(|| "The Athenaeum Agent install path has no parent.".to_string())?
        .to_path_buf();
    Ok((manifest_path, prefix, manifest))
}

fn default_prefix() -> Result<PathBuf, String> {
    if let Some(prefix) = env::var_os("ATHENAEUM_AGENT_PREFIX") {
        return Ok(PathBuf::from(prefix));
    }
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "Could not locate the current user's home directory.".to_string())?;
    #[cfg(target_os = "windows")]
    {
        let local = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        Ok(local.join("Athenaeum").join("agent"))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("Athenaeum")
            .join("agent"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let data = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        Ok(data.join("athenaeum").join("agent"))
    }
}

fn private_node_path(prefix: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let path = prefix.join("node").join("node.exe");
    #[cfg(not(target_os = "windows"))]
    let path = prefix.join("node").join("bin").join("node");
    Some(path)
}

/// Point the driver at `route`: its id and model, its `llm-pi-ai` registration as a
/// per-launch profile patch, and its key in the one variable the route reads.
///
/// The patch names the key's variable and never holds the key, which exists only in the
/// runtime's own environment.
fn apply_route(app: &AppHandle, command: &mut Command, route: &HarnessRoute) -> Result<(), String> {
    command
        .env("ATHENAEUM_AGENT_PROVIDER", &route.provider)
        .env("ATHENAEUM_AGENT_MODEL", route.model.trim())
        .env_remove("ATHENAEUM_AGENT_PATCH");
    if let Some(profile) = &route.pi_provider {
        let directory = app
            .path()
            .app_data_dir()
            .map(plain_path)
            .map_err(|error| format!("Could not locate the app data folder: {error}"))?;
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
        let patch = directory.join(ROUTE_PATCH_FILE);
        // A patch is a YAML array of loader entries; JSON is YAML, so serde writes it exactly.
        let entries = json!([{
            "id": "llm-pi-ai",
            "config": { "providers": { route.provider.as_str(): profile } },
        }]);
        fs::write(&patch, entries.to_string())
            .map_err(|error| format!("Could not write the chat route patch: {error}"))?;
        command.env("ATHENAEUM_AGENT_PATCH", &patch);
    }
    if let (Some(name), Some(key)) = (&route.api_key_env, &route.api_key) {
        command.env(name, key);
    }
    Ok(())
}

/// `path` without Windows' `\\?\` extended-length prefix on a drive path.
///
/// Tauri's resource directory and `canonicalize` both return that form, and Node cannot
/// start a script given one: resolving its main module stats the bare drive (`A:`) and
/// fails with EISDIR. A `\\?\UNC\` network path keeps its prefix.
fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

fn resolve_from(prefix: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        prefix.join(path)
    }
}

fn prepend_agent_path(command: &mut Command, prefix: &Path) {
    let mut paths = vec![
        prefix.join("bin"),
        prefix.join("runtime").join("node_modules").join(".bin"),
    ];
    #[cfg(target_os = "windows")]
    paths.push(prefix.join("node"));
    #[cfg(not(target_os = "windows"))]
    paths.push(prefix.join("node").join("bin"));
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    if let Ok(joined) = env::join_paths(paths) {
        command.env("PATH", joined);
    }
}

#[tauri::command]
pub async fn harness_check(
    app: AppHandle,
    state: State<'_, HarnessChatState>,
    project_path: String,
    route: Option<HarnessRoute>,
) -> Result<HarnessStatus, String> {
    let runtime = state.runtime_for(&app, Path::new(&project_path), route.as_ref())?;
    let response = runtime
        .request(json!({
            "type": "check",
            "requestId": runtime.next_request_id("check"),
            "projectPath": project_path,
        }))
        .await?;
    serde_json::from_value(response)
        .map_err(|error| format!("The Harness runtime returned an invalid status: {error}"))
}

#[tauri::command]
pub async fn harness_start_turn_stream(
    app: AppHandle,
    state: State<'_, HarnessChatState>,
    project_path: String,
    session_id: String,
    run_id: String,
    content_blocks: Vec<Value>,
    route: Option<HarnessRoute>,
) -> Result<HarnessTurnResult, String> {
    let runtime = state.runtime_for(&app, Path::new(&project_path), route.as_ref())?;
    let response = runtime
        .request(json!({
            "type": "run",
            "requestId": run_id,
            "runId": run_id,
            "projectPath": project_path,
            "sessionId": session_id,
            "contentBlocks": content_blocks,
        }))
        .await?;
    let result = response
        .get("result")
        .cloned()
        .ok_or_else(|| "The Harness runtime returned no turn result.".to_string())?;
    serde_json::from_value(result)
        .map_err(|error| format!("The Harness runtime returned an invalid turn result: {error}"))
}

#[tauri::command]
pub async fn harness_cancel_turn(
    state: State<'_, HarnessChatState>,
    run_id: String,
) -> Result<bool, String> {
    let Some(runtime) = state.current_runtime() else {
        return Ok(false);
    };
    let request_id = runtime.next_request_id("cancel");
    runtime.send(&json!({
        "type": "cancel",
        "requestId": request_id,
        "runId": run_id,
    }))?;
    Ok(true)
}

/// Disconnect the desktop SDK from its runtime; installed profiles and knowledge remain intact.
#[tauri::command]
pub fn harness_disconnect(state: State<'_, HarnessChatState>) {
    state.shutdown();
}

/// Resolve a native tool approval from Athenaeum's own chat controls.
#[tauri::command]
pub async fn agent_respond_approval(
    state: State<'_, HarnessChatState>,
    approval_id: String,
    decision: String,
) -> Result<(), String> {
    if decision != "accept" && decision != "decline" {
        return Err("Choose Allow or Deny".into());
    }
    let runtime = state.current_runtime().ok_or("The agent disconnected")?;
    runtime
        .request(
            json!({ "type": "approval", "requestId": runtime.next_request_id("approval"),
        "approvalId": approval_id, "decision": decision }),
        )
        .await?;
    Ok(())
}
