//! Source-workspace tools, deliberately separate from end-user wiki operations.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use athenaeum_core::portable::portable_error;
use athenaeum_core::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::process::{self, Step};
use crate::Outcome;

#[derive(Args)]
pub struct DevArgs {
    /// Athenaeum checkout root. Otherwise discover ancestors or WIKI_DEV_ROOT.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Print the exact plan without running it.
    #[arg(long, global = true)]
    dry_run: bool,
    #[command(subcommand)]
    command: DevCommand,
}

#[derive(Subcommand)]
enum DevCommand {
    /// List components, checkout paths, versions, and supported package scripts.
    Components,
    /// Show changes in the source checkout (read-only git status).
    Status,
    /// Check source checkout and tool prerequisites.
    Doctor,
    /// List executable package scripts and native tasks for each component.
    Tasks,
    /// Install dependencies using each component's package manager and lockfile.
    Install(Targets),
    /// Build selected components in dependency order.
    Build {
        #[command(flatten)]
        targets: Targets,
        /// Optimize native Rust builds.
        #[arg(long)]
        release: bool,
    },
    /// Typecheck, format-check, and lint selected components.
    Check(Targets),
    /// Format Rust code; package components use their declared fmt/format script.
    Fmt(Targets),
    /// Run real tests. Product suites use the central validation runner and fail on skips.
    Test {
        #[command(flatten)]
        targets: Targets,
        /// Explicit central validation step IDs, e.g. product-engine,product-local-to-cloud.
        #[arg(long, value_delimiter = ',', conflicts_with = "components")]
        suite: Vec<String>,
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Start one development server in the foreground; Ctrl+C stops it.
    Serve {
        #[arg(value_enum)]
        component: Component,
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run any existing package.json script, including packaging and reference-runtime tools.
    Run {
        #[arg(value_enum)]
        component: Component,
        script: String,
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct Targets {
    /// One or more components, or all. Omitted means all.
    #[arg(value_enum, value_delimiter = ',')]
    components: Vec<Component>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Component {
    All,
    Protocol,
    Engine,
    Cli,
    Integrations,
    Agent,
    App,
    Desktop,
    Validation,
}

const COMPONENTS: [Component; 8] = [
    Component::Protocol,
    Component::Engine,
    Component::Cli,
    Component::Integrations,
    Component::Agent,
    Component::App,
    Component::Desktop,
    Component::Validation,
];

impl Component {
    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Protocol => "protocol",
            Self::Engine => "engine",
            Self::Cli => "cli",
            Self::Integrations => "integrations",
            Self::Agent => "agent",
            Self::App => "app",
            Self::Desktop => "desktop",
            Self::Validation => "validation",
        }
    }
    fn repository(self) -> &'static str {
        match self {
            Self::Engine | Self::Cli | Self::App | Self::Desktop => "",
            Self::Protocol => "packages/protocol",
            Self::Integrations => "packages/integrations",
            Self::Agent => "packages/agent",
            Self::Validation => "validation",
            Self::All => unreachable!(),
        }
    }
    fn crate_name(self) -> Option<&'static str> {
        match self {
            Self::Engine => Some("athenaeum-core"),
            Self::Cli => Some("athenaeum-cli"),
            Self::Desktop => Some("athenaeum-app"),
            _ => None,
        }
    }
    fn manager(self) -> &'static str {
        "npm"
    }
    fn suites(self) -> &'static [&'static str] {
        match self {
            Self::Engine => &[
                "engine",
                "product-engine",
                "product-repository",
                "product-knowledge",
                "product-boundaries",
                "product-protocol",
            ],
            Self::Cli => &["product-cli", "product-engine", "product-agent-memory"],
            Self::Integrations => &["integrations", "product-mcp-desktop"],
            Self::Agent => &["native-agent-config", "agent", "product-agent-to-knowledge"],
            Self::App => &["app-frontend"],
            Self::Desktop => &["app-desktop"],
            Self::Protocol => &["protocol"],
            Self::Validation => &["product-cli"],
            Self::All => &[],
        }
    }
}

pub(crate) fn root(cwd: &Path, explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("WIKI_DEV_ROOT").map(PathBuf::from))
    {
        let root = root.canonicalize()?;
        return Ok(if root.join("athenaeum/Cargo.toml").is_file() {
            root.join("athenaeum")
        } else {
            root
        });
    }
    for ancestor in cwd.canonicalize()?.ancestors() {
        if ancestor.join("Cargo.toml").is_file()
            && ancestor.join("packages/protocol/package.json").is_file()
        {
            return Ok(ancestor.into());
        }
        if ancestor.join("athenaeum/Cargo.toml").is_file() {
            return Ok(ancestor.join("athenaeum"));
        }
    }
    Err(portable_error("source_not_found", "No Athenaeum source workspace here. Use wiki dev --root <athenaeum-checkout>, or set WIKI_DEV_ROOT."))
}

