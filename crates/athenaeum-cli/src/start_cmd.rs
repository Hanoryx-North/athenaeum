//! Local source-checkout launcher. Cached starts never invoke a build tool.
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
    /// Refresh the standalone app with an incremental build (no installers).
    #[arg(long)]
    build: bool,
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
            "launch": {"program": binary, "cwd": repo}, "buildRequired": needs_build}),
            || {
                for step in &steps {
                    println!("[desktop] {} {:?}", step.program, step.args);
                }
                println!("[desktop] launch {}", binary.display());
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
    let pid = spawn_desktop(&binary, &repo)?;
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

#[cfg(not(windows))]
fn spawn_desktop(binary: &Path, cwd: &Path) -> std::io::Result<u32> {
    let mut command = Command::new(binary);
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
fn spawn_desktop(binary: &Path, cwd: &Path) -> std::io::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    let program: Vec<u16> = binary.as_os_str().encode_wide().chain(Some(0)).collect();
    let directory: Vec<u16> = cwd.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut command_line: Vec<u16> = Some(b'"' as u16)
        .into_iter()
        .chain(binary.as_os_str().encode_wide())
        .chain([b'"' as u16, 0])
        .collect();
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
