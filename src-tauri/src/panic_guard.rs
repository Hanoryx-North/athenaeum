//! Panic-to-error boundary for Tauri commands.
//!
//! **Provenance:** the synchronous half of `llm_wiki/src-tauri/src/panic_guard.rs`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_an_error_naming_the_command() {
        let result: Result<(), String> = run_guarded("do_thing", || panic!("boom"));
        let message = result.unwrap_err();
        assert!(message.contains("do_thing"), "got {message}");
        assert!(message.contains("boom"), "got {message}");
    }

    #[test]
    fn success_passes_straight_through() {
        let result = run_guarded("do_thing", || Ok(7));
        assert_eq!(result, Ok(7));
    }
}
