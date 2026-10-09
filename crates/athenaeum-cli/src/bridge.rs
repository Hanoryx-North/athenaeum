//! One-shot, language-neutral transport for the Wiki integration contract.

use std::io::{Read, Write};
use std::path::Path;

use athenaeum_core::bridge::{execute, MAX_REQUEST_BYTES};
use athenaeum_core::wiki::WikiResponseEnvelope;
use athenaeum_core::workspace::Workspace;
use athenaeum_core::Result;

/// Execute one storage-independent Wiki request over stdin/stdout.
///
/// The bridge is intentionally one-shot for the first protocol version. It is
/// easy for any runtime language to integrate, holds no server open, and can
/// become a long-lived JSONL transport later without changing request or
/// response payloads. The contract itself, including how a request is answered, lives in
/// [`athenaeum_core::bridge`], which the desktop app also calls in-process; this file is only the
/// stdin/stdout framing.
pub fn run(workspace: Option<&Workspace>, requested_path: &Path) -> Result<bool> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;

    let (response, success): (WikiResponseEnvelope, bool) =
        execute(&bytes, workspace, requested_path);

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &response)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(success)
}
