//! Authenticated access to the running app. No window automation or DOM clicking.
use crate::{config_cmd, Outcome};
use athenaeum_core::{portable::portable_error, Result};
use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Args)]
pub struct DesktopArgs {
    /// Connection descriptor override; defaults beside the desktop settings.
    #[arg(long, global = true)]
    connection: Option<PathBuf>,
    /// Include secrets in command results and live state. Default: redact.
    #[arg(long, global = true)]
    reveal: bool,
    #[command(subcommand)]
    command: DesktopCommand,
}

#[derive(Subcommand)]
enum DesktopCommand {
    /// Show or restore the app through its native API, without window automation.
    Show,
    /// Inspect the running desktop, project and frontend build.
    Status,
    /// Discover live actions and their argument signatures. Filter by name.
    Actions { filter: Option<String> },
    /// Read live store state, optionally one store (wiki, chat, research, etc.).
    State { store: Option<String> },
    /// Run a registered action with a JSON positional argument array.
    Call {
        action: String,
        #[arg(long, default_value = "[]", conflicts_with = "stdin")]
        args: String,
        /// Read the JSON argument array from stdin, keeping secrets off argv.
        #[arg(long)]
        stdin: bool,
    },
    /// Invoke a registered native command using its named JSON arguments.
    Invoke {
        command: String,
        #[arg(long, default_value = "{}", conflicts_with = "stdin")]
        args: String,
        #[arg(long)]
        stdin: bool,
    },
    /// Flush saved state and reload frontend assets without compiling Rust.
    Reload,
    /// Flush saved state and exit the app without a desktop interaction.
    Quit,
}

#[derive(Deserialize)]
struct Connection {
    url: String,
    token: String,
}

fn arguments(value: &str, stdin: bool, array: bool) -> Result<Value> {
    let mut input = value.to_owned();
    if stdin {
        input.clear();
        std::io::stdin().read_to_string(&mut input)?;
    }
    let parsed: Value = serde_json::from_str(&input)?;
    if (array && !parsed.is_array()) || (!array && !parsed.is_object()) {
        return Err(portable_error(
            "invalid_request",
            if array {
                "--args must be a JSON array"
            } else {
                "--args must be a JSON object"
            },
        ));
    }
    Ok(parsed)
}

pub fn run(json_out: bool, args: &DesktopArgs) -> Result<Outcome> {
    let descriptor = args
        .connection
        .clone()
        .unwrap_or(config_cmd::path(None)?.with_file_name("desktop-control.json"));
    let bytes = std::fs::read(&descriptor).map_err(|e| {
        portable_error(
            "desktop_unavailable",
            format!(
                "Cannot read {}: {e}. Start the current desktop with wiki start --dev.",
                descriptor.display()
            ),
        )
    })?;
    let connection: Connection = serde_json::from_slice(&bytes)?;
    // Never send a local capability token to an arbitrary host or through a proxy.
    let url = reqwest::Url::parse(&connection.url)
        .map_err(|e| portable_error("invalid_config", e.to_string()))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(portable_error(
            "invalid_config",
            "Desktop descriptor must name an IPv4 loopback HTTP endpoint",
        ));
    }
    let (operation, payload) = match &args.command {
        DesktopCommand::Show => ("show", json!({})),
        DesktopCommand::Status => ("status", json!({})),
        DesktopCommand::Actions { filter } => ("actions", json!({"filter": filter})),
        DesktopCommand::State { store } => ("state", json!({"store": store})),
        DesktopCommand::Call {
            action,
            args,
            stdin,
        } => (
            "call",
            json!({"action": action, "args": arguments(args, *stdin, true)?}),
        ),
        DesktopCommand::Invoke {
            command,
            args,
            stdin,
        } => (
            "invoke",
            json!({"command": command, "args": arguments(args, *stdin, false)?}),
        ),
        DesktopCommand::Reload => ("call", json!({"action": "app.reload", "args": []})),
        DesktopCommand::Quit => ("call", json!({"action": "app.quit", "args": []})),
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(125))
        .build()
        .map_err(|e| portable_error("desktop_unavailable", e.to_string()))?;
    let value: Value = client.post(format!("{}/command", connection.url.trim_end_matches('/')))
        .bearer_auth(connection.token).json(&json!({"operation": operation, "payload": {"reveal": args.reveal, "request": payload}}))
        .send().map_err(|e| portable_error("desktop_unavailable", format!("Desktop request failed: {e}. Inspect status before retrying a mutation.")))?
        .error_for_status().map_err(|e| portable_error("desktop_command_failed", e.to_string()))?
        .json().map_err(|e| portable_error("desktop_command_failed", e.to_string()))?;
    if let Some(error) = value.get("error") {
        return Err(portable_error(
            "desktop_command_failed",
            error.as_str().unwrap_or("Desktop action failed"),
        ));
    }
    let result = value.get("result").unwrap_or(&value);
    crate::emit(json_out, result, || {
        println!("{}", serde_json::to_string_pretty(result)?);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}
