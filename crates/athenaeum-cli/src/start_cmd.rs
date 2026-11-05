//! Source work stays live; explicit standalone starts reuse the offline snapshot.
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::{Command, Stdio};

use athenaeum_core::portable::portable_error;
use athenaeum_core::Result;
use clap::Args;
use serde_json::json;

use crate::{dev_cmd, process, Outcome};

#[derive(Args)]
pub struct StartArgs {
    /// Parent of the source repositories. Also read from WIKI_DEV_ROOT.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Run the live desktop in the foreground (Vite UI updates, incremental Rust).
    #[arg(long, conflicts_with = "build")]
    dev: bool,
    /// Open the frozen offline snapshot instead of the live source desktop.
    #[arg(long, conflicts_with = "dev")]
    standalone: bool,
    /// Refresh the standalone app with an incremental build (no installers).
    #[arg(long)]
    build: bool,
    /// Show the opening account/local workflow without clearing saved data.
    #[arg(long, conflicts_with = "dev")]
    onboarding: bool,
    /// Show the build (if needed) and launch plan without executing it.
    #[arg(long)]
    dry_run: bool,
}

pub fn run(json_out: bool, cwd: &Path, args: &StartArgs) -> Result<Outcome> {
    let root = dev_cmd::root(cwd, args.root.as_deref())?;
    let repo = root.join("athenaeum");
    if args.dev {
        if json_out && !args.dry_run {
            return Err(portable_error(
                "invalid_request",
                "start --dev is a foreground stream; omit --json, or use --dry-run",
            ));
        }
        return process::run_steps(json_out, args.dry_run, &[dev_cmd::desktop_serve(&root)?]);
    }
    if !args.standalone && !args.build && !args.onboarding {
        let node = process::executable("node").ok_or_else(|| portable_error("missing_tool", "Node is required for live source development; use wiki start --standalone for the cached app"))?;
        let step = process::Step::new(
            "desktop",
            &repo,
            &node.to_string_lossy(),
            &["scripts/live-desktop.mjs", "--supervise"],
        );
        if args.dry_run {
            return process::run_steps(json_out, true, &[step]);
        }
        return start_live(json_out, &repo, &node);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .filter(|value| !value.is_empty())
        .map(|value| repo.join(value))
        .unwrap_or_else(|| repo.join("target"));
    let executable = if cfg!(windows) {
        "athenaeum-app.exe"
    } else {
        "athenaeum-app"
    };
    // Live Tauri development replaces target/debug with a Vite-dependent app.
    // Keep the embedded-frontend snapshot separate so it always opens offline.
    let snapshot = target.join("standalone").join(executable);
    let release = target.join("release").join(executable);
    let binary = if !args.build && !snapshot.is_file() && release.is_file() {
        release
    } else {
        snapshot
    };
    let needs_build = args.build || !binary.is_file();
    let steps = if needs_build {
        vec![dev_cmd::desktop_snapshot(&root)?]
    } else {
        Vec::new()
    };
    if args.dry_run {
        crate::emit(
            json_out,
            &json!({"dryRun": true, "steps": steps,
            "launch": {"program": binary, "cwd": repo,
                "args": if args.onboarding { vec!["--onboarding"] } else { Vec::new() }},
            "buildRequired": needs_build}),
            || {
                for step in &steps {
                    println!("[desktop] {} {:?}", step.program, step.args);
                }
                println!(
                    "[desktop] launch {}{}",
                    binary.display(),
                    if args.onboarding { " --onboarding" } else { "" }
                );
                Ok(())
            },
        )?;
        return Ok(Outcome::Ok);
    }
    if needs_build {
        eprintln!("[desktop] Preparing the standalone app with the incremental build cache. Later wiki start launches it directly.");
        // Build logs go to stderr; stdout remains one JSON launch report.
        let outcome = process::run_build_steps(&steps)?;
        if matches!(outcome, Outcome::ProblemsFound) {
            return Ok(outcome);
        }
    }
    if !binary.is_file() {
        return Err(portable_error("missing_tool", format!(
            "Desktop executable missing at {}. Run wiki start --build to prepare the standalone app. If using a custom Cargo cache, set CARGO_TARGET_DIR to it.",
            binary.display()
        )));
    }
    let arguments = if args.onboarding {
        vec!["--onboarding"]
    } else {
        Vec::new()
    };
    let pid = spawn_detached(&binary, &repo, &arguments)?;
    crate::emit(
        json_out,
        &json!({"started": true, "pid": pid,
        "binary": binary, "built": needs_build}),
        || {
            println!("Opened Athenaeum (PID {pid}).");
            Ok(())
        },
    )?;
    Ok(Outcome::Ok)
}

fn start_live(json_out: bool, repo: &Path, node: &Path) -> Result<Outcome> {
    let directory = repo.join(".athenaeum-dev");
    std::fs::create_dir_all(&directory)?;
    let existing = std::fs::read(directory.join("live.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let active = existing.and_then(|saved| {
        let port = u16::try_from(saved["port"].as_u64()?).ok()?;
        let token = saved["token"].as_str()?;
        let status: serde_json::Value = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_millis(1500))
            .build()
            .ok()?
            .get(format!("http://127.0.0.1:{port}/status"))
            .bearer_auth(token)
            .send()
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .ok()?;
        let root = PathBuf::from(status["root"].as_str()?)
            .canonicalize()
            .ok()?;
        (root == repo.canonicalize().ok()? && status["pid"] == saved["pid"]).then_some(status)
    });
    let log = directory.join("live.log");
    let report = match active {
        Some(status) => json!({"live": true, "reused": true, "pid": status["pid"], "logPath": log}),
        None => {
            let pid = spawn_detached(node, repo, &["scripts/live-desktop.mjs", "--supervise"])?;
            json!({"live": true, "started": true, "pid": pid, "logPath": log,
                "note": "Starting the live desktop. Inspect wiki desktop status for frontend readiness."})
        }
    };
    crate::emit(json_out, &report, || {
        println!("{}", serde_json::to_string_pretty(&report)?);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}

#[cfg(not(windows))]
fn spawn_detached(binary: &Path, cwd: &Path, arguments: &[&str]) -> std::io::Result<u32> {
    let mut command = Command::new(binary);
    command.args(arguments);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // The GUI must outlive the shell/process group that opened it.
        command.process_group(0);
    }
    Ok(command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?
        .id())
}

#[cfg(windows)]
fn spawn_detached(binary: &Path, cwd: &Path, arguments: &[&str]) -> std::io::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    let program: Vec<u16> = binary.as_os_str().encode_wide().chain(Some(0)).collect();
    // Node resolves its entrypoint incorrectly when GetCurrentDirectory retains
    // Rust canonicalize's Windows device prefix. Use the ordinary spelling.
    let cwd_text = cwd.to_string_lossy();
    let directory_text = cwd_text.strip_prefix(r"\\?\").unwrap_or(&cwd_text);
    let directory: Vec<u16> = directory_text.encode_utf16().chain(Some(0)).collect();
    let mut command_line = vec![b'"' as u16];
    command_line.extend(binary.as_os_str().encode_wide());
    command_line.push(b'"' as u16);
    for argument in arguments {
        // All callers supply fixed launcher flags/relative filenames, never shell text.
        if argument.contains('"') || argument.ends_with('\\') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid launcher argument",
            ));
        }
        command_line.extend(format!(" \"{argument}\"").encode_utf16());
    }
    command_line.push(0);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut child = PROCESS_INFORMATION::default();
    // Do not inherit *any* handles: PowerShell's pipeline handles can otherwise
    // keep the caller waiting for the GUI to exit, even with null stdio.
    // SAFETY: All strings are NUL-terminated and live across the call. The command
    // line is writable, and the initialized output struct receives owned handles.
    let created = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            0,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            null(),
            directory.as_ptr(),
            &startup,
            &mut child,
        )
    };
    if created == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: Successful CreateProcessW returned these two valid, owned handles.
    unsafe {
        CloseHandle(child.hThread);
        CloseHandle(child.hProcess);
    }
    Ok(child.dwProcessId)
}
