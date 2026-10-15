//! `athenaeum setup` and `athenaeum doctor`: the two commands that make an install usable.
//!
//! Setup does the whole first-run job in one pass, and every step is safe to repeat: give this
//! project a knowledge base and an identity, index it, optionally connect it to the hosted
//! Library, and wire the person's coding agents to it. It owns none of the agent wiring — that is
//! `athenaeum-integrations`, which this command finds and runs — and it owns none of the Library
//! protocol, which is the same client `athenaeum library` and `athenaeum sync` use.
//!
//! Nothing here contacts a network unless the person asked for the Library, and a step that fails
//! never undoes the ones before it: a half-finished setup is a working local install.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};

use athenaeum_core::portable::manifest::{default_name, RepositoryManifest};
use athenaeum_core::portable::sync::SyncState;
use athenaeum_core::prelude::*;
use athenaeum_core::status::{self, IndexScope};
use clap::Args;
use serde::Serialize;
use serde_json::{json, Value};

use crate::library::credentials;
use crate::portable_cmd;
use crate::Outcome;

/// The hosted Library `athenaeum setup --library` connects to when no other is named: the address the
/// Library site itself gives people for `athenaeum library login --server`. (Kept here rather than in
/// `library.rs` so that file stays byte-identical to the one the preserved tests are pinned to.)
pub const DEFAULT_SERVER: &str = "https://wiki.hanoryx.com";