/// Compile the frontend and native app together, without creating installers.
pub(crate) fn desktop_build(root: &Path, release: bool) -> Result<Step> {
    let mut args = vec!["build".into(), "--no-bundle".into()];
    if !release {
        args.push("--debug".into());
    }
    script(root, Component::Desktop, "tauri", &args)
}

/// Share one live desktop command with `wiki start --dev` and `wiki dev serve`.
pub(crate) fn desktop_serve(root: &Path) -> Result<Step> {
    script(root, Component::Desktop, "tauri", &["dev".into()])
}

/// Build and preserve an embedded-frontend app independently of live development.
pub(crate) fn desktop_snapshot(root: &Path) -> Result<Step> {
    script(root, Component::Desktop, "desktop:build", &[])
}

fn selected(targets: &Targets) -> Vec<Component> {
    let all = targets.components.is_empty() || targets.components.contains(&Component::All);
    COMPONENTS
        .into_iter()
        .filter(|c| all || targets.components.contains(c))
        .collect()
}

fn manifest(root: &Path, c: Component) -> Result<Value> {
    let file = root.join(c.repository()).join("package.json");
    if file.is_file() {
        Ok(serde_json::from_slice(&std::fs::read(file)?)?)
    } else {
        Ok(json!({}))
    }
}

fn script(root: &Path, c: Component, name: &str, extra: &[String]) -> Result<Step> {
    if !manifest(root, c)?["scripts"][name].is_string() {
        return Err(portable_error(
            "unsupported_task",
            format!(
                "{} has no {name:?} script; wiki dev tasks lists supported tasks",
                c.name()
            ),
        ));
    }
    let mut step = Step::new(
        c.name(),
        &root.join(c.repository()),
        c.manager(),
        &["run", name],
    );
    if !extra.is_empty() {
        step.args.push("--".into());
        step.args.extend_from_slice(extra);
    }
    Ok(step)
}

fn native(root: &Path, c: Component, task: &str) -> Step {
    let mut step = Step::new(c.name(), &root.join(c.repository()), "cargo", &[task]);
    if let Some(name) = c.crate_name() {
        step.args.extend(["-p".into(), name.into()]);
    }
    step
}

fn python() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

