//! Machine-readable discovery comes from clap, never a second hand-maintained list.
use athenaeum_core::Result;
use clap::CommandFactory;
use serde_json::{json, Value};

use crate::{Cli, Outcome};

fn describe(command: &clap::Command, prefix: &str, rows: &mut Vec<Value>) {
    let path = if prefix.is_empty() {
        command.get_name().to_string()
    } else {
        format!("{prefix} {}", command.get_name())
    };
    if command.is_hide_set() || command.get_name() == "help" {
        return;
    }
    rows.push(json!({
        "command": path,
        "about": command.get_about().map(ToString::to_string),
        "aliases": command.get_visible_aliases().collect::<Vec<_>>(),
        "options": command.get_arguments().map(|arg| json!({
            "name": arg.get_id().as_str(), "long": arg.get_long(), "short": arg.get_short(),
            "required": arg.is_required_set(), "help": arg.get_help().map(ToString::to_string),
            "values": arg.get_possible_values().iter().map(|v| v.get_name()).collect::<Vec<_>>(),
            "defaults": arg.get_default_values().iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()
    }));
    for child in command.get_subcommands() {
        describe(child, &path, rows);
    }
}

pub fn run(json_out: bool) -> Result<Outcome> {
    let mut command = Cli::command();
    command.build();
    let mut rows = Vec::new();
    describe(&command, "", &mut rows);
    crate::emit(
        json_out,
        &json!({"schemaVersion": 1, "commands": rows}),
        || {
            for row in &rows {
                println!(
                    "{:<34} {}",
                    row["command"].as_str().unwrap_or_default(),
                    row["about"].as_str().unwrap_or_default()
                );
            }
            Ok(())
        },
    )?;
    Ok(Outcome::Ok)
}
