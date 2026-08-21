//! `athenaeum` — command-line access to a knowledge base.
//!
//! This exists to keep the library honest. Every operation the desktop app
//! performs is reachable here, without a window, a frontend build or a model
//! API key, which means the core can be exercised in CI on all three target
//! platforms and debugged by reading the same commands a user would type.
//!
//! It is also the acceptance harness for buildplan §73 — create, read,
//! update, delete, version, validate.

use std::path::PathBuf;
use std::process::ExitCode;

use athenaeum_core::graph;
use athenaeum_core::prelude::*;
use clap::{Args, Parser, Subcommand, ValueEnum};

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

    /// Emit JSON instead of text, for scripting.
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
    /// Find candidate nodes by text.
    Search {
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Walk outward from a node and show what is reachable.
    Navigate(NavigateArgs),
    /// Assemble a knowledge brief for a task.
    Brief(BriefArgs),
    /// Show every archived revision of a node.
    History { id: String },
    /// Read the event journal.
    Journal {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Check the knowledge base for structural problems.
    Check,
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
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("athenaeum: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let cwd = cli
        .project
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    if let Command::Init = cli.command {
        let workspace = Workspace::init(&cwd)?;
        workspace.open()?;
        println!(
            "Initialized knowledge base at {}",
            workspace.root().display()
        );
        return Ok(());
    }

    let workspace =
        Workspace::discover(&cwd).unwrap_or_else(|| Workspace::at(cwd.join(".athenaeum")));
    if !workspace.root().exists() {
        return Err(Error::Blob(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "no knowledge base found from {}; run `athenaeum init` first",
                cwd.display()
            ),
        )));
    }
    let mut store = workspace.open()?;

    match &cli.command {
        Command::Init => unreachable!("handled above"),

        Command::Stats => {
            let stats = store.stats()?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("nodes      {} ({} live)", stats.nodes, stats.live_nodes);
                println!("edges      {}", stats.edges);
                println!("evidence   {}", stats.evidence);
                println!("events     {}", stats.events);
                println!("schema     v{}", stats.schema_version);
                println!("location   {}", workspace.root().display());
            }
        }

        Command::Add(args) => {
            let node_type: NodeType = args.node_type.parse().map_err(|_| Error::InvalidValue {
                field: "node type",
                value: args.node_type.clone(),
            })?;
            let authority: Authority = args.authority.parse().map_err(|_| Error::InvalidValue {
                field: "authority",
                value: args.authority.clone(),
            })?;
            let scope: Scope = args.scope.parse().map_err(|_| Error::InvalidValue {
                field: "scope",
                value: args.scope.clone(),
            })?;

            let mut node = KnowledgeNode::new(node_type, &args.title)
                .with_summary(&args.summary)
                .with_content(&args.content)
                .with_authority(authority)
                .with_scope(scope);
            if let Some(id) = &args.id {
                node = node.with_id(id.as_str());
            }
            if args.stated {
                node = node.with_provenance(Provenance::user());
            }

            let created = store.create_node(node)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&created)?);
            } else {
                println!("{}  {}", created.id, created.title);
            }
        }

        Command::Show { id } => {
            let node = resolve(&store, id)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&node)?);
            } else {
                print_node(&store, &node)?;
            }
        }

        Command::List(args) => {
            let mut filter = if args.all {
                NodeFilter::all()
            } else {
                NodeFilter::live()
            };
            if let Some(raw) = &args.node_type {
                filter = filter.of_type(raw.parse().map_err(|_| Error::InvalidValue {
                    field: "node type",
                    value: raw.clone(),
                })?);
            }
            let nodes = store.list_nodes(&filter.limit(args.limit))?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&nodes)?);
            } else if nodes.is_empty() {
                println!("(nothing yet)");
            } else {
                for node in nodes {
                    println!(
                        "{:<10} {:<40} {:<12} {}",
                        node.node_type,
                        truncate(&node.title, 40),
                        node.status,
                        node.id
                    );
                }
            }
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
            println!("{} is now at version {}", updated.id, updated.version);
        }

        Command::Status { id, status } => {
            let node = resolve(&store, id)?;
            let updated = store.set_node_status(&node.id, (*status).into())?;
            println!("{} → {}", updated.id, updated.status);
        }

        Command::Remove { id } => {
            let node = resolve(&store, id)?;
            store.delete_node(&node.id)?;
            println!("removed {} ({})", node.id, node.title);
        }

        Command::Link(args) => {
            let from = resolve(&store, &args.from)?;
            let to = resolve(&store, &args.to)?;
            let edge_type: EdgeType = args.edge_type.parse().map_err(|_| Error::InvalidValue {
                field: "edge type",
                value: args.edge_type.clone(),
            })?;

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
            println!(
                "{} → {} → {}  [{}]",
                from.title, created.edge_type, to.title, created.status
            );
        }

        Command::Evidence(args) => {
            let node = resolve(&store, &args.node)?;
            let kind: EvidenceKind = args.kind.parse().map_err(|_| Error::InvalidValue {
                field: "evidence kind",
                value: args.kind.clone(),
            })?;

            let mut anchor = SourceAnchor::file(&args.file);
            if let Some(symbol) = &args.symbol {
                anchor = anchor.with_symbol(symbol);
            }
            if let Some(commit) = &args.commit {
                anchor = anchor.with_commit(commit);
            }

            let mut evidence = Evidence::new(node.id.clone(), kind).with_anchor(anchor);
            if let Some(excerpt) = &args.excerpt {
                evidence = evidence.with_excerpt(excerpt);
            }

            let attached = store.attach_evidence(evidence)?;
            println!("attached {} to {}", attached.citation(), node.title);
        }

        Command::Search { query, limit } => {
            let query = query.join(" ");
            let hits = store.search(&query, *limit)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else if hits.is_empty() {
                println!("nothing found for {query:?}");
            } else {
                for hit in hits {
                    println!(
                        "{:>6.2}  {:<40} {}",
                        hit.score,
                        truncate(&hit.node.title, 40),
                        hit.node.id
                    );
                }
            }
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
            // Printed breadth-first, so a node's predecessor is named
            // explicitly rather than implied by indentation — at depth 2 the
            // node directly above is usually a sibling, not the parent.
            for item in reached {
                match &item.via {
                    None => println!("{} [{}]  (seed)", item.node.title, item.node.node_type),
                    Some(step) => {
                        let from = store.get_node(&step.from)?;
                        println!(
                            "{:indent$}{} --{}--> {} [{}]",
                            "",
                            from.title,
                            step.edge_type,
                            item.node.title,
                            item.node.node_type,
                            indent = (item.depth as usize) * 2
                        );
                    }
                }
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

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&brief)?);
            } else if args.prompt {
                println!("{}", brief.to_prompt());
            } else {
                println!("{}", brief.to_markdown());
            }
        }

        Command::History { id } => {
            let node = resolve(&store, id)?;
            let history = store.history(&node.id)?;
            println!("{} (currently v{})", node.title, node.version);
            if history.is_empty() {
                println!("  no earlier revisions");
            }
            for version in history {
                println!(
                    "  v{} {}  {}",
                    version.version,
                    version.recorded_at.format("%Y-%m-%d %H:%M"),
                    version.reason.as_deref().unwrap_or("(no reason recorded)")
                );
            }
        }

        Command::Journal { limit } => {
            for event in store.events(&EventFilter::default().limit(*limit))? {
                println!(
                    "{}  {:<22} {}",
                    event.at.format("%Y-%m-%d %H:%M:%S"),
                    event.kind,
                    event.subject.unwrap_or_default()
                );
            }
        }

        Command::Check => {
            let problems = check(&store)?;
            if problems.is_empty() {
                println!("no problems found");
            } else {
                for problem in &problems {
                    println!("{problem}");
                }
                return Err(Error::InvalidValue {
                    field: "knowledge base",
                    value: format!("{} problem(s)", problems.len()),
                });
            }
        }
    }

    Ok(())
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

