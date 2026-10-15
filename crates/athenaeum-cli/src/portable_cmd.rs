//! The commands that make a knowledge repository portable: its identity, the proposal lifecycle,
//! export and import, the trust ledger, and the Library.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use athenaeum_core::portable::archive;
use athenaeum_core::portable::ledger::TrustLedger;
use athenaeum_core::portable::manifest::{default_name, RepositoryManifest};
use athenaeum_core::portable::memory;
use athenaeum_core::portable::portable_error;
use athenaeum_core::portable::sync::{
    self, CreateRepository, Keep, Remote, RemoteRepository, SyncEngine, SyncPosition, SyncState,
};
use athenaeum_core::portable::syncset::SyncLimits;
use athenaeum_core::prelude::*;
use athenaeum_core::vault::PageRef;
use clap::{Subcommand, ValueEnum};
use serde::Serialize;

use crate::library::{credentials, LibraryClient, SERVER_ENV};
use crate::render::emit;
use crate::Outcome;

#[derive(Subcommand)]
pub enum RepoCommand {
    /// Give this knowledge repository an identity (`athenaeum.json`) and a workspace.
    Init {
        /// A human name; defaults to the directory's.
        #[arg(long)]
        name: Option<String>,
    },
    /// Show the repository's identity, index and Library link.
    Info,
}

