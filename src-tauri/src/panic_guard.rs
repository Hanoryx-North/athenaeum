//! Panic-to-error boundary for Tauri commands.
//!
//! **Provenance:** `llm_wiki/src-tauri/src/panic_guard.rs`, GPL-3.0, © Yong Su.
//!
//! Letting a panic unwind through the `extern "C"` Tauri command boundary is
//! undefined behaviour. Catching it here turns a bug in one command into an
//! error message in the UI rather than a vanished application window.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run a command body, converting any panic into an `Err`.
pub fn run_guarded<T, F>(label: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => Err(report(label, payload)),
    }
}

/// Run an async command body, converting any panic into an `Err`.
///
/// A panic after an await point is caught too, which a plain `catch_unwind`
/// around the future's construction would miss.
pub async fn run_guarded_async<T, Fut>(label: &str, fut: Fut) -> Result<T, String>
where
    Fut: std::future::Future<Output = Result<T, String>>,
{
    use futures::FutureExt;
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => Err(report(label, payload)),
    }
}

fn report(label: &str, payload: Box<dyn Any + Send>) -> String {
    let message = if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic".to_string()
    };
    eprintln!("[panic_guard] {label} panicked: {message}");
    format!("{label} failed: {message}")
}