/// Structural problems worth reporting: knowledge that has drifted into a
/// state a user would want to know about (§79).
fn check(store: &KnowledgeStore) -> Result<Vec<String>> {
    let mut problems = Vec::new();

    for node in store.list_nodes(&NodeFilter::all())? {
        if node.title.trim().is_empty() {
            problems.push(format!("{}: has no title", node.id));
        }
        if node.summary.trim().is_empty() && node.content.trim().is_empty() {
            problems.push(format!(
                "{} ({}): has no content at all",
                node.id, node.title
            ));
        }
        if node.needs_review() {
            problems.push(format!("{}: {} needs review", node.id, node.title));
        }
        // A rule or decision nobody can justify is the kind of knowledge that
        // quietly becomes wrong (§15).
        if matches!(node.node_type, NodeType::Rule | NodeType::Decision)
            && store.evidence_for(&node.id)?.is_empty()
        {
            problems.push(format!(
                "{} ({}): a {} with no evidence",
                node.id, node.title, node.node_type
            ));
        }
    }

    Ok(problems)
}

fn print_node(store: &KnowledgeStore, node: &KnowledgeNode) -> Result<()> {
    println!("{}", node.title);
    println!("{}", "-".repeat(node.title.len().max(3)));
    println!("id         {}", node.id);
    println!("type       {}", node.node_type);
    println!("status     {}", node.status);
    println!("authority  {}", node.authority);
    println!("scope      {}", node.scope);
    println!("version    {}", node.version);
    println!("source     {}", node.provenance.source);

    if !node.summary.is_empty() {
        println!("\n{}", node.summary);
    }
    if !node.content.is_empty() {
        println!("\n{}", node.content);
    }

    let neighbors = store.neighbors(&node.id, &EdgeFilter::all())?;
    if !neighbors.is_empty() {
        println!("\nrelationships");
        for neighbor in neighbors {
            let other = store.get_node(&neighbor.node)?;
            let marker = if neighbor.edge.is_traversable() {
                " "
            } else {
                "?"
            };
            println!("  {marker} {} → {}", neighbor.edge_type, other.title);
        }
    }

    let evidence = store.evidence_for(&node.id)?;
    if !evidence.is_empty() {
        println!("\nevidence");
        for item in evidence {
            println!("  {} ({})", item.citation(), item.kind);
        }
    }

    let utility = store.utility(&node.id)?;
    if utility.retrievals > 0 {
        println!(
            "\nretrieved {} time(s), used {} time(s)",
            utility.retrievals, utility.uses
        );
    }

    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}