#[derive(Args)]
pub struct SetupArgs {
    /// Connect this project to the hosted Library: sign in once in a browser, create a private
    /// repository for it and link them. Give a URL for a Library other than the default.
    #[arg(long, value_name = "URL", num_args = 0..=1, default_missing_value = DEFAULT_SERVER)]
    pub library: Option<String>,
    /// Do not offer to connect a Library.
    #[arg(long, conflicts_with = "library")]
    pub no_library: bool,
    /// Link an existing Library repository (`owner/name`) instead of creating one.
    #[arg(long, value_name = "OWNER/NAME", requires = "library")]
    pub library_remote: Option<String>,
    /// The name for the new Library repository. Defaults to this project's name.
    #[arg(long, value_name = "NAME", requires = "library")]
    pub library_name: Option<String>,
    /// Let the end of a coding-agent turn sync accepted pages with the Library. Off unless asked
    /// for, and left as it was when neither this nor `--no-auto-sync` is given.
    #[arg(long)]
    pub auto_sync: bool,
    /// Turn that off again.
    #[arg(long, conflicts_with = "auto_sync")]
    pub no_auto_sync: bool,
    /// Which coding agents to connect: claude, codex, cursor. Default: the ones installed.
    #[arg(long, value_delimiter = ',', value_name = "AGENT")]
    pub agents: Vec<String>,
    /// Do not touch any coding agent's settings.
    #[arg(long, conflicts_with = "agents")]
    pub no_agents: bool,
    /// How actively agents are asked to record knowledge: off, instruct or nudge. Left as it was
    /// when omitted (the first time: instruct).
    #[arg(long, value_parser = ["off", "instruct", "nudge"])]
    pub capture: Option<String>,
    /// Where `athenaeum-integrations` is (a launcher, or its `.mjs` bundle). Also read from
    /// `ATHENAEUM_INTEGRATIONS`.
    #[arg(long, value_name = "PATH")]
    pub integrations: Option<PathBuf>,
    /// Skip the first index of the project's files, symbols and Git history.
    #[arg(long)]
    pub skip_index: bool,
    /// Never ask a question; use the flags and the defaults.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// One line of the report. `ok` means "already as wanted", `done` means "changed something".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    id: &'static str,
    status: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

impl Step {
    fn new(id: &'static str, status: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            status,
            detail: detail.into(),
            fix: None,
        }
    }
    fn failed(id: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            id,
            status: "failed",
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
}

pub fn setup(json: bool, cwd: &Path, explicit_dir: bool, args: &SetupArgs) -> Result<Outcome> {
    let interactive = !args.yes && !json && std::io::stdin().is_terminal();
    let mut steps: Vec<Step> = Vec::new();
    let mut agents: Value = Value::Null;
    let progress = |line: &str| {
        if !json {
            println!("{line}");
        }
    };

    // -- the project ----------------------------------------------------------------------------
    let existing = Workspace::discover(cwd);
    let project: PathBuf = match &existing {
        Some(ws) => ws.project_root().unwrap_or(cwd).to_path_buf(),
        None => cwd.to_path_buf(),
    };
    if existing.is_none() && !explicit_dir && is_home_or_root(&project) {
        steps.push(Step::failed(
            "knowledge-base",
            format!(
                "{} is your home folder (or the filesystem root), not a project.",
                project.display()
            ),
            "cd into the project you want a knowledge base for, or pass -C <project>.",
        ));
        return finish(json, &project, steps, agents, None);
    }

    progress(&format!("Setting up Athenaeum for {}", project.display()));
    match prepare_project(&project, existing.is_some()) {
        Ok(found) => steps.extend(found),
        Err(error) => {
            steps.push(Step::failed(
                "knowledge-base",
                error.to_string(),
                "Check that the folder is writable.",
            ));
            return finish(json, &project, steps, agents, None);
        }
    }

    if args.skip_index {
        steps.push(Step::new(
            "index",
            "skipped",
            "skipped (--skip-index); run `athenaeum index --scope all` when you like",
        ));
    } else {
        steps.push(index_project(&project));
    }

    // -- the Library ----------------------------------------------------------------------------
    let mut want_library = args.library.clone();
    let linked_already = SyncState::load(&project).ok().flatten();
    if want_library.is_none()
        && !args.no_library
        && linked_already.is_none()
        && interactive
        && ask(
            &format!("Back this wiki up and share it through the Library ({DEFAULT_SERVER})? You sign in once in a browser"),
            true,
        )
    {
        want_library = Some(DEFAULT_SERVER.to_string());
    }
    let mut auto_sync: Option<bool> = if args.auto_sync {
        Some(true)
    } else if args.no_auto_sync {
        Some(false)
    } else {
        None
    };
    if let Some(server) = &want_library {
        let (step, linked) = connect_library(&project, server, args, progress);
        steps.push(step);
        if linked && auto_sync.is_none() && interactive {
            auto_sync = Some(ask(
                "Sync accepted pages automatically at the end of each coding-agent turn?",
                false,
            ));
        }
    } else if let Some(state) = &linked_already {
        steps.push(Step::new(
            "library",
            "ok",
            format!(
                "already linked to {} on {}",
                state
                    .remote_name
                    .clone()
                    .unwrap_or_else(|| state.remote_id.clone()),
                state.server
            ),
        ));
    } else {
        steps.push(Step::new(
            "library",
            "skipped",
            "not connected (optional): `athenaeum setup --library` connects it any time",
        ));
    }

    // -- the coding agents ----------------------------------------------------------------------
    if args.no_agents {
        steps.push(Step::new(
            "agents",
            "skipped",
            "left your coding agents alone (--no-agents)",
        ));
    } else {
        let (step, report) = connect_agents(args, auto_sync);
        steps.push(step);
        agents = report;
    }

    finish(json, &project, steps, agents, auto_sync)
}

fn is_home_or_root(dir: &Path) -> bool {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    dir.parent().is_none() || home.is_some_and(|home| same(dir, &home))
}

/// The knowledge base, the repository identity, and a `.gitignore` line for derived state.
fn prepare_project(project: &Path, existed: bool) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let workspace = Workspace::init(project)?;
    workspace.open()?;
    steps.push(if existed {
        Step::new(
            "knowledge-base",
            "ok",
            format!("using the knowledge base in {}", workspace.root().display()),
        )
    } else {
        Step::new(
            "knowledge-base",
            "done",
            format!("created {}", workspace.root().display()),
        )
    });

    let (manifest, created) = RepositoryManifest::read_or_init(project, &default_name(project))?;
    steps.push(if created {
        Step::new(
            "identity",
            "done",
            format!(
                "{} is now repository {:?} ({})",
                athenaeum_core::portable::manifest::MANIFEST_FILE,
                manifest.name,
                manifest.repository_id
            ),
        )
    } else {
        Step::new(
            "identity",
            "ok",
            format!(
                "repository {:?} ({})",
                manifest.name, manifest.repository_id
            ),
        )
    });

    steps.push(ignore_local_state(project));
    Ok(steps)
}

const IGNORE_LINE: &str = ".athenaeum/";

/// In a Git project, keep Athenaeum's derived state (the index, caches, exports) out of commits.
/// The wiki itself (`wiki/`, `athenaeum.json`) is meant to be committed, so it is not ignored.
fn ignore_local_state(project: &Path) -> Step {
    if !project.join(".git").exists() {
        return Step::new(
            "gitignore",
            "skipped",
            "not a Git project; nothing to ignore",
        );
    }
    let file = project.join(".gitignore");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    if text.lines().any(|line| {
        matches!(
            line.trim(),
            ".athenaeum" | ".athenaeum/" | "/.athenaeum" | "/.athenaeum/"
        )
    }) {
        return Step::new("gitignore", "ok", ".athenaeum/ is already ignored by Git");
    }
    let mut next = text;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("\n# Athenaeum's local state: the index and caches, rebuilt from wiki/ by `athenaeum index-vault`.\n");
    next.push_str(IGNORE_LINE);
    next.push('\n');
    match std::fs::write(&file, next) {
        Ok(()) => Step::new("gitignore", "done", "added .athenaeum/ to .gitignore"),
        Err(error) => Step::failed(
            "gitignore",
            format!("could not update .gitignore: {error}"),
            "Add `.athenaeum/` to .gitignore yourself.",
        ),
    }
}