#[derive(Subcommand)]
pub enum VaultCommand {
    /// Write pages for existing user-stated and verified nodes that are only in the database.
    ///
    /// Defaults to a dry run. Never deletes a row, never overwrites a file.
    Materialize {
        /// Actually write the pages.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub enum TrustCommand {
    /// Pages that arrived from a pull or an import and have not been reviewed since.
    List,
    /// Take responsibility for arrived pages, lifting the cap on their declared authority.
    ///
    /// Do this after reading them. Editing a page has the same effect.
    Adopt {
        /// Vault-relative paths, e.g. `wiki/auth.md`.
        paths: Vec<String>,
        /// Adopt everything that arrived.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum LibraryCommand {
    /// Connect to a Library server. Opens no browser: it prints a code and a URL.
    Login {
        /// The server, e.g. `https://library.example`. Defaults to `ATHENAEUM_LIBRARY_URL`.
        #[arg(long)]
        server: Option<String>,
        /// Store an access token you created in the Library's settings instead of using the
        /// browser flow.
        #[arg(long)]
        token: Option<String>,
    },
    /// Forget the stored credential for a server.
    Logout {
        #[arg(long)]
        server: Option<String>,
    },
    /// Which servers are connected, and as whom.
    Status {
        #[arg(long)]
        server: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum SyncCommand {
    /// Link this repository to a Library repository, creating one if asked.
    Link {
        #[arg(long)]
        server: Option<String>,
        /// An existing repository: `owner/name`, or its id.
        #[arg(long, conflicts_with = "create")]
        remote: Option<String>,
        /// Create a new repository with this name.
        #[arg(long)]
        create: Option<String>,
        /// Make a newly created repository public. The default is private.
        #[arg(long, requires = "create")]
        public: bool,
        #[arg(long, requires = "create")]
        description: Option<String>,
    },
    /// Download a Library repository into a new directory and link it.
    Clone {
        /// `owner/name`, or a repository id.
        remote: String,
        destination: PathBuf,
        #[arg(long)]
        server: Option<String>,
    },
    /// Where this repository stands against the Library. Works offline.
    Status,
    /// Bring remote changes in, merging what can be merged and holding what cannot.
    Pull,
    /// Send local changes. Refused while a conflict is open.
    Push {
        #[arg(long, short)]
        message: Option<String>,
    },
    /// Settle a conflicted path.
    Resolve {
        path: String,
        #[arg(long)]
        keep: KeepArg,
    },
    /// Forget the link. Files and history are untouched.
    Unlink,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KeepArg {
    Local,
    Remote,
    Both,
}

impl From<KeepArg> for Keep {
    fn from(value: KeepArg) -> Self {
        match value {
            KeepArg::Local => Keep::Local,
            KeepArg::Remote => Keep::Remote,
            KeepArg::Both => Keep::Both,
        }
    }
}

fn project_of(workspace: &Workspace) -> Result<&Path> {
    workspace.project_root().ok_or_else(|| Error::InvalidValue {
        field: "workspace",
        value: workspace.root().display().to_string(),
    })
}

fn reindex(store: &mut KnowledgeStore, project: &Path) -> Result<VaultIndexReport> {
    VaultIndexer::scan_and_index(store, project, VaultScanOptions::default())
}

// ---------------------------------------------------------------------------
// repo
// ---------------------------------------------------------------------------

pub fn repo_init(json: bool, cwd: &Path, name: Option<&str>) -> Result<Outcome> {
    let workspace = Workspace::init(cwd)?;
    workspace.open()?;
    let name = name.map(str::to_string).unwrap_or_else(|| default_name(cwd));
    let (manifest, created) = RepositoryManifest::read_or_init(cwd, &name)?;
    emit(json, &serde_json::json!({ "manifest": manifest, "created": created }), || {
        if created {
            println!("Created {} for {:?} ({})", athenaeum_core::portable::manifest::MANIFEST_FILE, manifest.name, manifest.repository_id);
        } else {
            println!("{:?} already has an identity: {}", manifest.name, manifest.repository_id);
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoInfo {
    path: String,
    initialized: bool,
    repository_id: Option<String>,
    name: Option<String>,
    format_version: Option<u32>,
    nodes: Option<u64>,
    proposals: Option<usize>,
    pages: Option<usize>,
    remote: Option<RemoteInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteInfo {
    server: String,
    remote_id: String,
    last_synced: Option<String>,
    conflicts: usize,
}

pub fn repo_info(json: bool, cwd: &Path) -> Result<Outcome> {
    let workspace = Workspace::discover(cwd);
    let root = workspace
        .as_ref()
        .and_then(|w| w.project_root().map(Path::to_path_buf))
        .unwrap_or_else(|| cwd.to_path_buf());
    let manifest = RepositoryManifest::read(&root)?;
    let (nodes, proposals, pages) = match &workspace {
        Some(workspace) => {
            let store = workspace.open()?;
            let stats = store.stats()?;
            let proposals = memory::proposals(&store, None)?.len();
            let pages = store
                .list_nodes(&NodeFilter::all())?
                .iter()
                .filter(|n| PageRef::of(n, &[], None).is_some() || is_page_node(n))
                .count();
            (Some(stats.nodes), Some(proposals), Some(pages))
        }
        None => (None, None, None),
    };
    let remote = if manifest.is_some() {
        SyncState::load(&root)?.map(|state| RemoteInfo {
            server: state.server,
            remote_id: state.remote_id,
            last_synced: state.last_synced,
            conflicts: state.conflicts.len(),
        })
    } else {
        None
    };
    let info = RepoInfo {
        path: root.display().to_string(),
        initialized: workspace.is_some(),
        repository_id: manifest.as_ref().map(|m| m.repository_id.clone()),
        name: manifest.as_ref().map(|m| m.name.clone()),
        format_version: manifest.as_ref().map(|m| m.format_version),
        nodes,
        proposals,
        pages,
        remote,
    };
    emit(json, &info, || {
        println!("path         {}", info.path);
        println!("workspace    {}", if info.initialized { "yes" } else { "no (run `athenaeum init`)" });
        match (&info.name, &info.repository_id) {
            (Some(name), Some(id)) => println!("repository   {name} ({id})"),
            _ => println!("repository   no athenaeum.json (a legacy vault; `athenaeum repo init` adds one)"),
        }
        if let Some(nodes) = info.nodes {
            println!("nodes        {nodes} ({} proposal(s) awaiting a decision)", info.proposals.unwrap_or(0));
        }
        match &info.remote {
            Some(remote) => println!("library      {} ({}){}", remote.server, remote.remote_id, if remote.conflicts > 0 { format!(", {} conflict(s)", remote.conflicts) } else { String::new() }),
            None => println!("library      not linked"),
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

fn is_page_node(node: &KnowledgeNode) -> bool {
    node.provenance
        .detail
        .as_deref()
        .is_some_and(|d| d.starts_with(athenaeum_core::vault::PAGE_PROVENANCE))
}

// ---------------------------------------------------------------------------
// the proposal lifecycle
// ---------------------------------------------------------------------------

pub fn proposals(json: bool, store: &mut KnowledgeStore, limit: u32) -> Result<Outcome> {
    let service = WikiService::new(store);
    let proposals = service.proposals(Some(limit))?;
    emit(json, &proposals, || {
        if proposals.is_empty() {
            println!("no proposals awaiting a decision");
        }
        for proposal in &proposals {
            println!(
                "{:<12} {:<40} proposed by {}  {}",
                proposal.node.node_type,
                crate::render::truncate(&proposal.node.title, 40),
                proposal.node.provenance.detail.as_deref().unwrap_or("a model"),
                proposal.node.id
            );
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn accept(json: bool, store: &mut KnowledgeStore, reference: &str, by: &str) -> Result<Outcome> {
    let node = crate::resolve(store, reference)?;
    let accepted = memory::accept(store, &node.id, by)?;
    emit(json, &accepted, || {
        if accepted.already_accepted {
            println!("{:?} was already accepted: {}", accepted.node.title, accepted.page.path);
        } else {
            println!("accepted {:?} → {}", accepted.node.title, accepted.page.path);
            if !accepted.relationships_left_local.is_empty() {
                println!(
                    "  {} relationship(s) stay local until their other end is accepted: {}",
                    accepted.relationships_left_local.len(),
                    accepted.relationships_left_local.join("; ")
                );
            }
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn reject(
    json: bool,
    store: &mut KnowledgeStore,
    reference: &str,
    reason: Option<&str>,
    by: &str,
) -> Result<Outcome> {
    let node = crate::resolve(store, reference)?;
    let rejected = memory::reject(store, &node.id, reason, by)?;
    emit(json, &rejected, || {
        println!("rejected {:?}; it will not be offered again", rejected.title);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn materialize(json: bool, store: &mut KnowledgeStore, apply: bool) -> Result<Outcome> {
    let report = memory::materialize(store, apply)?;
    emit(json, &report, || {
        let verb = if report.dry_run { "would write" } else { "wrote" };
        for item in &report.written {
            println!("{verb} {}  ← {:?}", item.path, item.title);
        }
        for item in &report.left_alone {
            println!("left alone {:?}: {}", item.title, item.reason);
        }
        if report.dry_run {
            println!("dry run: {} page(s) would be written; pass --apply to write them", report.written.len());
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

// ---------------------------------------------------------------------------
// export / import
// ---------------------------------------------------------------------------

pub fn export(json: bool, workspace: &Workspace, output: &Path) -> Result<Outcome> {
    let report = archive::export(project_of(workspace)?, output)?;
    emit(json, &report, || {
        println!("exported {} file(s), {} bytes → {}", report.files, report.bytes, report.path);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn import(json: bool, archive_path: &Path, destination: &Path) -> Result<Outcome> {
    let report = archive::import(archive_path, destination)?;
    let workspace = Workspace::init(destination)?;
    let mut store = workspace.open()?;
    let index = reindex(&mut store, destination)?;
    emit(json, &serde_json::json!({ "import": report, "index": index }), || {
        println!(
            "imported {} file(s) into {} ({}); indexed {} page(s), {} node(s) created",
            report.files, report.destination, report.repository_id, index.pages, index.nodes_created
        );
        if report.capped_files > 0 {
            println!(
                "{} page(s) arrived from outside: their declared authority is capped at \
                 imported_knowledge until you edit or `athenaeum trust adopt` them",
                report.capped_files
            );
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

// ---------------------------------------------------------------------------
// trust
// ---------------------------------------------------------------------------

pub fn trust(json: bool, store: &mut KnowledgeStore, workspace: &Workspace, command: &TrustCommand) -> Result<Outcome> {
    let project = project_of(workspace)?;
    let mut ledger = TrustLedger::load(project)?;
    match command {
        TrustCommand::List => {
            let paths: Vec<&str> = ledger.paths().filter(|p| p.ends_with(".md")).collect();
            emit(json, &paths, || {
                if paths.is_empty() {
                    println!("nothing is waiting for review");
                }
                for path in &paths {
                    println!("{path}");
                }
                Ok(())
            })?;
        }
        TrustCommand::Adopt { paths, all } => {
            let targets: Vec<String> = if *all {
                ledger.paths().map(str::to_string).collect()
            } else {
                paths.clone()
            };
            if targets.is_empty() {
                return Err(portable_error("invalid_request", "name the pages to adopt, or pass --all"));
            }
            let adopted: Vec<&String> = targets.iter().filter(|p| ledger.forget(p)).collect();
            ledger.save(project)?;
            reindex(store, project)?;
            emit(json, &adopted, || {
                println!("adopted {} page(s); their declared authority now applies", adopted.len());
                Ok(())
            })?;
        }
    }
    Ok(Outcome::Ok)
}

// ---------------------------------------------------------------------------
// the Library
// ---------------------------------------------------------------------------

fn server_arg(flag: Option<&str>, state: Option<&SyncState>) -> Result<String> {
    if let Some(server) = flag {
        return Ok(server.to_string());
    }
    if let Some(state) = state.filter(|s| !s.server.is_empty()) {
        return Ok(state.server.clone());
    }
    if let Ok(server) = std::env::var(SERVER_ENV) {
        if !server.trim().is_empty() {
            return Ok(server);
        }
    }
    let stored = credentials::servers();
    if stored.len() == 1 {
        return Ok(stored[0].0.clone());
    }
    Err(portable_error(
        "invalid_request",
        format!("name the Library with --server or {SERVER_ENV}"),
    ))
}

pub fn library(json: bool, command: &LibraryCommand) -> Result<Outcome> {
    match command {
        LibraryCommand::Login { server, token } => {
            let server = server_arg(server.as_deref(), None)?;
            let client = LibraryClient::new(&server, token.clone())?;
            let server = client.server();
            let (token, username) = match token {
                Some(token) => {
                    let who = client.whoami()?;
                    (token.clone(), who.username)
                }
                None => device_login(&client)?,
            };
            let stored_in = credentials::store(&server, &token, Some(&username))?;
            emit(json, &serde_json::json!({ "server": server, "username": username, "storedIn": stored_in }), || {
                println!("connected to {server} as {username}");
                eprintln!(
                    "note: the token is stored in {} (readable only by you); this build does not use \
                     the operating system's credential store",
                    stored_in.display()
                );
                Ok(())
            })?;
        }
        LibraryCommand::Logout { server } => {
            let server = crate::library::normalize_server(&server_arg(server.as_deref(), None)?)?
                .to_string()
                .trim_end_matches('/')
                .to_string();
            let removed = credentials::forget(&server)?;
            emit(json, &serde_json::json!({ "server": server, "removed": removed }), || {
                println!("{}", if removed { "signed out" } else { "there was no stored credential" });
                Ok(())
            })?;
        }
        LibraryCommand::Status { server } => {
            let servers: Vec<(String, Option<String>)> = match server {
                Some(server) => vec![(server.clone(), None)],
                None => credentials::servers(),
            };
            let mut rows = Vec::new();
            for (server, stored_user) in servers {
                let (reachable, username, detail) = match LibraryClient::for_server(&server)
                    .and_then(|client| client.whoami())
                {
                    Ok(who) => (true, Some(who.username), None),
                    Err(error) => (!matches!(&error, Error::Portable { code, .. } if code == "offline"), stored_user, Some(error.to_string())),
                };
                rows.push(serde_json::json!({ "server": server, "username": username, "reachable": reachable, "detail": detail }));
            }
            emit(json, &rows, || {
                if rows.is_empty() {
                    println!("not connected to any Library; run `athenaeum library login --server <url>`");
                }
                for row in &rows {
                    println!("{}  {}  {}", row["server"].as_str().unwrap_or(""), row["username"].as_str().unwrap_or("(unknown)"), row["detail"].as_str().unwrap_or("ok"));
                }
                Ok(())
            })?;
        }
    }
    Ok(Outcome::Ok)
}

/// Link `project` to a Library repository: an existing one (`remote`), or a new one (`create`),
/// private unless `public`. The repository's identity file supplies the id of a new one, so
/// creating twice from the same project cannot make two hosted repositories.
pub fn link_repository(
    project: &Path,
    server: &str,
    remote: Option<&str>,
    create: Option<&str>,
    public: bool,
    description: Option<&str>,
) -> Result<(RemoteRepository, SyncState)> {
    let client = client_for(server)?;
    let repository = match (remote, create) {
        (Some(remote), None) => client.resolve(remote)?,
        (None, Some(name)) => {
            let (manifest, _) = RepositoryManifest::read_or_init(project, &default_name(project))?;
            client.create_repository(&CreateRepository {
                id: Some(manifest.repository_id),
                name: name.to_string(),
                visibility: if public { "public" } else { "private" }.to_string(),
                description: description.map(str::to_string),
            })?
        }
        _ => {
            return Err(portable_error(
                "invalid_request",
                "choose --remote <owner/name|id> to link an existing repository, or --create <name> to make one",
            ))
        }
    };
    let linked = sync::link(project, &client.server(), &repository)?;
    Ok((repository, linked))
}

/// Who `server` knows this machine as: the stored credential if it still works, otherwise a fresh
/// browser sign-in. The flag says whether a sign-in was needed.
pub fn ensure_signed_in(server: &str) -> Result<(String, String, bool)> {
    let client = LibraryClient::for_server(server)?;
    let server = client.server();
    if let Ok(who) = client.whoami() {
        return Ok((server, who.username, false));
    }
    let fresh = LibraryClient::new(&server, None)?;
    let (token, username) = device_login(&fresh)?;
    credentials::store(&server, &token, Some(&username))?;
    Ok((server, username, true))
}

fn device_login(client: &LibraryClient) -> Result<(String, String)> {
    let started = client.start_device_flow()?;
    let uri = started
        .verification_uri_complete
        .clone()
        .unwrap_or_else(|| started.verification_uri.clone());
    if std::env::var("ATHENAEUM_PROGRESS").is_ok_and(|mode| mode == "json") {
        // A program driving this command (the desktop app) shows the code itself.
        eprintln!(
            "{}",
            serde_json::json!({ "event": "device_code", "url": uri, "code": started.user_code, "expiresIn": started.expires_in })
        );
    } else {
        eprintln!("To connect, open this address in a browser where you are signed in to the Library:");
        eprintln!("\n    {uri}\n");
        eprintln!("and confirm the code  {}", started.user_code);
        eprintln!("Waiting for approval (Ctrl-C to cancel)…");
    }
    let deadline = Instant::now() + Duration::from_secs(started.expires_in);
    let mut interval = Duration::from_secs(started.interval.max(1));
    while Instant::now() < deadline {
        std::thread::sleep(interval);
        match client.poll_device_flow(&started.device_code) {
            Ok(Some(token)) => {
                let username = match token.username {
                    Some(username) => username,
                    None => LibraryClient::new(&client.server(), Some(token.access_token.clone()))?
                        .whoami()?
                        .username,
                };
                return Ok((token.access_token, username));
            }
            Ok(None) => interval = (interval + Duration::from_secs(1)).min(Duration::from_secs(15)),
            Err(error) => return Err(error),
        }
    }
    Err(portable_error("expired_token", "the code expired before it was approved; run login again"))
}

// ---------------------------------------------------------------------------
// sync
// ---------------------------------------------------------------------------

fn client_for(server: &str) -> Result<LibraryClient> {
    LibraryClient::for_server(server)
}

fn limits_of(client: &LibraryClient) -> SyncLimits {
    client
        .info()
        .ok()
        .and_then(|info| info.limits)
        .unwrap_or_default()
}

fn describe_offline(error: &Error) -> bool {
    matches!(error, Error::Portable { code, .. } if code == "offline")
}

pub fn sync(
    json: bool,
    cwd: &Path,
    command: Option<&SyncCommand>,
    default_message: Option<&str>,
) -> Result<Outcome> {
    // Commands that create the directory they work in cannot open a workspace first.
    if let Some(SyncCommand::Clone { remote, destination, server }) = command {
        return clone(json, remote, destination, server.as_deref());
    }

    let workspace = Workspace::discover(cwd).ok_or_else(|| Error::NoWorkspace {
        path: cwd.display().to_string(),
    })?;
    let project = project_of(&workspace)?.to_path_buf();
    let state = SyncState::load(&project).ok().flatten();

    match command {
        Some(SyncCommand::Link { server, remote, create, public, description }) => {
            let server = server_arg(server.as_deref(), state.as_ref())?;
            let (repository, linked) = link_repository(
                &project,
                &server,
                remote.as_deref(),
                create.as_deref(),
                *public,
                description.as_deref(),
            )?;
            emit(json, &linked, || {
                println!("linked to {} ({}) on {}", repository.name, repository.id, linked.server);
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        Some(SyncCommand::Unlink) => {
            let removed = sync::unlink(&project)?;
            emit(json, &serde_json::json!({ "unlinked": removed }), || {
                println!("{}", if removed { "unlinked" } else { "it was not linked" });
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        Some(SyncCommand::Status) | Some(SyncCommand::Pull) | Some(SyncCommand::Push { .. }) | Some(SyncCommand::Resolve { .. }) | None => {
            let Some(state) = state else {
                return Err(portable_error(
                    "not_linked",
                    "this repository is not linked to a Library; run `athenaeum sync link --create <name>` or `--remote <owner/name>`",
                ));
            };
            let client = client_for(&state.server)?;
            let engine = SyncEngine::new(&project, &client).with_limits(limits_of(&client));
            let mut store = workspace.open()?;

            let result = run_sync(json, &engine, &mut store, &project, command, default_message);
            match result {
                Err(error) if describe_offline(&error) && !matches!(command, Some(SyncCommand::Status)) => {
                    emit(json, &serde_json::json!({ "status": "offline", "message": error.to_string() }), || {
                        eprintln!("offline: {error}");
                        Ok(())
                    })?;
                    Ok(Outcome::BridgeError)
                }
                other => other,
            }
        }
        Some(SyncCommand::Clone { .. }) => unreachable!("handled above"),
    }
}

fn run_sync(
    json: bool,
    engine: &SyncEngine<'_, LibraryClient>,
    store: &mut KnowledgeStore,
    project: &Path,
    command: Option<&SyncCommand>,
    default_message: Option<&str>,
) -> Result<Outcome> {
    match command {
        Some(SyncCommand::Status) => {
            let status = engine.status()?;
            emit(json, &status, || {
                match status.position {
                    SyncPosition::NotLinked => println!("not linked"),
                    SyncPosition::Offline => println!("offline — {}", status.detail.as_deref().unwrap_or("the Library cannot be reached")),
                    SyncPosition::Conflicted => println!("conflicted — resolve: {}", status.conflicts.join(", ")),
                    SyncPosition::UpToDate => println!("up to date"),
                    SyncPosition::Ahead => println!("ahead — {} local change(s) to push", status.pending.len()),
                    SyncPosition::Behind => println!("behind — the Library has changes to pull"),
                    SyncPosition::Diverged => println!("diverged — {} local change(s), and the Library has changes to pull", status.pending.len()),
                }
                for path in status.pending.added.iter() { println!("  + {path}"); }
                for path in status.pending.modified.iter() { println!("  ~ {path}"); }
                for path in status.pending.deleted.iter() { println!("  - {path}"); }
                for skipped in &status.skipped {
                    println!("  (stays local: {} — {})", skipped.path, skipped.reason);
                }
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        Some(SyncCommand::Resolve { path, keep }) => {
            engine.resolve(path, (*keep).into())?;
            reindex(store, project)?;
            emit(json, &serde_json::json!({ "resolved": path }), || {
                println!("{path} resolved");
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        Some(SyncCommand::Pull) => {
            let outcome = engine.pull()?;
            after_pull(json, store, project, &outcome)
        }
        Some(SyncCommand::Push { message }) => {
            let outcome = engine.push(message.as_deref().or(default_message).unwrap_or("Sync from athenaeum"))?;
            emit(json, &outcome, || {
                if outcome.nothing_to_push {
                    println!("nothing to push");
                } else {
                    println!(
                        "pushed {} change(s) as {}",
                        outcome.pushed.len(),
                        outcome.revision.as_deref().unwrap_or("?")
                    );
                }
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        // `athenaeum sync` on its own: pull, then push what is left.
        None => {
            let pulled = engine.pull()?;
            let index = reindex_after_pull(store, project, &pulled)?;
            if !json {
                print_pull(&pulled, index.as_ref());
            }
            if !pulled.conflicts.is_empty() {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "pull": pulled, "index": index, "push": null }))?
                    );
                }
                return Ok(Outcome::ProblemsFound);
            }
            let pushed = engine.push(default_message.unwrap_or("Sync from athenaeum"))?;
            emit(json, &serde_json::json!({ "pull": pulled, "index": index, "push": pushed }), || {
                if pushed.nothing_to_push {
                    println!("nothing to push");
                } else {
                    println!("pushed {} change(s) as {}", pushed.pushed.len(), pushed.revision.as_deref().unwrap_or("?"));
                }
                Ok(())
            })?;
            Ok(Outcome::Ok)
        }
        _ => unreachable!("handled by the caller"),
    }
}

/// Rebuild the derived index after a pull changed files. Returns the report, if there was one.
fn reindex_after_pull(
    store: &mut KnowledgeStore,
    project: &Path,
    outcome: &sync::PullOutcome,
) -> Result<Option<VaultIndexReport>> {
    // The index is derived: after a pull it is rebuilt from the downloaded text alone.
    if outcome.changed {
        Ok(Some(reindex(store, project)?))
    } else {
        Ok(None)
    }
}

fn print_pull(outcome: &sync::PullOutcome, index: Option<&VaultIndexReport>) {
    if outcome.up_to_date {
        println!("already up to date");
        return;
    }
    println!(
        "pulled {}: {} updated, {} merged, {} deleted",
        outcome.revision.as_deref().unwrap_or("?"),
        outcome.updated.len(),
        outcome.merged.len(),
        outcome.deleted.len()
    );
    if let Some(index) = index {
        println!("rebuilt the local index: {} page(s), {} node(s) created", index.pages, index.nodes_created);
    }
    if !outcome.updated.is_empty() {
        println!("pulled pages are capped at imported_knowledge until you edit or `athenaeum trust adopt` them");
    }
    for path in &outcome.conflicts {
        println!("CONFLICT {path}: left as it is. Resolve with `athenaeum sync resolve {path} --keep local|remote|both`");
    }
}

fn after_pull(
    json: bool,
    store: &mut KnowledgeStore,
    project: &Path,
    outcome: &sync::PullOutcome,
) -> Result<Outcome> {
    let index = reindex_after_pull(store, project, outcome)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "pull": outcome, "index": index }))?);
    } else {
        print_pull(outcome, index.as_ref());
    }
    Ok(if outcome.conflicts.is_empty() { Outcome::Ok } else { Outcome::ProblemsFound })
}

fn clone(json: bool, remote: &str, destination: &Path, server: Option<&str>) -> Result<Outcome> {
    let server = server_arg(server, None)?;
    let client = client_for(&server)?;
    let repository: RemoteRepository = client.resolve(remote)?;
    if destination.exists() && std::fs::read_dir(destination)?.next().is_some() {
        return Err(portable_error(
            "conflict",
            format!("{} is not empty; a clone never overwrites", destination.display()),
        ));
    }
    std::fs::create_dir_all(destination)?;
    let engine = SyncEngine::new(destination, &client).with_limits(limits_of(&client));
    let outcome = engine.clone_into(&repository, &client.server())?;

    let workspace = Workspace::init(destination)?;
    let mut store = workspace.open()?;
    let index = reindex(&mut store, destination)?;
    emit(json, &serde_json::json!({ "repository": repository, "pull": outcome, "index": index }), || {
        println!(
            "cloned {} into {}: {} file(s); indexed {} page(s)",
            repository.name,
            destination.display(),
            outcome.updated.len(),
            index.pages
        );
        println!("cloned pages are capped at imported_knowledge until you edit or `athenaeum trust adopt` them");
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn draft_source(
    json: bool,
    store: &mut KnowledgeStore,
    source: &str,
    title: &str,
) -> Result<Outcome> {
    let drafted = memory::draft_from_source(store, source, title)?;
    emit(json, &drafted, || {
        println!(
            "proposed {:?} from {} — review it with `athenaeum proposals`, then `accept` or `reject`",
            drafted.node.title, source
        );
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

pub fn import_legacy_core(json: bool, store: &mut KnowledgeStore, apply: bool) -> Result<Outcome> {
    let report = memory::import_legacy_core(store, apply)?;
    emit(json, &report, || {
        if !report.found {
            println!("no retired athenaeum-core index (.athenaeum/local-core.db) here; nothing to import");
            return Ok(());
        }
        let verb = if report.dry_run { "would import" } else { "imported" };
        println!("{verb} {} pending proposal(s) from .athenaeum/local-core.db", report.imported.len());
        for item in &report.imported {
            println!("  {:<40} {} evidence, {} relationship(s)", crate::render::truncate(&item.title, 40), item.evidence, item.relationships);
        }
        for note in &report.left_alone {
            println!("  left alone: {note}");
        }
        if report.dry_run && !report.imported.is_empty() {
            println!("re-run with --apply to import them; the old database is never changed");
        }
        Ok(())
    })?;
    Ok(Outcome::Ok)
}
