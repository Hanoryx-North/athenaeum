#!/usr/bin/env python3
"""Validate the current checkout directly; no sibling repos or historical Git blobs."""
from __future__ import annotations

import argparse
from functools import cache
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
ENV = {**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8",
       "ATHENAEUM_REPO": str(ROOT), "ATHENAEUM_AGENT_ROOT": str(ROOT / "packages/agent")}
TARGET = Path(ENV.get("CARGO_TARGET_DIR", ROOT / "target"))
if not TARGET.is_absolute():
    TARGET = ROOT / TARGET
ENV["CARGO_TARGET_DIR"] = str(TARGET)
ENV.setdefault("ATHENAEUM_BIN", str(TARGET / "debug" / ("athenaeum.exe" if os.name == "nt" else "athenaeum")))
NPM = shutil.which("npm") or "npm"


class MissingPrerequisite(Exception):
    pass


def run(command: list[str], cwd: Path = ROOT) -> None:
    print(f"$ {cwd.relative_to(ROOT) or '.'}: {' '.join(command)}", flush=True)
    # Stream and retain output so a framework's successful exit cannot hide skipped tests.
    child = subprocess.Popen(command, cwd=cwd, env=ENV, text=True, encoding="utf-8",
                             errors="replace", stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = []
    for line in child.stdout:
        print(line, end="", flush=True)
        output.append(line)
    if child.wait():
        raise subprocess.CalledProcessError(child.returncode, command)
    text = "".join(output)
    skips = re.findall(r"\.\.\. skipped ['\"]([^\n]+)|# skipped ([1-9]\d*)|\b([1-9]\d*) skipped", text)
    if skips:
        raise MissingPrerequisite(f"test framework reported skipped tests: {skips}")


def npm(workspace: str | None, *args: str) -> None:
    run([NPM, *args, *(["--workspace", workspace] if workspace else [])])


def engine() -> None:
    run(["cargo", "test", "-p", "athenaeum-core", "-p", "athenaeum-cli", "--all-targets"])
    run(["cargo", "test", "-p", "athenaeum-core", "-p", "athenaeum-cli", "--doc"])
    run(["cargo", "build", "-p", "athenaeum-cli"])


@cache
def require_engine() -> None:
    if not os.environ.get("ATHENAEUM_BIN") or not Path(ENV["ATHENAEUM_BIN"]).is_file():
        run(["cargo", "build", "-p", "athenaeum-cli"])


def python_tests(*modules: str) -> None:
    require_engine()
    run([sys.executable, "-m", "unittest", "-v", *modules], HERE)


def protocol() -> None:
    npm("@athenaeum/protocol", "run", "build")
    npm("@athenaeum/protocol", "run", "check")
    npm("@athenaeum/protocol", "test")
    run(["cargo", "test", "-p", "athenaeum-protocol"])


def integrations() -> None:
    npm("athenaeum-integrations", "run", "verify")


def agent() -> None:
    require_engine()
    npm("athenaeum-agent", "run", "typecheck")
    npm("athenaeum-agent", "run", "build")
    npm("athenaeum-agent", "test")


def mcp() -> None:
    integrations()
    python_tests("product_tests.test_mcp_desktop")


def agent_knowledge() -> None:
    npm("athenaeum-agent", "run", "build")
    python_tests("product_tests.test_agent_to_knowledge")


def product_protocol() -> None:
    python_tests("product_tests.test_protocol_contract")
    run([sys.executable, "generate_app_fixtures.py", "--check"], HERE)
    run(["node", "--import", "tsx", "--test", "validation/product_tests/protocol_contract.test.ts",
         "validation/product_tests/desktop_contract.test.ts"])


def frontend() -> None:
    npm(None, "run", "build")


def desktop() -> None:
    if not (shutil.which("protoc") or ENV.get("PROTOC")):
        raise MissingPrerequisite("desktop compilation needs protoc or PROTOC")
    run(["cargo", "check", "-p", "athenaeum-app", "--all-targets"])


def harness_runtime() -> None:
    harness = ENV.get("DSH_REPO")
    if not harness or not (Path(harness) / "apps/cli/lib/bin.js").is_file():
        raise MissingPrerequisite("set DSH_REPO to a built upstream checkout for the optional Harness runtime test")
    require_engine()
    npm("athenaeum-agent", "exec", "--", "vitest", "run", "--config", "vitest.runtime.config.ts")
    run(["node", "validation/product_tests/desktop_harness.test.mjs"])


def native_smoke() -> None:
    binary = TARGET / "native-runtime/debug" / ("athenaeum-agent.exe" if os.name == "nt" else "athenaeum-agent")
    if not binary.is_file():
        raise MissingPrerequisite("build the source runtime with npm run native:build -w athenaeum-agent")
    ENV["ATHENAEUM_NATIVE_AGENT_BIN"] = str(binary)
    python_tests("product_tests.test_native_agent", "product_tests.test_desktop_agent")


# Optional runtime/platform checks are explicit. They are never reported as verified by default.
STEPS = {
    "native-agent-config": ("Owned runtime configuration, permissions and session behavior", lambda: (run([sys.executable, "packages/agent/runtime/verify_source.py"]), run(["node", "--test", "packages/agent/runtime/src/runtime.test.mjs", "packages/agent/runtime/src/provider.test.mjs"]))),
    "native-agent-runtime": ("Source-built native agent compilation (optional)", lambda: run(["node", "packages/agent/runtime/manage.mjs", "build"])),
    "native-agent-smoke": ("Native service and desktop tool/approval smoke tests (optional)", lambda: native_smoke()),
    "product-automation": ("Per-wiki saving/sync policy and durable page links", lambda: python_tests("product_tests.test_wiki_automation")),
    "product-desktop-control": ("Live desktop request transport and catalog helpers", lambda: run(["node", "--import", "tsx", "--test", "validation/product_tests/desktop_control.test.mjs"])),
    "protocol": ("Shared TypeScript and Rust wire contracts and fixtures", protocol),
    "engine": ("Engine and CLI unit, integration and doc tests; CLI build", engine),
    "integrations": ("MCP/connectors typecheck, build and standalone bundle", integrations),
    "agent": ("Agent adapter types, build, seam and real-engine tests", agent),
    "app-frontend": ("Desktop frontend typecheck and production bundle", frontend),
    "frontend-tests": ("Desktop frontend unit and scenario tests", lambda: npm(None, "test")),
    "product-cli": ("Command discovery, settings, development plans and lifecycle", lambda: python_tests("product_tests.test_wiki_cli")),
    "product-engine": ("Knowledge lifecycle, vault rebuild and legacy data import", lambda: python_tests("product_tests.test_local_memory_loop", "product_tests.test_engine_migration")),
    "product-repository": ("Repository identities and incremental indexing", lambda: python_tests("product_tests.test_repository_ids")),
    "product-knowledge": ("Evidence, freshness, duplicates and proposal review", lambda: python_tests("product_tests.test_knowledge_review")),
    "product-boundaries": ("Corruption, permissions, hostile retrieval and portability", lambda: python_tests("product_tests.test_failure_boundaries")),
    "product-protocol": ("Engine/desktop contract and generated fixture drift", product_protocol),
    "product-mcp-desktop": ("Real MCP server to engine and desktop request surface", mcp),
    "product-agent-to-knowledge": ("Agent proposes; review saves; another session recalls", agent_knowledge),
    "product-agent-memory": ("Installed setup, instruction repair, disconnect and memory", lambda: python_tests("product_tests.test_agent_setup")),
    "app-desktop": ("Native desktop compilation (optional)", desktop),
    "harness-runtime": ("Optional Harness profile and desktop driver against upstream", harness_runtime),
    "native-packaging": ("Staged native installer resources (optional)", lambda: python_tests("product_tests.test_native_installers")),
    "bench": ("Keyless memory benchmark (optional)", lambda: (require_engine(), run([sys.executable, "product_tests/bench_memory.py"], HERE))),
}
OPTIONAL = {"native-agent-runtime", "native-agent-smoke", "app-desktop", "harness-runtime", "native-packaging", "bench"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--only", default="")
    parser.add_argument("--skip", default="")
    parser.add_argument("--with", dest="extra", default="")
    parser.add_argument("--fail-on-skip", action="store_true")
    parser.add_argument("--results", type=Path)
    args = parser.parse_args()
    if args.list:
        for name, (description, _) in STEPS.items():
            print(f"{name:28} {description}")
        return 0
    split = lambda value: set(filter(None, value.split(",")))
    requested = split(args.only) | split(args.extra) | split(args.skip)
    if requested - STEPS.keys():
        parser.error(f"unknown steps: {', '.join(sorted(requested - STEPS.keys()))}")
    selected = (split(args.only) or (STEPS.keys() - OPTIONAL)) | split(args.extra)
    selected -= split(args.skip)
    results = []
    for name, (_, action) in STEPS.items():
        if name not in selected:
            continue
        start = time.monotonic()
        print(f"\n=== {name} ===", flush=True)
        status, detail = "passed", ""
        try:
            action()
        except (FileNotFoundError, MissingPrerequisite) as error:
            status, detail = "skipped", str(error)
        except (subprocess.CalledProcessError, OSError) as error:
            status, detail = "failed", str(error)
        results.append({"step": name, "status": status, "seconds": round(time.monotonic() - start, 2), "detail": detail})
    print("\n" + "\n".join(f"{item['status']:8} {item['step']}: {item['detail']}" for item in results))
    if args.results:
        args.results.write_text(json.dumps({"schema": 1, "results": results}, indent=2) + "\n", encoding="utf-8")
    return int(any(item["status"] == "failed" or (args.fail_on_skip and item["status"] == "skipped") for item in results))


if __name__ == "__main__":
    sys.exit(main())
