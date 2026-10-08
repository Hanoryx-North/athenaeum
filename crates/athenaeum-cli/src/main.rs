//! `athenaeum` — command-line access to a knowledge base.
//!
//! This exists to keep the library honest. Every operation the desktop app
//! performs is reachable here, without a window, a frontend build or a model
//! API key, which means the core can be exercised in CI on all three target
//! platforms and debugged by reading the same commands a user would type.
//!
//! It is also the acceptance harness for buildplan §73 — create, read,
//! update, delete, version, validate.
//!
//! This file defines arguments and dispatches. Rendering lives in
//! [`render`]; knowledge-base validation lives in [`check`].

mod bridge;
mod library;
mod portable_cmd;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use athenaeum_core::graph;
use athenaeum_core::prelude::*;
use athenaeum_core::vault::{lint, LintOptions, LintSeverity, PageRef};
use clap::{Args, Parser, Subcommand, ValueEnum};

use portable_cmd::{LibraryCommand, RepoCommand, SyncCommand, TrustCommand, VaultCommand};
use render::emit;

#[derive(Parser)]
#[command(
    name = "athenaeum",
    version,
    about = "A local, navigable knowledge graph for a codebase."
)]
struct Cli {
    /// Project directory. Defaults to the nearest ancestor containing
    /// `.athenaeum/`, or the current directory.
    #[arg(long, short = 'C', global = true)]
    project: Option<PathBuf>,

    /// Emit JSON instead of text, for scripting. Honoured by every command.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create `.athenaeum/` in the project directory.
    Init,
    /// Show what the knowledge base contains.
    Stats,
    /// Add a node.
    Add(AddArgs),
    /// Print a node with its evidence and relationships.
    Show { id: String },
    /// List nodes.
    List(ListArgs),
    /// Replace a node's summary or content, archiving the previous version.
    Edit(EditArgs),
    /// Change a node's lifecycle status.
    Status { id: String, status: StatusArg },
    /// Delete a node, its edges and its history.
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
    /// neighbours cannot give.
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
    Index(IndexArgs),
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
    /// Operations on the vault itself: `vault materialize`.
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    /// Write the repository to a zip: text, sources and a manifest of hashes, never the index.
    Export { output: PathBuf },
    /// Create a repository from an export. Never overwrites; indexes when done.
    Import { archive: PathBuf, destination: PathBuf },
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
    #[arg(long)]
    note: Option<String>,
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
                // A script asked for JSON, so a failure is JSON too, with a code it can branch on.
                let code = match &err {
                    Error::Portable { code, .. } => code.clone(),
                    Error::NoWorkspace { .. } => "workspace_not_found".to_string(),
                    Error::NodeNotFound(_) => "not_found".to_string(),
                    _ => "error".to_string(),
                };
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
        println!(
            "Initialized knowledge base at {}",
            workspace.root().display()
        );
        return Ok(Outcome::Ok);
    }

    // These work on a directory that has no workspace yet, or create the one they work in.
    match &cli.command {
        Command::Repo { command: RepoCommand::Init { name } } => {
            return portable_cmd::repo_init(cli.json, &cwd, name.as_deref());
        }
        Command::Repo { command: RepoCommand::Info } => {
            return portable_cmd::repo_info(cli.json, &cwd);
        }
        Command::Import { archive, destination } => {
            return portable_cmd::import(cli.json, archive, destination);
        }
        Command::Library { command } => return portable_cmd::library(cli.json, command),
        Command::Sync { command, message } => {
            return portable_cmd::sync(cli.json, &cwd, command.as_ref(), message.as_deref());
        }
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
                if let Err(error) = athenaeum_core::portable::memory::materialize_stated(
                    &mut store,
                    &created.id,
                ) {
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

        Command::Status { id, status } => {
            let node = resolve(&store, id)?;
            let updated = store.set_node_status(&node.id, (*status).into())?;
            emit(cli.json, &updated, || {
                println!("{} → {}", updated.id, updated.status);
                Ok(())
            })?;
        }

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
            emit(cli.json, &path, || {
                render::path(&store, &from, &to, path.as_deref())
            })?;
            if path.is_none() {
                return Ok(Outcome::ProblemsFound);
            }
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

        Command::Repo { .. } | Command::Import { .. } | Command::Library { .. } | Command::Sync { .. } => {
            unreachable!("handled before the workspace is opened")
        }

        Command::Proposals { limit } => return portable_cmd::proposals(cli.json, &mut store, *limit),
        Command::Accept { id, by } => return portable_cmd::accept(cli.json, &mut store, id, by),
        Command::Reject { id, reason, by } => {
            return portable_cmd::reject(cli.json, &mut store, id, reason.as_deref(), by)
        }
        Command::Vault { command: VaultCommand::Materialize { apply } } => {
            return portable_cmd::materialize(cli.json, &mut store, *apply)
        }
        Command::Export { output } => return portable_cmd::export(cli.json, &workspace, output),
        Command::Trust { command } => {
            return portable_cmd::trust(cli.json, &mut store, &workspace, command)
        }

        Command::Index(args) => {
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

        Command::Check(args) => {
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
