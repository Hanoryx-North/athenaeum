//! Versioned, one-shot client of the separate local core. Existing desktop
//! commands remain available while user data migrates between derived indexes.
//!
//! Which executable answers, in order: `ATHENAEUM_CORE_BIN`, then the older
//! `ATHENAEUM_CLI_BIN` (an explicit choice is the only choice, so a mistake in
//! it is reported rather than papered over), then an `athenaeum-core` shipped
//! beside the app, then `athenaeum-core` on `PATH`. Only a read may fall back
//! to the pre-split `athenaeum` command, which answers from the older
//! `knowledge.db` index; a write (propose, accept, ...) and the proposal queue
//! go to the core or fail, so new knowledge never lands in an index that is
//! being retired.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use athenaeum_protocol::bridge::{
    BridgeOutcome, BridgeRequest, BridgeRequestEnvelope, BridgeResponseEnvelope,
};
use athenaeum_protocol::BRIDGE_PROTOCOL_VERSION;
use serde_json::Value;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;
/// A knowledge walk is bounded work, not a model call; a core that has not
/// answered by now is stuck.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How an executable wants to be asked for one bridge exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flavor {
    /// `athenaeum-core bridge --workspace DIR`
    Core,
    /// The pre-split `athenaeum -C DIR bridge`.
    Legacy,
}

#[derive(Clone, Debug)]
struct Launch {
    executable: OsString,
    flavor: Flavor,
}

impl Launch {
    fn new(executable: impl Into<OsString>) -> Self {
        let executable = executable.into();
        let flavor = match Path::new(&executable)
            .file_stem()
            .and_then(|stem| stem.to_str())
        {
            Some("athenaeum") => Flavor::Legacy,
            _ => Flavor::Core,
        };
        Self { executable, flavor }
    }

    fn arguments(&self, workspace: &str) -> Vec<OsString> {
        match self.flavor {
            Flavor::Core => vec!["bridge".into(), "--workspace".into(), workspace.into()],
            Flavor::Legacy => vec!["-C".into(), workspace.into(), "bridge".into()],
        }
    }

    fn name(&self) -> String {
        self.executable.to_string_lossy().into_owned()
    }
}

/// The executables to try, best first. `reads` allows the pre-split command as
/// a last resort.
fn candidates(reads: bool) -> Vec<Launch> {
    for variable in ["ATHENAEUM_CORE_BIN", "ATHENAEUM_CLI_BIN"] {
        if let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            return vec![Launch::new(value)];
        }
    }
    let mut launches = Vec::new();
    let name = if cfg!(windows) {
        "athenaeum-core.exe"
    } else {
        "athenaeum-core"
    };
    // Where a bundled sidecar is installed: next to the app's own executable.
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|directory| directory.join(name)))
        .filter(|path| path.is_file())
    {
        launches.push(Launch::new(beside));
    }
    launches.push(Launch::new("athenaeum-core"));
    if reads {
        launches.push(Launch::new("athenaeum"));
    }
    launches
}

/// What one attempt came to.
enum Attempt {
    /// The executable could not be started; the next candidate may be tried.
    Unavailable(String),
    /// The executable ran, so its outcome is the answer.
    Finished(Result<Value, String>),
}

fn read_capped(reader: impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let _ = reader.take(cap as u64 + 1).read_to_end(&mut bytes);
    let truncated = bytes.len() > cap;
    (bytes, truncated)
}

fn exchange(
    launch: &Launch,
    workspace: &str,
    payload: &[u8],
    request_id: &str,
    method: &str,
) -> Attempt {
    let mut command = Command::new(&launch.executable);
    command
        .args(launch.arguments(workspace))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A console program started from a window app would flash a console.
        command.creation_flags(0x0800_0000);
    }
    match command.spawn() {
        Ok(mut child) => Attempt::Finished(finish(&mut child, payload, request_id, method)),
        Err(error) => Attempt::Unavailable(format!("could not start {}: {error}", launch.name())),
    }
}

fn finish(
    child: &mut Child,
    payload: &[u8],
    request_id: &str,
    method: &str,
) -> Result<Value, String> {
    let stdout = child.stdout.take().ok_or("bridge stdout was unavailable")?;
    let stderr = child.stderr.take().ok_or("bridge stderr was unavailable")?;
    // Each pipe is drained on its own thread, so a chatty child can never
    // block on a full pipe while this one waits for it to exit.
    let out = std::thread::spawn(move || read_capped(stdout, MAX_RESPONSE_BYTES));
    let err = std::thread::spawn(move || read_capped(stderr, MAX_DIAGNOSTIC_BYTES));
    if let Some(mut stdin) = child.stdin.take() {
        // A child that exits before reading its input explains itself on
        // stdout; the failed write adds nothing. Dropping `stdin` ends the input.
        let _ = stdin.write_all(payload);
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the local core did not answer within {} seconds",
                    TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("bridge failed: {error}")),
        }
    }
    let (output, truncated) = out
        .join()
        .map_err(|_| "bridge output reader failed".to_string())?;
    let (diagnostics, _) = err.join().unwrap_or_default();
    if truncated {
        return Err("bridge response is too large".to_string());
    }
    let reply: BridgeResponseEnvelope = serde_json::from_slice(&output).map_err(|error| {
        let detail = String::from_utf8_lossy(&diagnostics);
        let detail = detail.trim();
        if detail.is_empty() {
            format!("bridge returned invalid JSON: {error}")
        } else {
            format!("bridge returned invalid JSON ({error}): {detail}")
        }
    })?;
    if reply.protocol_version != BRIDGE_PROTOCOL_VERSION
        || reply.request_id.as_deref() != Some(request_id)
    {
        return Err("bridge protocol version or request id mismatch".to_string());
    }
    match reply.outcome {
        BridgeOutcome::Ok { response } if response.method == method => {
            serde_json::to_value(response).map_err(|error| error.to_string())
        }
        BridgeOutcome::Ok { .. } => Err("bridge answered a different method".to_string()),
        BridgeOutcome::Error { error } => Err(format!("{}: {}", error.code, error.message)),
    }
}

#[tauri::command]
pub async fn core_bridge(workspace: String, request: Value) -> Result<Value, String> {
    let parsed: BridgeRequest = serde_json::from_value(request)
        .map_err(|error| format!("invalid bridge request: {error}"))?;
    let method = parsed.method().to_string();
    // Retrieval may fall back to the older index. The proposal queue may not: it belongs to the
    // core, and a queue listed from one place and accepted into another would be worse than none.
    let reads = matches!(
        method.as_str(),
        "context" | "search" | "open" | "navigate" | "related" | "evidence" | "history"
    );
    let request_id = uuid::Uuid::new_v4().to_string();
    let envelope = BridgeRequestEnvelope {
        protocol_version: BRIDGE_PROTOCOL_VERSION,
        request_id: Some(request_id.clone()),
        request: parsed,
    };
    let payload = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    if payload.len() > MAX_REQUEST_BYTES {
        return Err("bridge request is too large".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let mut unavailable = Vec::new();
        for launch in candidates(reads) {
            match exchange(&launch, &workspace, &payload, &request_id, &method) {
                Attempt::Finished(result) => return result,
                Attempt::Unavailable(reason) => unavailable.push(reason),
            }
        }
        Err(format!(
            "the local core is not installed ({}); install athenaeum-core or set ATHENAEUM_CORE_BIN",
            unavailable.join("; ")
        ))
    })
    .await
    .map_err(|error| format!("bridge task failed: {error}"))?
}