fn index_project(project: &Path) -> Step {
    let result = Workspace::discover(project)
        .ok_or_else(|| Error::NoWorkspace {
            path: project.display().to_string(),
        })
        .and_then(|workspace| {
            let mut store = workspace.open()?;
            status::index_scopes(&mut store, &workspace, IndexScope::All, &[])
        });
    match result {
        Ok(outcome) => {
            let summary = serde_json::to_value(&outcome).unwrap_or(Value::Null);
            let count = |key: &str| summary.get(key).and_then(Value::as_u64).unwrap_or(0);
            let mut text = format!(
                "indexed {} wiki page(s), {} project file(s) and {} symbol(s)",
                count("vaultPages"),
                count("repositoryFiles"),
                count("symbols")
            );
            if summary.get("incomplete").and_then(Value::as_bool) == Some(true) {
                text.push_str("; the project is large, so a budget ended the scan early (run `athenaeum index --scope all` to continue)");
            }
            Step::new("index", "done", text)
        }
        Err(error) => Step::failed(
            "index",
            error.to_string(),
            "Run `athenaeum index --scope all` to see the full error.",
        ),
    }
}

fn connect_library(
    project: &Path,
    server: &str,
    args: &SetupArgs,
    progress: impl Fn(&str),
) -> (Step, bool) {
    if let Ok(Some(state)) = SyncState::load(project) {
        return (
            Step::new(
                "library",
                "ok",
                format!(
                    "already linked to {} on {}",
                    state.remote_name.unwrap_or(state.remote_id),
                    state.server
                ),
            ),
            true,
        );
    }
    progress(&format!("Connecting to the Library at {server}"));
    let (server, username, signed_in) = match portable_cmd::ensure_signed_in(server) {
        Ok(found) => found,
        Err(error) => {
            return (
                Step::failed(
                    "library",
                    format!("could not sign in to {server}: {error}"),
                    format!("Check your network, then run `athenaeum setup --library {server}` again. Everything else works offline."),
                ),
                false,
            )
        }
    };
    let name = args
        .library_name
        .clone()
        .unwrap_or_else(|| default_name(project));
    let linked = match &args.library_remote {
        Some(remote) => {
            portable_cmd::link_repository(project, &server, Some(remote), None, false, None)
        }
        None => portable_cmd::link_repository(project, &server, None, Some(&name), false, None),
    };
    match linked {
        Ok((repository, _)) => (
            Step::new(
                "library",
                "done",
                format!(
                    "{} as {username}; linked to {} ({}), private. Nothing is sent until you sync",
                    if signed_in { "signed in" } else { "already signed in" },
                    repository.name,
                    repository.id
                ),
            ),
            true,
        ),
        Err(error) => (
            Step::failed(
                "library",
                format!("signed in as {username} but could not link: {error}"),
                "Pick another name with --library-name, or link an existing repository with --library-remote owner/name.",
            ),
            false,
        ),
    }
}

// -- the agent integration ----------------------------------------------------------------------

/// How to run `athenaeum-integrations`: a program, plus any leading arguments it needs.
struct Integrations {
    program: PathBuf,
    leading: Vec<PathBuf>,
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    }
}

fn find_integrations(explicit: Option<&Path>) -> Option<Integrations> {
    let node = || -> PathBuf {
        if let Some(node) = std::env::var_os("ATHENAEUM_NODE") {
            return PathBuf::from(node);
        }
        if let Some(bin) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            for private in [
                bin.join("..").join("node").join("bin").join("node"),
                bin.join("..").join("node").join("node.exe"),
            ] {
                if private.is_file() {
                    return private;
                }
            }
        }
        PathBuf::from("node")
    };
    let launch = |path: &Path| -> Option<Integrations> {
        if !path.is_file() {
            return None;
        }
        let script = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("mjs" | "js" | "cjs")
        );
        Some(if script {
            Integrations {
                program: node(),
                leading: vec![path.to_path_buf()],
            }
        } else {
            Integrations {
                program: path.to_path_buf(),
                leading: Vec::new(),
            }
        })
    };
    let from_env = std::env::var_os("ATHENAEUM_INTEGRATIONS").map(PathBuf::from);
    if let Some(path) = explicit.map(Path::to_path_buf).or(from_env) {
        return launch(&path);
    }
    let beside = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .map(|dir| dir.join(exe_name("athenaeum-integrations")))
    });
    if let Some(found) = beside.as_deref().and_then(launch) {
        return Some(found);
    }
    // Last resort: whatever is on PATH.
    let name = exe_name("athenaeum-integrations");
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&name))
            .find_map(|candidate| launch(&candidate))
    })
}

