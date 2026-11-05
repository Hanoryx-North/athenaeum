//! Explicit CLI orchestration. No shell strings, background jobs, or bridge exposure.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use athenaeum_core::portable::portable_error;
use athenaeum_core::Result;
use serde::Serialize;
use serde_json::json;

use crate::Outcome;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub component: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl Step {
    pub fn new(component: &str, cwd: &Path, program: &str, args: &[&str]) -> Self {
        Self {
            component: component.into(),
            cwd: cwd.into(),
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
        }
    }
}

pub fn executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    let extensions = if cfg!(windows) {
        vec![".exe", ".com", ".cmd", ".bat", ""]
    } else {
        vec![""]
    };
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn run_steps(json_out: bool, dry_run: bool, steps: &[Step]) -> Result<Outcome> {
    run_steps_inner(json_out, dry_run, steps, true)
}

/// Build as part of another command: all child output goes to stderr, with no report.
pub fn run_build_steps(steps: &[Step]) -> Result<Outcome> {
    run_steps_inner(true, false, steps, false)
}

fn run_steps_inner(json_out: bool, dry_run: bool, steps: &[Step], report: bool) -> Result<Outcome> {
    if steps.is_empty() {
        return Err(portable_error(
            "unsupported_task",
            "No executable task for the selected components; wiki dev tasks lists supported tasks",
        ));
    }
    if dry_run {
        crate::emit(json_out, &json!({"dryRun": true, "steps": steps}), || {
            for step in steps {
                println!(
                    "[{}] {} {:?} (in {})",
                    step.component,
                    step.program,
                    step.args,
                    step.cwd.display()
                );
            }
            Ok(())
        })?;
        return Ok(Outcome::Ok);
    }
    // Validate every prerequisite before running the first mutating step.
    let programs = steps
        .iter()
        .map(|step| {
            if !step.cwd.is_dir() {
                return Err(portable_error(
                    "source_not_found",
                    format!("Missing {}", step.cwd.display()),
                ));
            }
            executable(&step.program).ok_or_else(|| {
                portable_error(
                    "missing_tool",
                    format!(
                        "{} is not installed or on PATH (needed by {})",
                        step.program, step.component
                    ),
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut reports = Vec::new();
    let mut ok = true;
    for (step, program) in steps.iter().zip(programs) {
        eprintln!("[{}] {} {:?}", step.component, step.program, step.args);
        let start = Instant::now();
        let status = Command::new(program)
            .args(&step.args)
            .current_dir(&step.cwd)
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("ATHENAEUM_BIN", std::env::current_exe()?)
            .stdout(if json_out {
                Stdio::from(std::io::stderr())
            } else {
                Stdio::inherit()
            })
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit())
            .status()?;
        reports.push(json!({"step": step, "ok": status.success(), "exitCode": status.code(), "durationMs": start.elapsed().as_millis()}));
        if !status.success() {
            ok = false;
            break;
        }
    }
    if json_out && report {
        println!(
            "{}",
            json!({"ok": ok, "steps": reports, "planned": steps.len(), "executed": reports.len()})
        );
    }
    Ok(if ok {
        Outcome::Ok
    } else {
        Outcome::ProblemsFound
    })
}

pub fn launch(json_out: bool, cwd: &Path, kind: &str, args: &[String]) -> Result<Outcome> {
    if json_out {
        return Err(portable_error(
            "invalid_request",
            "agent and mcp stream their own protocols; omit --json",
        ));
    }
    let (env, name) = if kind == "agent" {
        ("ATHENAEUM_AGENT_BIN", "athenaeum-agent")
    } else {
        ("ATHENAEUM_INTEGRATIONS", "athenaeum-integrations")
    };
    let configured = std::env::var_os(env).map(PathBuf::from);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let program = configured
        .or_else(|| {
            beside.as_ref().and_then(|dir| {
                let candidate = dir.join(if cfg!(windows) {
                    if kind == "agent" {
                        format!("{name}.exe")
                    } else {
                        format!("{name}.cmd")
                    }
                } else {
                    name.into()
                });
                candidate.is_file().then_some(candidate)
            })
        })
        .or_else(|| {
            if !cfg!(debug_assertions) {
                return None;
            }
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let candidate = if kind == "agent" {
                root.join("target/native-runtime/debug")
                    .join(if cfg!(windows) {
                        "athenaeum-agent.exe"
                    } else {
                        "athenaeum-agent"
                    })
            } else {
                root.join("packages/integrations/dist/athenaeum-integrations.mjs")
            };
            candidate.is_file().then_some(candidate)
        })
        .or_else(|| executable(name))
        .ok_or_else(|| {
            portable_error(
                "missing_tool",
                format!("{name} is not installed; run the Athenaeum installer or set {env}"),
            )
        })?;
    let mut command = if program
        .extension()
        .is_some_and(|ext| ext == "mjs" || ext == "js")
    {
        let private_node = beside.as_ref().map(|dir| {
            dir.join(if cfg!(windows) {
                "../node/node.exe"
            } else {
                "../node/bin/node"
            })
        });
        let node = private_node
            .filter(|p| p.is_file())
            .or_else(|| executable("node"))
            .ok_or_else(|| {
                portable_error(
                    "missing_tool",
                    "Node is needed to run the integration bundle",
                )
            })?;
        let mut command = Command::new(node);
        command.arg(program);
        command
    } else {
        Command::new(program)
    };
    if kind == "mcp" {
        command.arg("mcp");
    }
    let project = athenaeum_core::prelude::Workspace::discover(cwd)
        .and_then(|ws| ws.project_root().map(Path::to_path_buf))
        .unwrap_or_else(|| cwd.into());
    let status = command
        .args(args)
        .current_dir(project)
        .env("ATHENAEUM_BIN", std::env::current_exe()?)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(if status.success() {
        Outcome::Ok
    } else {
        Outcome::ProblemsFound
    })
}
