#!/usr/bin/env python3
"""Apply the small, reviewable Athenaeum delta to the pinned Codex snapshot."""

import hashlib
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile


ROOT = Path(__file__).resolve().parent
if os.name == "nt" and not str(ROOT).startswith("\\\\?\\"):
    ROOT = Path("\\\\?\\" + str(ROOT))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from-checkout", type=Path, help="Regenerate the port from the exact recorded Git revision")
    args = parser.parse_args()
    upstream = json.loads((ROOT / "upstream.json").read_text(encoding="utf-8"))
    source = ROOT / "codex"
    if args.from_checkout:
        # Regenerate from original Git bytes, not text decoded with the host's locale.
        archive = subprocess.check_output([
            "git", "-C", str(args.from_checkout), "archive", upstream["revision"], *upstream["paths"]
        ])
        with tarfile.open(fileobj=io.BytesIO(archive)) as snapshot:
            originals = {name: snapshot.extractfile(name).read() for name in upstream["files"]}
        del archive
        current_port = json.loads((ROOT / "port.json").read_text(encoding="utf-8"))
        for name, digest in upstream["files"].items():
            expected = current_port["modified"].get(name, {}).get("sha256", digest)
            if hashlib.sha256((source / name).read_bytes()).hexdigest() != expected:
                raise ValueError(f"Unrecorded source changes: {name}")
    else:
        originals = {name: (source / name).read_bytes() for name in upstream["files"]}
    changes = {}
    reasons = {}
    for name, digest in upstream["files"].items():
        data = originals[name]
        if hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"Source is already modified: {name}")
        if b"CODEX_HOME" in data or b"CODEX_SQLITE_HOME" in data:
            try:
                data.decode("utf-8")
            except UnicodeDecodeError:
                continue
            changes[name] = data.replace(b"CODEX_HOME", b"ATHENAEUM_AGENT_HOME").replace(
                b"CODEX_SQLITE_HOME", b"ATHENAEUM_AGENT_SQLITE_HOME"
            )
            reasons[name] = ["Isolate Athenaeum state, including helper processes, from Codex"]

    def replace(name, before, after, reason, count=1):
        original = changes.get(name, originals[name]).replace(b"\r\n", b"\n")
        before, after = before.encode(), after.encode()
        if original.count(before) != count:
            raise ValueError(f"Expected {count} port anchors in {name}: {before!r}")
        changes[name] = original.replace(before, after)
        reasons.setdefault(name, []).append(reason)

    replace("codex-rs/utils/home-dir/src/lib.rs", '".codex"', '".athenaeum-agent"',
            "Use a separate default user home and update the upstream expectation", count=2)
    replace("codex-rs/utils/home-dir/src/lib.rs", "`~/.codex`", "`~/.athenaeum-agent`",
            "Document the native home")
    replace("codex-rs/cli/Cargo.toml", 'default-run = "codex"',
            'default-run = "athenaeum-agent"\n\n[[bin]]\nname = "athenaeum-agent"\npath = "src/athenaeum_main.rs"',
            "Build an Athenaeum executable from the runtime itself")
    replace("codex-rs/cli/src/main.rs", '    bin_name = "codex",',
            '    name = "athenaeum-agent",\n    bin_name = "athenaeum-agent",', "Brand the entry point")
    replace("codex-rs/cli/src/main.rs",
            'override_usage = "codex [OPTIONS] [PROMPT]\\n       codex [OPTIONS] <COMMAND> [ARGS]"',
            'override_usage = "athenaeum-agent [OPTIONS] [PROMPT]\\n       athenaeum-agent [OPTIONS] <COMMAND> [ARGS]"',
            "Show the native command in help")
    replace("codex-rs/cli/src/main.rs", "/// Codex CLI\n", "/// Athenaeum coding agent (Codex runtime)\n",
            "Identify Athenaeum in terminal help")
    replace("codex-rs/cli/src/main.rs", "/// Run Codex non-interactively.", "/// Run Athenaeum non-interactively.",
            "Identify the headless command")
    replace("codex-rs/cli/src/main.rs", "/// Launch the Desktop app (opens the app installer if missing).",
            "/// Show how to open the Athenaeum desktop.", "Describe the native desktop command")
    replace("codex-rs/cli/src/main.rs", "/// Update Codex to the latest version.",
            "/// Show how to update the source-built Athenaeum runtime.", "Describe source-based updates")
    replace("codex-rs/cli/src/main.rs", "    // Retain the launch target through TUI exit,",
            "    // Athenaeum uses its embedded server, never the upstream auto-updating daemon.\n"
            "    interactive.no_daemon = true;\n    // Retain the launch target through TUI exit,",
            "Keep native sessions in the source-built runtime")
    update_file = "codex-rs/cli/src/main.rs"
    update_text = changes[update_file].decode()
    action_start = update_text.index("fn run_update_action(\n")
    action_end = update_text.index("\n#[cfg(windows)]\nfn resolve_windows_update_command_from_path", action_start)
    update_text = (update_text[:action_start] +
        'fn run_update_action(\n    _action: UpdateAction,\n'
        '    _cli_executable: Option<&std::path::Path>,\n) -> anyhow::Result<()> {\n'
        '    run_update_command()\n}\n' + update_text[action_end:])
    start = update_text.index("fn run_update_command() -> anyhow::Result<()> {")
    end = update_text.index("\nfn run_execpolicycheck", start)
    changes[update_file] = (update_text[:start] +
        'fn run_update_command() -> anyhow::Result<()> {\n'
        '    anyhow::bail!("Update Athenaeum Agent from its source or Athenaeum installer; '
        'upstream Codex updates cannot replace this runtime.")\n}\n' + update_text[end:]).encode()
    reasons[update_file].append("Prevent an upstream updater replacing the native executable")
    replace(update_file,
            '#[cfg(any(target_os = "macos", target_os = "windows"))]\nmod desktop_app;\n', '',
            "Do not compile the upstream desktop installer into the native entry point")
    replace(update_file, '#[cfg(windows)]\nfn resolve_windows_update_command_from_path(',
            '#[cfg(all(windows, test))]\nfn resolve_windows_update_command_from_path(',
            "Retain the upstream updater helper only for its preserved tests")
    app_file = "codex-rs/cli/src/app_cmd.rs"
    app_text = originals[app_file].decode("utf-8").replace("\r\n", "\n")
    app_start = app_text.index("pub async fn run_app(cmd: AppCommand) -> anyhow::Result<()> {")
    changes[app_file] = (app_text[:app_start] +
        'pub async fn run_app(_cmd: AppCommand) -> anyhow::Result<()> {\n'
        '    anyhow::bail!("Open the Athenaeum desktop with `wiki start`.")\n}\n').encode()
    reasons[app_file] = ["Direct users to Athenaeum instead of launching or installing Codex Desktop"]
    replace("codex-rs/tui/src/history_cell/session.rs", "OpenAI Codex", "Athenaeum Agent (Codex runtime)",
            "Identify the terminal product and its source", count=2)
    replace("codex-rs/core/Cargo.toml", "[dependencies]\n",
            '[dependencies]\nathenaeum-runtime-config = { path = "../../../athenaeum-config" }\n',
            "Attach product policy without importing the knowledge engine")
    replace("codex-rs/core/src/config/mod.rs",
            "        let developer_instructions = developer_instructions.or(cfg.developer_instructions);",
            "        let developer_instructions = Some(athenaeum_runtime_config::developer_instructions(\n"
            "            developer_instructions.or(cfg.developer_instructions),\n        ));",
            "Attach native identity and the recall/propose contract, preserving user instructions")
    replace("codex-rs/core/src/config/mod.rs",
            "        let mcp_servers = constrain_mcp_servers(cfg.mcp_servers.clone(), mcp_servers.as_ref())",
            '        let mut configured_mcp_servers = cfg.mcp_servers.clone();\n'
            '        if !configured_mcp_servers.contains_key("athenaeum") {\n'
            '            let wiki = std::env::var("ATHENAEUM_BIN").unwrap_or_else(|_| "wiki".to_string());\n'
            '            let server = athenaeum_runtime_config::wiki_server(&wiki)?;\n'
            '            let server = serde_json::from_value(server)\n'
            '                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;\n'
            '            configured_mcp_servers.insert("athenaeum".to_string(), server);\n'
            '        }\n'
            '        let mcp_servers = constrain_mcp_servers(configured_mcp_servers, mcp_servers.as_ref())',
            "Supply optional proposal-only wiki MCP before applying upstream policy constraints")
    replace("codex-rs/core/src/config/mod.rs",
            "cfg.check_for_update_on_startup.unwrap_or(true)", "cfg.check_for_update_on_startup.unwrap_or(false)",
            "Disable upstream auto-update checks by default")
    replace("codex-rs/core/src/config/mod.rs",
            "analytics_enabled: cfg.analytics.as_ref().and_then(|a| a.enabled),",
            "analytics_enabled: Some(cfg.analytics.as_ref().and_then(|a| a.enabled).unwrap_or(false)),",
            "Default upstream analytics off while preserving explicit configuration")

    # A local package adds no registry resolution. Keep the imported dependency pins intact.
    lock_file = "codex-rs/Cargo.lock"
    text = changes.get(lock_file, originals[lock_file]).decode("utf-8").replace("\r\n", "\n")
    anchor = '\nname = "codex-core"\n'
    start = text.index(anchor)
    deps = text.index('dependencies = [\n', start) + len('dependencies = [\n')
    text = text[:deps] + ' "athenaeum-runtime-config",\n' + text[deps:]
    text += '\n[[package]]\nname = "athenaeum-runtime-config"\nversion = "0.1.0"\ndependencies = [\n "serde_json",\n]\n'
    changes[lock_file] = text.encode()
    reasons[lock_file] = ["Lock the local Athenaeum configuration package; preserve all upstream versions"]

    additions = {"codex-rs/cli/src/athenaeum_main.rs":
                 b'// Athenaeum executable: compiles the complete imported runtime in-process.\ninclude!("main.rs");\n'}
    for name, data in {**changes, **additions}.items():
        target = source / name
        if not target.is_file() or target.read_bytes() != data:
            target.write_bytes(data)
    record = {
        "upstreamRevision": upstream["revision"],
        "modified": {name: {"sha256": hashlib.sha256(data).hexdigest(), "reasons": reasons[name]}
                     for name, data in sorted(changes.items())},
        "added": {name: hashlib.sha256(data).hexdigest() for name, data in additions.items()},
    }
    (ROOT / "port.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"Applied Athenaeum adaptations to {len(changes)} upstream files; added {len(additions)} entry point")


if __name__ == "__main__":
    main()