fn run_integrations(found: &Integrations, extra: &[String]) -> std::result::Result<Value, String> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| e.to_string())?;
    let mut command = Process::new(&found.program);
    command
        .args(&found.leading)
        .args(extra)
        .arg("--athenaeum")
        .arg(&exe)
        .arg("--json");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command
        .output()
        .map_err(|e| format!("could not run {}: {e}", found.program.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Value>(text.trim()).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim();
        if reason.is_empty() {
            "athenaeum-integrations gave no answer".to_string()
        } else {
            reason.to_string()
        }
    })
}

fn connect_agents(args: &SetupArgs, auto_sync: Option<bool>) -> (Step, Value) {
    let Some(found) = find_integrations(args.integrations.as_deref()) else {
        return (
            Step::failed(
                "agents",
                "the agent integration (athenaeum-integrations) is not installed, so Claude Code and Codex were not connected",
                "Run the Athenaeum installer, or pass --integrations <path> to the athenaeum-integrations launcher.",
            ),
            Value::Null,
        );
    };
    let mut extra = vec!["connect".to_string()];
    if let Some(capture) = &args.capture {
        extra.push("--capture".into());
        extra.push(capture.clone());
    }
    if !args.agents.is_empty() {
        extra.push("--agent".into());
        extra.push(args.agents.join(","));
    }
    match auto_sync {
        Some(true) => extra.push("--sync".into()),
        Some(false) => extra.push("--no-sync".into()),
        None => {}
    }
    match run_integrations(&found, &extra) {
        Ok(report) => {
            let list = report
                .get("agents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let failed = list.iter().any(|a| {
                matches!(
                    a.get("status").and_then(Value::as_str),
                    Some("error" | "conflict")
                )
            });
            let names: Vec<String> = list
                .iter()
                .filter(|a| {
                    matches!(
                        a.get("status").and_then(Value::as_str),
                        Some("connected" | "unchanged")
                    )
                })
                .filter_map(|a| a.get("agent").and_then(Value::as_str).map(str::to_string))
                .collect();
            let detail = if failed {
                list.iter()
                    .filter(|a| {
                        matches!(
                            a.get("status").and_then(Value::as_str),
                            Some("error" | "conflict")
                        )
                    })
                    .filter_map(|a| a.get("detail").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else if names.is_empty() {
                report
                    .get("note")
                    .and_then(Value::as_str)
                    .unwrap_or("no coding agent found")
                    .to_string()
            } else {
                format!("connected {}", names.join(", "))
            };
            let step = if failed {
                Step::failed(
                    "agents",
                    detail,
                    "Fix the file named above, then run `athenaeum setup` again.",
                )
            } else if names.is_empty() {
                Step::new("agents", "skipped", detail)
            } else {
                let changed = list
                    .iter()
                    .any(|a| a.get("status").and_then(Value::as_str) == Some("connected"));
                Step::new("agents", if changed { "done" } else { "ok" }, detail)
            };
            (step, report)
        }
        Err(reason) => (
            Step::failed(
                "agents",
                format!("connecting agents failed: {reason}"),
                "Run `athenaeum doctor` for details.",
            ),
            Value::Null,
        ),
    }
}

fn finish(
    json_out: bool,
    project: &Path,
    steps: Vec<Step>,
    agents: Value,
    auto_sync: Option<bool>,
) -> Result<Outcome> {
    let failed = steps.iter().any(|s| s.status == "failed");
    let next: Vec<&str> = if failed {
        vec!["Fix the step marked FAIL above, then run `athenaeum setup` again. It is safe to repeat."]
    } else {
        vec![
            "Open Claude Code or Codex in this project and work as usual: it recalls what the wiki knows and proposes what it learns.",
            "Review what agents propose in the Athenaeum app (Review), or with `athenaeum proposals`; accepting writes the page.",
            "`athenaeum sync` shares accepted pages with the Library once it is connected. `athenaeum doctor` checks every link.",
        ]
    };
    if json_out {
        let value = json!({ "ok": !failed, "project": project.display().to_string(), "steps": steps, "agents": agents, "autoSync": auto_sync, "next": next });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!();
        for step in &steps {
            let mark = match step.status {
                "done" | "ok" => "ok  ",
                "failed" => "FAIL",
                _ => "-   ",
            };
            println!("{mark} {}: {}", step.id, step.detail);
            if let Some(fix) = &step.fix {
                println!("       fix: {fix}");
            }
        }
        println!();
        for line in &next {
            println!("  {line}");
        }
    }
    Ok(if failed {
        Outcome::ProblemsFound
    } else {
        Outcome::Ok
    })
}

fn ask(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint} ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return default_yes;
    }
    match line.trim().to_ascii_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    }
}

