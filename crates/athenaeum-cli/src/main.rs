// `wiki` and the compatible `athenaeum` executable share this entry point.
// Arguments and dispatch only; knowledge rules stay in athenaeum-core.

mod api_cmd;
mod bridge;
mod catalog;
mod config_cmd;
mod dev_cmd;
mod library;
mod portable_cmd;
mod process;
mod product_cmd;
mod render;
mod setup_cmd;
mod start_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use athenaeum_core::graph;
use athenaeum_core::prelude::*;
use athenaeum_core::vault::{lint, LintOptions, LintSeverity, PageRef, VaultSync};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

use portable_cmd::{LibraryCommand, RepoCommand, SyncCommand, TrustCommand, VaultCommand};
use product_cmd::ScopeArg;
use render::emit;

#[derive(Parser)]
#[command(
    name = "wiki",
    version,
    about = "Athenaeum: local wikis, knowledge, agents, Library, and development.",
    after_help = "Start: wiki setup\nDiscover every command: wiki commands\nDeveloper tools: wiki dev --help\nThe athenaeum executable accepts the same commands."
)]
struct Cli {
    /// Project directory. Defaults to the nearest ancestor containing
    /// `.athenaeum/`, or the current directory.
    ///
    /// `--workspace` is the spelling the separate `athenaeum-core` command used, kept so a
    /// client written for it (`bridge --workspace DIR`) reaches this engine unchanged.
    #[arg(long, short = 'C', global = true, visible_alias = "workspace")]
    project: Option<PathBuf>,