pub fn run(json_out: bool, cwd: &Path, args: &DevArgs) -> Result<Outcome> {
    let root = root(cwd, args.root.as_deref())?;
    match &args.command {
        DevCommand::Components | DevCommand::Tasks => {
            let rows = COMPONENTS.iter().map(|&c| {
                let package = manifest(&root, c)?;
                Ok(json!({"component": c.name(), "path": root.join(c.repository()),
                    "present": root.join(c.repository()).is_dir(), "version": package.get("version"),
                    "packageManager": c.manager(), "crate": c.crate_name(), "scripts": package.get("scripts"),
                    "testSuites": c.suites()}))
            }).collect::<Result<Vec<_>>>()?;
            crate::emit(json_out, &json!({"root": root, "components": rows}), || {
                println!("{}", serde_json::to_string_pretty(&rows)?);
                Ok(())
            })?;
            return Ok(Outcome::Ok);
        }
        DevCommand::Doctor => {
            let tools = ["cargo", "rustc", "node", "npm", python(), "git", "protoc"]
                .map(|name| json!({"tool": name, "path": process::executable(name)}));
            let missing = COMPONENTS
                .iter()
                .filter(|c| !root.join(c.repository()).is_dir())
                .map(|c| c.name())
                .collect::<Vec<_>>();
            let ok = missing.is_empty() && tools.iter().all(|t| !t["path"].is_null());
            let report =
                json!({"ok": ok, "root": root, "tools": tools, "missingComponents": missing});
            crate::emit(json_out, &report, || {
                println!("{}", serde_json::to_string_pretty(&report)?);
                Ok(())
            })?;
            return Ok(if ok {
                Outcome::Ok
            } else {
                Outcome::ProblemsFound
            });
        }
        _ => {}
    }
    let mut steps = Vec::new();
    if matches!(args.command, DevCommand::Install(_)) {
        let install = if root.join("package-lock.json").is_file() {
            "ci"
        } else {
            "install"
        };
        steps.push(Step::new("dependencies", &root, "npm", &[install]));
        steps.push(Step::new(
            "native-dependencies",
            &root,
            "cargo",
            &["fetch", "--locked"],
        ));
        return process::run_steps(json_out, args.dry_run, &steps);
    }
    match &args.command {
        DevCommand::Status => {
            steps.push(Step::new(
                "workspace",
                &root,
                "git",
                &["status", "--short", "--branch"],
            ));
        }
        DevCommand::Run {
            component,
            script: name,
            args,
        } => {
            if *component == Component::All {
                return Err(portable_error(
                    "invalid_request",
                    "run requires one component",
                ));
            }
            steps.push(script(&root, *component, name, args)?);
        }
        DevCommand::Serve {
            component: c,
            args: extra,
        } => {
            if json_out && !args.dry_run {
                return Err(portable_error(
                    "invalid_request",
                    "serve is a foreground stream; omit --json, or use --dry-run",
                ));
            }
            let mut step = match c {
                Component::Desktop => desktop_serve(&root)?,
                Component::App => script(&root, *c, "dev", &[])?,
                _ => {
                    return Err(portable_error(
                        "unsupported_task",
                        "serve supports app and desktop",
                    ))
                }
            };
            if !extra.is_empty() {
                step.args.push("--".into());
                step.args.extend_from_slice(extra);
            }
            steps.push(step);
        }
        DevCommand::Test {
            targets,
            suite,
            args: extra,
        } => {
            let validation = root.join("validation");
            if !validation.join("verify_all.py").is_file() {
                return Err(portable_error(
                    "source_not_found",
                    "validation/verify_all.py is missing",
                ));
            }
            if suite.is_empty()
                && (targets.components.is_empty() || targets.components.contains(&Component::All))
            {
                steps.push(Step::new(
                    "validation",
                    &validation,
                    python(),
                    &["verify_all.py", "--fail-on-skip"],
                ));
            } else {
                let suites = if suite.is_empty() {
                    selected(targets)
                        .iter()
                        .flat_map(|c| c.suites().iter().map(|s| (*s).to_string()))
                        .collect::<BTreeSet<_>>()
                } else {
                    suite.iter().cloned().collect()
                };
                steps.push(Step::new(
                    "validation",
                    &validation,
                    python(),
                    &[
                        "verify_all.py",
                        "--only",
                        &suites.into_iter().collect::<Vec<_>>().join(","),
                        "--fail-on-skip",
                    ],
                ));
            }
            steps.last_mut().unwrap().args.extend_from_slice(extra);
        }
        DevCommand::Install(targets)
        | DevCommand::Check(targets)
        | DevCommand::Fmt(targets)
        | DevCommand::Build { targets, .. } => {
            let mut seen = BTreeSet::new();
            for c in selected(targets) {
                let dir = root.join(c.repository());
                if !dir.is_dir() {
                    return Err(portable_error(
                        "source_not_found",
                        format!("Missing {}", dir.display()),
                    ));
                }
                match &args.command {
                    DevCommand::Install(_) => {
                        if !seen.insert(c.repository()) {
                            continue;
                        }
                        if dir.join("package.json").is_file() {
                            let install_args: &[&str] = if dir.join("package-lock.json").is_file() {
                                &["ci"]
                            } else {
                                &["install"]
                            };
                            steps.push(Step::new(c.name(), &dir, c.manager(), install_args));
                        }
                        if dir.join("Cargo.toml").is_file() {
                            steps.push(Step::new(c.name(), &dir, "cargo", &["fetch", "--locked"]));
                        }
                    }
                    DevCommand::Build { release, .. } => {
                        if c == Component::Desktop {
                            steps.push(desktop_build(&root, *release)?);
                        } else if c.crate_name().is_some() {
                            let mut step = native(&root, c, "build");
                            if *release {
                                step.args.push("--release".into());
                            }
                            steps.push(step);
                        } else if c != Component::Validation {
                            steps.push(script(&root, c, "build", &[])?);
                            if c == Component::Agent {
                                let extra = if *release {
                                    vec!["--release".to_string()]
                                } else {
                                    vec![]
                                };
                                steps.push(script(&root, c, "native:build", &extra)?);
                            }
                        }
                    }
                    DevCommand::Check(_) => {
                        if c.crate_name().is_some() {
                            let mut format = native(&root, c, "fmt");
                            format.args.push("--check".into());
                            steps.push(format);
                            let mut step = native(&root, c, "clippy");
                            step.args.extend([
                                "--all-targets".into(),
                                "--".into(),
                                "-D".into(),
                                "warnings".into(),
                            ]);
                            steps.push(step);
                        } else if c == Component::Validation {
                            steps.push(Step::new(
                                c.name(),
                                &dir,
                                python(),
                                &["-m", "compileall", "-q", "."],
                            ));
                        } else if c == Component::Protocol {
                            steps.push(script(&root, c, "build", &[])?);
                            steps.push(script(&root, c, "check", &[])?);
                        } else {
                            steps.push(script(&root, c, "typecheck", &[])?);
                            if c == Component::Agent {
                                steps.push(script(&root, c, "native:verify-source", &[])?);
                                steps.push(script(&root, c, "native:check", &[])?);
                            }
                        }
                    }
                    DevCommand::Fmt(_) => {
                        if c.crate_name().is_some() {
                            steps.push(native(&root, c, "fmt"));
                        } else {
                            let package = manifest(&root, c)?;
                            let name = ["fmt", "format"]
                                .into_iter()
                                .find(|name| package["scripts"][*name].is_string());
                            if let Some(name) = name {
                                steps.push(script(&root, c, name, &[])?);
                            } else if targets.components.len() == 1
                                && targets.components[0] != Component::All
                            {
                                return Err(portable_error(
                                    "unsupported_task",
                                    format!("{} declares no formatter", c.name()),
                                ));
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        _ => unreachable!(),
    }
    process::run_steps(json_out, args.dry_run, &steps)
}