// -- doctor -------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    id: String,
    ok: bool,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

fn check(id: &str, ok: bool, detail: impl Into<String>, fix: Option<&str>) -> Check {
    Check {
        id: id.to_string(),
        ok,
        detail: detail.into(),
        fix: if ok { None } else { fix.map(str::to_string) },
    }
}

pub fn doctor(json_out: bool, cwd: &Path, integrations: Option<&Path>) -> Result<Outcome> {
    let mut checks = vec![check(
        "engine",
        true,
        format!("athenaeum {}", env!("CARGO_PKG_VERSION")),
        None,
    )];

    match Workspace::discover(cwd) {
        None => checks.push(check(
            "knowledge-base",
            true,
            "no knowledge base here (agents work normally); `athenaeum setup` creates one",
            None,
        )),
        Some(workspace) => {
            let detail = match workspace
                .open()
                .and_then(|store| status::index_status(&store))
            {
                Ok(index) => (
                    true,
                    format!(
                        "{}{}",
                        workspace.root().display(),
                        if index.stale {
                            " (index is behind the files; run `athenaeum index-vault`)"
                        } else {
                            ""
                        }
                    ),
                ),
                Err(error) => (false, error.to_string()),
            };
            checks.push(check(
                "knowledge-base",
                detail.0,
                detail.1,
                Some("Run `athenaeum index-vault` to rebuild the index from the wiki."),
            ));
            let project = workspace.project_root().unwrap_or(cwd).to_path_buf();
            match SyncState::load(&project) {
                Ok(Some(state)) => {
                    let signed_in = credentials::token_for(&state.server).is_some();
                    checks.push(check(
                        "library",
                        signed_in,
                        format!(
                            "linked to {} on {}{}",
                            state.remote_name.unwrap_or(state.remote_id),
                            state.server,
                            if signed_in { "" } else { " but not signed in" }
                        ),
                        Some("Run `athenaeum library login`."),
                    ));
                }
                Ok(None) => checks.push(check(
                    "library",
                    true,
                    "not linked to a Library (optional): `athenaeum setup --library`",
                    None,
                )),
                Err(error) => checks.push(check(
                    "library",
                    false,
                    error.to_string(),
                    Some("Check .athenaeum/sync.json."),
                )),
            }
        }
    }

    match find_integrations(integrations) {
        None => checks.push(check(
            "integrations",
            false,
            "athenaeum-integrations is not installed, so coding agents cannot reach the knowledge base",
            Some("Run the Athenaeum installer again."),
        )),
        Some(found) => match run_integrations(&found, &["doctor".to_string()]) {
            Ok(report) => {
                for c in report.get("checks").and_then(Value::as_array).into_iter().flatten() {
                    // The integration repeats the engine and knowledge-base checks; keep only what is new.
                    let id = c.get("id").and_then(Value::as_str).unwrap_or("?");
                    if matches!(id, "athenaeum" | "athenaeum-runs" | "knowledge-base" | "library") {
                        continue;
                    }
                    checks.push(Check {
                        id: id.to_string(),
                        ok: c.get("ok").and_then(Value::as_bool).unwrap_or(false),
                        detail: c.get("detail").and_then(Value::as_str).unwrap_or("").to_string(),
                        fix: c.get("fix").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
            Err(reason) => checks.push(check("integrations", false, reason, Some("Run the Athenaeum installer again."))),
        },
    }

    let ok = checks.iter().all(|c| c.ok);
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "ok": ok, "checks": checks }))?
        );
    } else {
        for c in &checks {
            println!(
                "{} {}: {}",
                if c.ok { "ok  " } else { "FAIL" },
                c.id,
                c.detail
            );
            if let Some(fix) = &c.fix {
                println!("       fix: {fix}");
            }
        }
        println!(
            "{}",
            if ok {
                "\nEverything is connected."
            } else {
                "\nSomething needs attention (see above)."
            }
        );
    }
    Ok(if ok {
        Outcome::Ok
    } else {
        Outcome::ProblemsFound
    })
}