    /// Emit JSON instead of text, for scripting. Honoured by every command.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Open the compiled desktop app, or use --dev for live UI updates while editing.
    Start(start_cmd::StartArgs),
    /// List every command and option, generated from the actual parser.
    Commands,
    /// Generate shell completion (bash, elvish, fish, powershell, zsh).
    Completions { shell: clap_complete::Shell },
    /// Inspect and edit the desktop's saved settings. Quit the app before writing.
    Config(config_cmd::ConfigArgs),
    /// Build, check, test, and serve the Athenaeum source projects.
    Dev(dev_cmd::DevArgs),
    /// Discover and call the full Library API, including authorized operator routes.
    Api(api_cmd::ApiArgs),
    /// Launch the installed Athenaeum Agent in this project. Pass its flags after `--`.
    Agent {
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the installed MCP server on stdio. Pass MCP flags after `--`.
    Mcp {
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Create `.athenaeum/` in the project directory.
    Init,
    /// Set up Athenaeum for this project and connect your coding agents. Safe to repeat.
    ///
    /// Creates the knowledge base and the repository's identity, indexes the project, optionally
    /// connects the hosted Library (`--library`), and wires Claude Code, Codex and Cursor to the
    /// knowledge base so they recall it and propose what they learn.
    Setup(setup_cmd::SetupArgs),
    /// Remove Athenaeum's configuration from selected external agents, preserving other settings.
    DisconnectAgents {
        #[arg(long, value_delimiter = ',', value_parser = ["claude", "codex", "cursor"])]
        agents: Vec<String>,
    },
    /// Check every link: the engine, this project, the Library and your coding agents.
    Doctor {
        /// Where athenaeum-integrations is (a launcher, or its `.mjs` bundle).
        #[arg(long, value_name = "PATH")]
        integrations: Option<PathBuf>,
    },
    /// Show what the knowledge base contains.
    Stats,
    /// Add a node.
    #[command(visible_alias = "create")]
    Add(AddArgs),
    /// Print a node with its evidence and relationships.
    #[command(visible_alias = "open")]
    Show { id: String },
    /// List nodes.
    List(ListArgs),
    /// Replace a node's summary or content, archiving the previous version.
    Edit(EditArgs),
    /// Show the workspace and index state, or change a node's lifecycle status.
    ///
    /// With no arguments: whether a knowledge base is here, how big it is and whether the
    /// derived index is behind the files on disk. An absent vault is a state, not a failure,
    /// and this command reports it and exits 0. With `<id> <status>`: that node's lifecycle
    /// status, exactly as before.
    Status {
        /// A node id or exact title. Omit for the workspace and index state.
        #[arg(requires = "status")]
        id: Option<String>,
        /// The lifecycle status to set. Needs a node.
        #[arg(requires = "id")]
        status: Option<StatusArg>,
    },
    /// Delete a node, its edges and its history.
    #[command(visible_alias = "delete")]
    Remove { id: String },
    /// Assert a relationship: `link <from> <type> <to>`.
    Link(LinkArgs),
    /// Attach evidence to a node.
    Evidence(EvidenceArgs),
    /// Confirm a claim against its evidence, optionally raising what it
    /// counts as: `verify <id> --authority project_rule`.
    ///
    /// A promotion is refused unless the claim's evidence establishes it —
    /// a project rule needs configuration, project documentation, a recorded
    /// decision, or the user saying so.
    Verify(VerifyArgs),
    /// Replace one claim with another: `supersede <old> <new>`.
    ///
    /// The old claim is kept and stops being offered; §17 is explicit that
    /// historical knowledge is not deleted.
    Supersede { old: String, new: String },
    /// Record that two claims disagree: `contradict <a> <b>`.
    ///
    /// Both stop being offered until somebody decides between them. Resolve
    /// it with `supersede`, or by setting the survivor back to valid.
    Contradict { a: String, b: String },
    /// Find candidate nodes by text.
    Search {
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Walk outward from a node and show what is reachable.
    Navigate(NavigateArgs),
    /// Show how one node connects to another: `path <from> <to>`.
    ///
    /// The answer to "why is this relevant to that", which a list of
    /// neighbours cannot give. Finding no path is an ordinary answer — the
    /// two nodes may simply be unconnected — and exits 0.
    Path(PathArgs),
    /// Assemble a knowledge brief for a task.
    Brief(BriefArgs),
    /// Show every archived revision of a node.
    History { id: String },
    /// Read the event journal.
    Journal {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Handle one versioned Wiki integration request from standard input.
    ///
    /// This command always writes one JSON response, independent of --json.
    Bridge,
    /// Deterministically index files, symbols, imports and local Git history.
    ///
    /// Without `--scope` this indexes the repository with the budgets below and prints the
    /// detailed repository report, which is what it has always done. `--scope vault|repository|all`
    /// indexes with the engine's own budgets, prints one unified summary, and reports what it
    /// skipped and whether a budget ended the walk early. `--scope repository` is therefore not
    /// byte-identical to plain `index`: same work, summary instead of detail.
    ///
    /// A repository pass reuses the files whose size and modification time are unchanged, which
    /// is what makes re-indexing a large tree cheap; `--full` reads every file again.
    Index(IndexArgs),
    /// Every file the index has read: sources under `raw/sources/`, repository files, their
    /// content hashes, how much of the graph points at them, and whether they moved since.
    Sources {
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Rebuild the derived index over the vault's markdown.
    ///
    /// Safe to run at any time and cheap to run often: it writes only what
    /// changed. Deleting `.athenaeum/knowledge.db` and running this costs
    /// nothing but the time it takes.
    IndexVault(IndexVaultArgs),
    /// Report what the wiki links at and what links at nothing.
    ///
    /// Reads the index rather than the markdown, so the links it reports on
    /// are the ones the knowledge base actually holds. Indexes first, so a
    /// vault edited a second ago lints correctly. Exits non-zero only when
    /// a link is broken; being unlinked is a shape, not an error.
    Lint(LintArgs),
    /// Search for moved source anchors and update knowledge staleness.
    Relocate(RelocateArgs),
    /// Review the knowledge base: contradictions, disputes, stale anchors,
    /// unsupported rules, proposals awaiting acceptance, unreachable nodes.
    ///
    /// Exits non-zero when something found could hand an agent a wrong
    /// answer. Findings that are only untidy are reported and forgiven,
    /// unless `--strict` says otherwise.
    Check(CheckArgs),
    /// Review the knowledge base: contradictions, disputes, stale anchors, unsupported rules,
    /// proposals awaiting a decision, duplicates, near-duplicates, superseded knowledge, orphans
    /// and incomplete scans.
    ///
    /// Exits non-zero only when a finding could hand an agent a wrong answer, unless `--strict`
    /// says every finding counts. `check` remains the terse internal sweep; this is the queue a
    /// person or a script acts on, in the same shape the bridge's `review` method returns.
    Review {
        #[arg(long, default_value_t = 200)]
        limit: u32,
        /// Treat every finding as a failure, not only the blocking ones.
        #[arg(long)]
        strict: bool,
    },
    /// The repository's identity: `repo init`, `repo info`.
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    /// Model proposals awaiting a decision.
    Proposals {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Accept a proposal: write it to the vault as a page, so it is permanent and portable.
    ///
    /// Accepting is not verifying. The page records the authority the claim already had.
    Accept {
        id: String,
        /// Who is deciding, recorded in the page.
        #[arg(long = "as", default_value = "user")]
        by: String,
    },
    /// Reject a proposal so it is not offered again. Writes no file.
    Reject {
        id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "as", default_value = "user")]
        by: String,
    },
    /// Propose an observation drawn from a file under `raw/sources/`, without a model.
    ///
    /// The draft quotes the file's first line and cites it with its content hash. It is a
    /// proposal: review it with `proposals`, then `accept` or `reject` it.
    DraftSource {
        /// A vault-relative path under `raw/sources/`.
        #[arg(long)]
        source: String,
        #[arg(long)]
        title: String,
    },
    /// Import proposals left in the retired `athenaeum-core` index (`.athenaeum/local-core.db`).
    ///
    /// Defaults to a dry run. Reads the old database and never changes or deletes it.
    ImportLegacyCore {
        /// Actually propose them.
        #[arg(long)]
        apply: bool,
    },
    /// Operations on the vault itself: `vault materialize`.
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    /// Write the repository to a zip: text, sources and a manifest of hashes, never the index.
    Export { output: PathBuf },
    /// Create a repository from an export. Never overwrites; indexes when done.
    Import {
        archive: PathBuf,
        destination: PathBuf,
    },
    /// Pages that arrived from outside and have not been reviewed.
    Trust {
        #[command(subcommand)]
        command: TrustCommand,
    },
    /// Connect to an Athenaeum Library server.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Synchronise with a Library. On its own: pull, then push what is left.
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommand>,
        /// The revision message for the push.
        #[arg(long, short, global = true)]
        message: Option<String>,
    },
}

#[derive(Args)]
struct VerifyArgs {
    id: String,
    /// What the claim should count as once confirmed. Omit to confirm it
    /// without changing that.
    #[arg(long)]
    authority: Option<String>,
}

#[derive(Args)]
struct CheckArgs {
    /// Treat every finding as a failure, not only the ones that could hand
    /// an agent a wrong answer.
    #[arg(long)]
    strict: bool,
}

#[derive(Args)]
struct AddArgs {
    /// One of: project, repository, module, file, symbol, concept, fact,
    /// rule, decision, procedure, gotcha, observation, preference, entity,
    /// api, test, source, dependency.
    #[arg(long = "type", short = 't', default_value = "observation")]
    node_type: String,
    title: String,
    #[arg(long, short, default_value = "")]
    summary: String,
    #[arg(long, short, default_value = "")]
    content: String,
    /// Use a specific id instead of a generated one, e.g. `Decision_042`.
    #[arg(long)]
    id: Option<String>,
    #[arg(long, default_value = "unverified_observation")]
    authority: String,
    #[arg(long, default_value = "project")]
    scope: String,
    /// Record this as stated by the user rather than proposed by a model.
    #[arg(long)]
    stated: bool,
}

#[derive(Args)]
struct ListArgs {
    #[arg(long = "type", short = 't')]
    node_type: Option<String>,
    /// Include superseded and archived knowledge.
    #[arg(long)]
    all: bool,
    #[arg(long, default_value_t = 50)]
    limit: u32,
}

#[derive(Args)]
struct EditArgs {
    id: String,
    #[arg(long, short)]
    summary: Option<String>,
    #[arg(long, short)]
    content: Option<String>,
    #[arg(long, short)]
    title: Option<String>,
    /// Why the change was made. Stored with the archived revision.
    #[arg(long, short)]
    reason: Option<String>,
}

#[derive(Args)]
struct LinkArgs {
    from: String,
    /// e.g. depends_on, verified_by, constrained_by, has_gotcha.
    edge_type: String,
    to: String,
    /// Mark as deterministically derived rather than model-proposed, which
    /// makes it immediately traversable.
    #[arg(long)]
    derived: Option<String>,
    /// Why the relationship exists, recorded with it.
    #[arg(long)]
    note: Option<String>,
    /// How sure whoever wrote it was, from 0 to 1. Omit when the
    /// relationship is simply a fact.
    #[arg(long)]
    confidence: Option<f32>,
}

#[derive(Args)]
struct EvidenceArgs {
    node: String,
    /// Repository-relative path to the file that justifies the claim.
    #[arg(long, short)]
    file: String,
    #[arg(long, short)]
    symbol: Option<String>,
    #[arg(long, short)]
    excerpt: Option<String>,
    #[arg(long, default_value = "source_file")]
    kind: String,
    #[arg(long)]
    commit: Option<String>,
}

#[derive(Args)]
struct NavigateArgs {
    id: String,
    #[arg(long, short, default_value_t = 2)]
    depth: u8,
    #[arg(long, default_value_t = 25)]
    max_nodes: usize,
}

#[derive(Args)]
struct PathArgs {
    from: String,
    to: String,
    /// How far to look before giving up.
    #[arg(long, short, default_value_t = 6)]
    depth: u8,
    #[arg(long, default_value_t = 2000)]
    max_nodes: usize,
}

#[derive(Args)]
struct BriefArgs {
    /// The task the brief is for.
    task: Vec<String>,
    /// Seed the walk from these nodes. When omitted, seeds come from
    /// searching the knowledge base for the task text.
    #[arg(long)]
    seed: Vec<String>,
    #[arg(long, short, default_value_t = 2)]
    depth: u8,
    /// Wrap the output in the trust boundary a model should receive.
    #[arg(long)]
    prompt: bool,
}

#[derive(Args)]
struct IndexArgs {
    /// What to index: `vault`, `repository` or `all`.
    ///
    /// Omit to index only the repository with the budgets below. With `--scope`, the pass uses
    /// the engine's own scan budgets, so the four budget flags cannot be combined with it.
    #[arg(
        long,
        value_enum,
        conflicts_with_all = ["max_commits", "max_files", "max_file_bytes", "max_total_bytes", "max_duration_ms", "full"]
    )]
    scope: Option<ScopeArg>,
    /// A repository-relative path to re-index. Repeatable; repository scope only.
    #[arg(long = "path", value_name = "PATH", requires = "scope")]
    paths: Vec<String>,
    /// Read every repository file again instead of reusing the ones whose size and modification
    /// time are unchanged. Slower; what a first scan, or a suspicious one, does.
    #[arg(long, conflicts_with = "scope")]
    full: bool,
    /// Maximum number of recent commits to connect to indexed files.
    #[arg(long, default_value_t = 500)]
    max_commits: usize,
    /// Maximum number of repository files to scan.
    #[arg(long, default_value_t = 20_000)]
    max_files: usize,
    /// Maximum UTF-8 bytes read from one file.
    #[arg(long, default_value_t = 1_000_000)]
    max_file_bytes: u64,
    /// Maximum UTF-8 bytes read across the repository.
    #[arg(long, default_value_t = 67_108_864)]
    max_total_bytes: u64,
    /// Wall-clock ceiling for the pass, in milliseconds; 0 means no ceiling. Reaching it stops
    /// the walk and reports the scan incomplete rather than appearing to have read everything.
    #[arg(long, default_value_t = 30_000)]
    max_duration_ms: u64,
}

#[derive(Args)]
struct IndexVaultArgs {
    /// Maximum number of wiki pages to index.
    #[arg(long, default_value_t = 50_000)]
    max_pages: usize,
    /// Maximum number of files under `raw/sources/` to index.
    #[arg(long, default_value_t = 50_000)]
    max_sources: usize,
    /// Maximum UTF-8 bytes read from one page.
    #[arg(long, default_value_t = 4_000_000)]
    max_page_bytes: u64,
    /// Maximum UTF-8 bytes read across the whole vault.
    #[arg(long, default_value_t = 268_435_456)]
    max_total_bytes: u64,
    /// Wall-clock ceiling for the pass, in milliseconds; 0 means no ceiling. Reaching it stops
    /// the walk and reports the scan incomplete rather than appearing to have read everything.
    #[arg(long, default_value_t = 30_000)]
    max_duration_ms: u64,
    /// List every unresolved `[[link]]` rather than the first few.
    #[arg(long)]
    show_unresolved: bool,
}

#[derive(Args)]
struct LintArgs {
    /// Do not report pages nothing links to.
    #[arg(long)]
    ignore_orphan: bool,
    /// Do not report pages that link to nothing.
    #[arg(long)]
    ignore_no_outlinks: bool,
    /// Skip a page entirely. Repeatable; names a page by slug or path.
    #[arg(long = "ignore-page", value_name = "PAGE")]
    ignore_pages: Vec<String>,
    /// Maximum number of wiki pages to index before linting.
    #[arg(long, default_value_t = 50_000)]
    max_pages: usize,
    /// Maximum number of files under `raw/sources/` to index.
    #[arg(long, default_value_t = 50_000)]
    max_sources: usize,
    /// Maximum UTF-8 bytes read from one page.
    #[arg(long, default_value_t = 4_000_000)]
    max_page_bytes: u64,
    /// Maximum UTF-8 bytes read across the whole vault.
    #[arg(long, default_value_t = 268_435_456)]
    max_total_bytes: u64,
    /// Wall-clock ceiling for the pass, in milliseconds; 0 means no ceiling.
    #[arg(long, default_value_t = 30_000)]
    max_duration_ms: u64,
}

#[derive(Args)]
struct RelocateArgs {
    /// Maximum number of source files to inspect while relocating anchors.
    #[arg(long, default_value_t = 20_000)]
    max_files: usize,
    /// Maximum UTF-8 bytes read from one file.
    #[arg(long, default_value_t = 1_000_000)]
    max_file_bytes: u64,
    /// Maximum UTF-8 bytes read across the repository.
    #[arg(long, default_value_t = 67_108_864)]
    max_total_bytes: u64,
    /// Wall-clock ceiling for the scan, in milliseconds; 0 means no ceiling.
    #[arg(long, default_value_t = 30_000)]
    max_duration_ms: u64,
}

#[derive(Clone, Copy, ValueEnum)]
enum StatusArg {
    Valid,
    Stale,
    NeedsReview,
    Superseded,
    Contradicted,
    Archived,
}

impl From<StatusArg> for NodeStatus {
    fn from(value: StatusArg) -> Self {
        match value {
            StatusArg::Valid => NodeStatus::Valid,
            StatusArg::Stale => NodeStatus::Stale,
            StatusArg::NeedsReview => NodeStatus::StaleRequiresReview,
            StatusArg::Superseded => NodeStatus::Superseded,
            StatusArg::Contradicted => NodeStatus::Contradicted,
            StatusArg::Archived => NodeStatus::Archived,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(outcome) => outcome.into(),
        Err(err) => {
            if cli.json {
                // A script asked for JSON, so a failure is JSON too, with a code it can branch
                // on. The mapping is the bridge's, not a second one: `knowledge_error` for a
                // store that cannot be read, `invalid_request` for a value the library refuses,
                // `workspace_not_found` for an absent knowledge base — the same string a client
                // sees from `athenaeum bridge` for the same failure.
                let code = athenaeum_core::bridge::error_code(&err);
                println!(
                    "{}",
                    serde_json::json!({ "error": { "code": code, "message": err.to_string() } })
                );
            }
            eprintln!("athenaeum: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What a command concluded, separately from whether it failed.
///
/// `check` finding problems is not an error — it is a successful inspection
/// with a non-zero verdict, and inventing an error to carry that would put a
/// misleading `athenaeum: invalid …` line under output that is already clear.
enum Outcome {
    Ok,
    ProblemsFound,
    BridgeError,
}

impl From<Outcome> for ExitCode {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Ok => ExitCode::SUCCESS,
            Outcome::ProblemsFound | Outcome::BridgeError => ExitCode::FAILURE,
        }
    }
}

/// Parse one of the library's vocabularies, naming the field that was wrong.
fn parse_value<T: std::str::FromStr>(raw: &str, field: &'static str) -> Result<T> {
    raw.parse::<T>().map_err(|_| Error::InvalidValue {
        field,
        value: raw.to_string(),
    })
}

fn run(cli: &Cli) -> Result<Outcome> {
    let cwd = match &cli.project {
        Some(project) => project.clone(),
        None => std::env::current_dir()?,
    };

    if let Command::Init = cli.command {
        let workspace = Workspace::init(&cwd)?;
        workspace.open()?;
        emit(
            cli.json,
            &serde_json::json!({
                "initialized": true,
                "projectPath": cwd.display().to_string(),
                "workspacePath": workspace.root().display().to_string(),
            }),
            || {
                println!(
                    "Initialized knowledge base at {}",
                    workspace.root().display()
                );
                Ok(())
            },
        )?;
        return Ok(Outcome::Ok);
    }

    // These work on a directory that has no workspace yet, or create the one they work in.
    match &cli.command {
        Command::Commands => return catalog::run(cli.json),
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "wiki", &mut std::io::stdout());
            return Ok(Outcome::Ok);
        }
        Command::Config(args) => return config_cmd::run(cli.json, args),
        Command::Start(args) => return start_cmd::run(cli.json, &cwd, args),
        Command::Dev(args) => return dev_cmd::run(cli.json, &cwd, args),
        Command::Api(args) => return api_cmd::run(cli.json, args),
        Command::Agent { args } => return process::launch(cli.json, &cwd, "agent", args),
        Command::Mcp { args } => return process::launch(cli.json, &cwd, "mcp", args),
        Command::Setup(args) => {
            return setup_cmd::setup(cli.json, &cwd, cli.project.is_some(), args)
        }
        Command::DisconnectAgents { agents } => {
            return setup_cmd::disconnect_agents(cli.json, agents)
        }
        Command::Doctor { integrations } => {
            return setup_cmd::doctor(cli.json, &cwd, integrations.as_deref())
        }
        Command::Repo {
            command: RepoCommand::Init { name },
        } => {
            return portable_cmd::repo_init(cli.json, &cwd, name.as_deref());
        }
        Command::Repo {
            command: RepoCommand::Info,
        } => {
            return portable_cmd::repo_info(cli.json, &cwd);
        }
        Command::Import {
            archive,
            destination,
        } => {
            return portable_cmd::import(cli.json, archive, destination);
        }
        Command::Library { command } => return portable_cmd::library(cli.json, command),
        Command::Sync { command, message } => {
            return portable_cmd::sync(cli.json, &cwd, command.as_ref(), message.as_deref());
        }
        // `athenaeum status` with no node is a question about the directory, and a directory
        // with no knowledge base is exactly the answer it exists to give.
        Command::Status { id: None, .. } => return product_cmd::status(cli.json, &cwd),
        _ => {}
    }

    // The bridge owns its error envelope, including workspace discovery and
    // schema compatibility failures. Handling it before the ordinary CLI
    // open path guarantees an external runtime always receives JSON.
    if let Command::Bridge = cli.command {
        let workspace = Workspace::discover(&cwd);
        return Ok(if bridge::run(workspace.as_ref(), &cwd)? {
            Outcome::Ok
        } else {
            Outcome::BridgeError
        });
    }

    let workspace = Workspace::discover(&cwd).ok_or_else(|| Error::NoWorkspace {
        path: cwd.display().to_string(),
    })?;
    let mut store = workspace.open()?;

    // A read answered from an index built before the vault's last edit would present stale
    // knowledge as current. The bridge already reconciles before it answers a read; the CLI
    // answers from the same index, so it does the same, and says so when it changed something.
    if reads_vault(&cli.command) {
        refresh_vault_index(&mut store, &workspace);
    }

    match &cli.command {
        Command::Init => unreachable!("handled before the workspace is opened"),

        Command::Stats => {
            let stats = store.stats()?;
            emit(cli.json, &stats, || {
                render::stats(&stats, workspace.root());
                Ok(())
            })?;
        }

        Command::Add(args) => {
            let mut node =
                KnowledgeNode::new(parse_value(&args.node_type, "node type")?, &args.title)
                    .with_summary(&args.summary)
                    .with_content(&args.content)
                    .with_authority(parse_value(&args.authority, "authority")?)
                    .with_scope(parse_value(&args.scope, "scope")?);
            if let Some(id) = &args.id {
                node = node.with_id(id.as_str());
            }
            if args.stated {
                node = node.with_provenance(Provenance::user());
            }

            let created = store.create_node(node)?;
            // A node the user stated is accepted by being stated, so it becomes a page now. It
            // would otherwise exist only in the database and be lost with it.
            if args.stated {
                if let Err(error) =
                    athenaeum_core::portable::memory::materialize_stated(&mut store, &created.id)
                {
                    eprintln!(
                        "warning: {} was stored, but could not be written to the vault: {error}",
                        created.id
                    );
                }
            }
            emit(cli.json, &created, || {
                println!("{}  {}", created.id, created.title);
                Ok(())
            })?;
        }

        Command::Show { id } => {
            let node = resolve(&store, id)?;
            emit(cli.json, &node, || render::node_detail(&store, &node))?;
        }

        Command::List(args) => {
            let mut filter = if args.all {
                NodeFilter::all()
            } else {
                NodeFilter::live()
            };
            if let Some(raw) = &args.node_type {
                filter = filter.of_type(parse_value(raw, "node type")?);
            }
            let nodes = store.list_nodes(&filter.limit(args.limit))?;
            emit(cli.json, &nodes, || {
                render::node_list(&nodes);
                Ok(())
            })?;
        }

        Command::Edit(args) => {
            let mut node = resolve(&store, &args.id)?;
            if let Some(title) = &args.title {
                node.title = title.clone();
            }
            if let Some(summary) = &args.summary {
                node.summary = summary.clone();
            }
            if let Some(content) = &args.content {
                node.content = content.clone();
            }
            let updated = store.update_node(node, args.reason.clone())?;
            emit(cli.json, &updated, || {
                println!("{} is now at version {}", updated.id, updated.version);
                Ok(())
            })?;
        }

        Command::Status {
            id: Some(id),
            status: Some(status),
        } => {
            let node = resolve(&store, id)?;
            let updated = store.set_node_status(&node.id, (*status).into())?;
            emit(cli.json, &updated, || {
                println!("{} → {}", updated.id, updated.status);
                Ok(())
            })?;
        }

        Command::Status { .. } => unreachable!("handled before the workspace is opened"),

        Command::Remove { id } => {
            let node = resolve(&store, id)?;
            store.delete_node(&node.id)?;
            emit(cli.json, &node, || {
                println!("removed {} ({})", node.id, node.title);
                Ok(())
            })?;
        }

        Command::Verify(args) => {
            let node = resolve(&store, &args.id)?;
            let authority = args
                .authority
                .as_deref()
                .map(|value| parse_value::<Authority>(value, "authority"))
                .transpose()?;
            let verified = store.verify(&node.id, authority)?;
            // A page's authority is what the page says, and the index follows the vault: without
            // writing it there, the next reconcile would put the old value back.
            if verified.authority != node.authority {
                if let Some(project) = workspace.project_root() {
                    let evidence = store.evidence_for(&node.id)?;
                    if let Some(page) = PageRef::of(&node, &evidence, Some(project)) {
                        let target = project.join(&page.path);
                        let text = std::fs::read_to_string(&target)?;
                        let updated = athenaeum_core::vault::set_fields(
                            &text,
                            &[(
                                "authority",
                                athenaeum_core::vault::FieldValue::Scalar(
                                    verified.authority.as_str().to_string(),
                                ),
                            )],
                        );
                        athenaeum_core::portable::write_atomic(&target, updated.as_bytes())?;
                        VaultIndexer::scan_and_index(
                            &mut store,
                            project,
                            VaultScanOptions::default(),
                        )?;
                    }
                }
            }
            emit(cli.json, &verified, || {
                println!(
                    "{:?} verified; it counts as {}",
                    verified.title, verified.authority
                );
                Ok(())
            })?;
        }

        Command::Supersede { old, new } => {
            let old = resolve(&store, old)?;
            let new = resolve(&store, new)?;
            store.supersede(&old.id, &new.id)?;
            let old = store.get_node(&old.id)?;
            emit(cli.json, &old, || {
                println!(
                    "{:?} now supersedes {:?}, which is {} and kept",
                    new.title, old.title, old.status
                );
                Ok(())
            })?;
        }

        Command::Contradict { a, b } => {
            let a = resolve(&store, a)?;
            let b = resolve(&store, b)?;
            let edge = store.contradict(&a.id, &b.id)?;
            emit(cli.json, &edge, || {
                println!(
                    "{:?} and {:?} are recorded as contradicting; neither is offered \
                     until one is resolved",
                    a.title, b.title
                );
                Ok(())
            })?;
        }

        Command::Link(args) => {
            let from = resolve(&store, &args.from)?;
            let to = resolve(&store, &args.to)?;
            let edge_type: EdgeType = parse_value(&args.edge_type, "edge type")?;

            let mut edge = match &args.derived {
                Some(detail) => {
                    KnowledgeEdge::deterministic(from.id.clone(), edge_type, to.id.clone(), detail)
                }
                None => KnowledgeEdge::new(from.id.clone(), edge_type, to.id.clone()),
            };
            if let Some(note) = &args.note {
                edge = edge.with_note(note);
            }
            // A confidence outside 0..1 is not a confidence. Refused at the boundary rather than
            // stored and re-rendered as something a reader would take at face value.
            if let Some(confidence) = args.confidence {
                if !(0.0..=1.0).contains(&confidence) {
                    return Err(Error::InvalidValue {
                        field: "confidence",
                        value: confidence.to_string(),
                    });
                }
                edge = edge.with_confidence(confidence);
            }

            let created = store.connect(edge)?;
            emit(cli.json, &created, || {
                println!(
                    "{} → {} → {}  [{}]",
                    from.title, created.edge_type, to.title, created.status
                );
                Ok(())
            })?;
        }

        Command::Evidence(args) => {
            let node = resolve(&store, &args.node)?;

            let mut anchor = SourceAnchor::file(&args.file);
            if let Some(symbol) = &args.symbol {
                anchor = anchor.with_symbol(symbol);
            }
            if let Some(commit) = &args.commit {
                anchor = anchor.with_commit(commit);
            }

            let mut evidence =
                Evidence::new(node.id.clone(), parse_value(&args.kind, "evidence kind")?)
                    .with_anchor(anchor);
            if let Some(excerpt) = &args.excerpt {
                evidence = evidence.with_excerpt(excerpt);
            }

            let attached = store.attach_evidence(evidence)?;
            emit(cli.json, &attached, || {
                println!("attached {} to {}", attached.citation(), node.title);
                Ok(())
            })?;
        }

        Command::Search { query, limit } => {
            let query = query.join(" ");
            let hits = store.search(&query, *limit)?;
            emit(cli.json, &hits, || {
                if hits.is_empty() {
                    println!("nothing found for {query:?}");
                } else {
                    render::search_hits(&hits);
                }
                Ok(())
            })?;
        }

        Command::Navigate(args) => {
            let node = resolve(&store, &args.id)?;
            let reached = graph::navigate(
                &store,
                std::slice::from_ref(&node.id),
                &NavigationOptions::default()
                    .depth(args.depth)
                    .max_nodes(args.max_nodes),
            )?;
            emit(cli.json, &reached, || {
                render::navigation(&reached);
                Ok(())
            })?;
        }

        Command::Path(args) => {
            let from = resolve(&store, &args.from)?;
            let to = resolve(&store, &args.to)?;
            let path = graph::shortest_path(
                &store,
                &from.id,
                &to.id,
                &NavigationOptions::default()
                    .depth(args.depth)
                    .max_nodes(args.max_nodes),
            )?;
            // Succeeding with "no path" is the answer, not a failure: the two nodes exist and
            // nothing connects them within the search depth. The bridge's `path` method reports
            // the same thing as `found: false`, and the exit code agrees with it.
            emit(cli.json, &path, || {
                render::path(&store, &from, &to, path.as_deref())
            })?;
        }

        Command::Brief(args) => {
            let task = args.task.join(" ");
            let seeds = if args.seed.is_empty() {
                store
                    .search(&task, 5)?
                    .into_iter()
                    .map(|hit| hit.node.id)
                    .collect::<Vec<_>>()
            } else {
                args.seed
                    .iter()
                    .map(|raw| resolve(&store, raw).map(|node| node.id))
                    .collect::<Result<Vec<_>>>()?
            };

            let brief = KnowledgeBrief::assemble(
                &mut store,
                &task,
                &seeds,
                &NavigationOptions::default().depth(args.depth),
            )?;

            emit(cli.json, &brief, || {
                if args.prompt {
                    println!("{}", brief.to_prompt());
                } else {
                    println!("{}", brief.to_markdown());
                }
                Ok(())
            })?;
        }

        Command::History { id } => {
            let node = resolve(&store, id)?;
            let versions = store.history(&node.id)?;
            emit(cli.json, &versions, || {
                render::history(&node, &versions);
                Ok(())
            })?;
        }

        Command::Journal { limit } => {
            let events = store.events(&EventFilter::default().limit(*limit))?;
            emit(cli.json, &events, || {
                render::journal(&events);
                Ok(())
            })?;
        }

        Command::Bridge => unreachable!("handled before the workspace is opened"),

        Command::Sources { limit } => return product_cmd::sources(cli.json, &store, *limit),

        Command::Repo { .. }
        | Command::Commands
        | Command::Completions { .. }
        | Command::Config(_)
        | Command::Start(_)
        | Command::Dev(_)
        | Command::Api(_)
        | Command::Agent { .. }
        | Command::Mcp { .. }
        | Command::Import { .. }
        | Command::Library { .. }
        | Command::Sync { .. }
        | Command::Setup(_)
        | Command::DisconnectAgents { .. }
        | Command::Doctor { .. } => {
            unreachable!("handled before the workspace is opened")
        }

        Command::DraftSource { source, title } => {
            return portable_cmd::draft_source(cli.json, &mut store, source, title)
        }
        Command::ImportLegacyCore { apply } => {
            return portable_cmd::import_legacy_core(cli.json, &mut store, *apply)
        }
        Command::Proposals { limit } => {
            return portable_cmd::proposals(cli.json, &mut store, *limit)
        }
        Command::Accept { id, by } => return portable_cmd::accept(cli.json, &mut store, id, by),
        Command::Reject { id, reason, by } => {
            return portable_cmd::reject(cli.json, &mut store, id, reason.as_deref(), by)
        }
        Command::Vault {
            command: VaultCommand::Materialize { apply },
        } => return portable_cmd::materialize(cli.json, &mut store, *apply),
        Command::Export { output } => return portable_cmd::export(cli.json, &workspace, output),
        Command::Trust { command } => {
            return portable_cmd::trust(cli.json, &mut store, &workspace, command)
        }

        Command::Index(args) => {
            // `--scope` is the unified pass: it can cover the vault, the repository or both, and
            // it reports what it skipped and whether a budget ended the walk early. Without it,
            // the command does exactly what it always did — a repository pass with the budgets
            // the caller named — so an existing script sees the same report it saw before.
            if let Some(scope) = args.scope {
                return product_cmd::index(cli.json, &mut store, &workspace, scope, &args.paths);
            }
            let project_root = workspace
                .project_root()
                .ok_or_else(|| Error::InvalidValue {
                    field: "workspace",
                    value: workspace.root().display().to_string(),
                })?;
            let report = RepositoryIndexer::scan_and_index(
                &mut store,
                project_root,
                RepositoryScanOptions {
                    max_commits: args.max_commits,
                    max_files: args.max_files,
                    max_file_bytes: args.max_file_bytes,
                    max_total_bytes: args.max_total_bytes,
                    max_duration_ms: args.max_duration_ms,
                    incremental: !args.full,
                },
            )?;
            emit(cli.json, &report, || {
                println!(
                    "indexed {} files, {} symbols ({} exported), {} imports and {} commits",
                    report.files,
                    report.symbols,
                    report.exported_symbols,
                    report.resolved_imports,
                    report.commits
                );
                println!(
                    "code graph: {} call(s), {} implementation(s), {} test reference(s)",
                    report.resolved_calls, report.implementations, report.test_links
                );
                println!(
                    "{} nodes created, {} updated, {} stale; {} edges created, {} removed",
                    report.nodes_created,
                    report.nodes_updated,
                    report.nodes_marked_stale,
                    report.edges_created,
                    report.edges_removed
                );
                for warning in &report.warnings {
                    eprintln!("warning: {warning}");
                }
                Ok(())
            })?;
        }

        Command::IndexVault(args) => {
            let project_root = workspace
                .project_root()
                .ok_or_else(|| Error::InvalidValue {
                    field: "workspace",
                    value: workspace.root().display().to_string(),
                })?;
            let report = VaultIndexer::scan_and_index(
                &mut store,
                project_root,
                VaultScanOptions {
                    max_pages: args.max_pages,
                    max_sources: args.max_sources,
                    max_page_bytes: args.max_page_bytes,
                    max_total_bytes: args.max_total_bytes,
                    max_duration_ms: args.max_duration_ms,
                },
            )?;
            emit(cli.json, &report, || {
                render::vault_index(&report, args.show_unresolved);
                Ok(())
            })?;
        }

        Command::Lint(args) => {
            let project_root = workspace
                .project_root()
                .ok_or_else(|| Error::InvalidValue {
                    field: "workspace",
                    value: workspace.root().display().to_string(),
                })?;
            let report = lint::scan_and_lint(
                &mut store,
                project_root,
                VaultScanOptions {
                    max_pages: args.max_pages,
                    max_sources: args.max_sources,
                    max_page_bytes: args.max_page_bytes,
                    max_total_bytes: args.max_total_bytes,
                    max_duration_ms: args.max_duration_ms,
                },
                &LintOptions {
                    ignore_orphan: args.ignore_orphan,
                    ignore_no_outlinks: args.ignore_no_outlinks,
                    ignore_pages: args.ignore_pages.clone(),
                },
            )?;
            // Only a broken link fails the command. Orphans and dead ends
            // are shapes a wiki legitimately has — plenty of vaults have
            // deliberate leaves — and exiting non-zero for them would make
            // the exit code mean "this wiki exists".
            let broken = report
                .findings
                .iter()
                .any(|finding| finding.severity == LintSeverity::Warning);
            emit(cli.json, &report, || {
                render::vault_lint(&report);
                Ok(())
            })?;
            if broken {
                return Ok(Outcome::ProblemsFound);
            }
        }

        Command::Relocate(args) => {
            let project_root = workspace
                .project_root()
                .ok_or_else(|| Error::InvalidValue {
                    field: "workspace",
                    value: workspace.root().display().to_string(),
                })?;
            let report = AnchorRelocator::scan(
                project_root,
                RepositoryScanOptions {
                    max_files: args.max_files,
                    max_file_bytes: args.max_file_bytes,
                    max_total_bytes: args.max_total_bytes,
                    max_commits: 0,
                    max_duration_ms: args.max_duration_ms,
                    // Relocation looks for a moved anchor, not for changed code, so it reuses the
                    // files the last scan already read; the hash check inside the scan is what
                    // keeps a reuse from being wrong.
                    incremental: true,
                },
            )?
            .relocate_store(&mut store)?;
            emit(cli.json, &report, || {
                println!(
                    "{} exact, {} by symbol, {} by fingerprint, {} stale",
                    report.exact,
                    report.relocated_by_symbol,
                    report.relocated_by_fingerprint,
                    report.stale
                );
                for warning in &report.warnings {
                    eprintln!("warning: {warning}");
                }
                Ok(())
            })?;
        }

        Command::Review { limit, strict } => {
            note_if_index_stale(&store);
            return product_cmd::review(cli.json, &store, *limit, *strict);
        }

        Command::Check(args) => {
            note_if_index_stale(&store);
            let report = athenaeum_core::review::sweep(&store)?;
            let blocking = report.has_blocking();
            emit(cli.json, &report, || {
                render::review(&report);
                Ok(())
            })?;
            // Being untidy is not a build failure; being wrong is. Only the
            // findings that mean an agent may be handed something incorrect
            // set the exit code, unless the caller asks for all of them.
            if blocking || (args.strict && !report.findings.is_empty()) {
                return Ok(Outcome::ProblemsFound);
            }
        }
    }

    Ok(Outcome::Ok)
}

/// Accept either a node id or a title, because typing `Decision_042` is
/// pleasant and typing `node_9f2c…` is not.
fn resolve(store: &KnowledgeStore, reference: &str) -> Result<KnowledgeNode> {
    let id = NodeId::from_existing(reference);
    if let Some(node) = store.find_node(&id)? {
        return Ok(node);
    }
    store
        .find_node_by_title(reference)?
        .ok_or(Error::NodeNotFound(id))
}

/// True for the commands that answer from the vault's derived index.
///
/// The same set the bridge treats as reads: a question asked just after the markdown moved must
/// not be answered from the index as it was before the move. Commands that index the vault
/// themselves (`lint`, `index-vault`) and commands that do not read vault knowledge (`journal`,
/// the repository scan) are not listed, and neither are mutations: a write reconciles what it
/// needs, and doing it here would make an edit depend on a scan it never asked for.
///
/// `status`, `review`, `check` and `sources` are absent on purpose: they *report* whether the
/// index is behind and what a scan did not read, so repairing it first would erase the state they
/// exist to show. They report rather than repair; a read repairs.
fn reads_vault(command: &Command) -> bool {
    matches!(
        command,
        Command::Stats
            | Command::Show { .. }
            | Command::List(_)
            | Command::Search { .. }
            | Command::Navigate(_)
            | Command::Path(_)
            | Command::Brief(_)
            | Command::History { .. }
            | Command::Proposals { .. }
    )
}

/// Say so when a report about the index was produced from an index that is behind the files.
///
/// The report is not repaired here — `review`, `check` and `sources` exist to describe the state
/// they find, including a stale one — but a person reading a queue deserves to know that the
/// markdown has moved on since it was built.
fn note_if_index_stale(store: &KnowledgeStore) {
    match athenaeum_core::status::index_status(store) {
        Ok(status) if status.stale => eprintln!(
            "athenaeum: the files have changed since the index was last built; run \
             `athenaeum index --scope all` for a report that reflects them"
        ),
        Ok(_) => {}
        Err(error) => eprintln!("athenaeum: could not tell whether the index is current: {error}"),
    }
}

/// Reconcile the derived index with the vault before a read is answered from it.
///
/// A failure here never fails the read: the answer would have been the same without the step, so
/// the reason goes to stderr — where `--json` output on stdout is untouched — and the command
/// proceeds. The message names what changed rather than silently repairing it, because "the index
/// was behind" is something a script may want to notice.
fn refresh_vault_index(store: &mut KnowledgeStore, workspace: &Workspace) {
    let Some(project) = workspace.project_root() else {
        return;
    };
    match VaultIndexer::sync_if_changed(store, project, VaultScanOptions::default()) {
        Ok(VaultSync::Unchanged) => {}
        Ok(VaultSync::Reconciled(report)) => eprintln!(
            "athenaeum: the vault changed since it was last indexed; refreshed the index \
             ({} page(s), {} node(s) created, {} updated, {} removed)",
            report.pages, report.nodes_created, report.nodes_updated, report.nodes_removed
        ),
        Ok(VaultSync::NotIndexed) => {
            // Saying this on a project that simply has no vault would be noise; saying it where
            // markdown exists and the index does not is the difference between an empty answer
            // and a wrong one.
            if project.join("wiki").is_dir() {
                eprintln!(
                    "athenaeum: this vault has not been indexed yet, so this answer comes from an \
                     empty index; run `athenaeum index-vault`"
                );
            }
        }
        Err(error) => eprintln!(
            "athenaeum: could not refresh the vault index, answering from the last one: {error}"
        ),
    }
}
