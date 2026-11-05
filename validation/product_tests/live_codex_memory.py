"""Opt-in real Codex MCP capture/review/recall, using synthetic data and existing CLI sign-in.

Run with ATHENAEUM_BIN set. No credentials are copied or inspected, no user config is edited,
and each Codex invocation is a fresh ephemeral session. Model use is deliberate, not a default test.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from product_tests.harness import BIN, Bridge, run


def main() -> None:
    if not BIN:
        raise RuntimeError("Set ATHENAEUM_BIN to the real CLI")
    root = Path(__file__).resolve().parents[1]
    codex = shutil.which("codex")
    node = shutil.which("node")
    bundle = root.parent / "packages/integrations/dist/athenaeum-integrations.mjs"
    if not codex or not node or not bundle.is_file():
        raise RuntimeError("Install Codex and build the Athenaeum MCP bundle first")
    evidence = root / ".release-live-codex"
    evidence.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="athenaeum live codex ") as temporary:
        project = Path(temporary)
        source = project / "raw/sources/retries.md"
        source.parent.mkdir(parents=True)
        source.write_text("# Retry contract\n\nThis project allows at most 17 retries per operation.\n"
                          "The shared upstream quota counts retries as new operations, so unbounded retries exhaust the quota during outages.\n", encoding="utf-8")
        run(project, "init")
        run(project, "index-vault")
        config = {
            "command": node,
            "args": [str(bundle), "mcp", "--athenaeum", BIN, "--repo", f"release={project}", "--allow", "propose"],
            "startup_timeout_sec": 30,
            "tool_timeout_sec": 60,
        }
        # JSON's basic strings/arrays are valid TOML values; fields are a closed, trusted set.
        toml = "{ " + ", ".join(f"{k} = {json.dumps(v)}" for k, v in config.items()) + " }"
        base = [codex, "exec", "--ignore-user-config", "--json", "--ephemeral", "--skip-git-repo-check",
                "--sandbox", "workspace-write", "-C", str(project), "-c", 'approval_policy="never"',
                "-c", f"mcp_servers.athenaeum={toml}"]

        def session(name: str, prompt: str) -> str:
            # stdin preserves multiline prompts through Windows' codex.cmd wrapper.
            result = subprocess.run([*base, "-"], input=prompt, cwd=project, capture_output=True, encoding="utf-8", timeout=300)
            (evidence / f"{name}.jsonl").write_text(result.stdout, encoding="utf-8")
            (evidence / f"{name}.stderr.log").write_text(result.stderr, encoding="utf-8")
            if result.returncode:
                raise RuntimeError(f"Codex {name} failed ({result.returncode}); inspect {evidence}")
            return result.stdout

        first = session("capture", "This is an isolated Athenaeum release test. The source raw/sources/retries.md "
                        "has been read by the test driver and its exact contents are provided below. "
                        "Use only Athenaeum MCP tools; do not run shell tools. "
                        "then use the athenaeum MCP wiki tools to propose one durable gotcha summarizing the retry limit "
                        "and its reason. Attach source_file evidence anchored to that file, with its exact excerpt. "
                        "Use repository release where required. Do not accept proposals or write wiki Markdown. "
                        "Finish once the evidence-backed proposal is queued.\n\nSource contents:\n" + source.read_text(encoding="utf-8"))
        engine = Bridge(project)
        proposals = engine.request("proposals")["proposals"]
        if not proposals:
            raise AssertionError("Real Codex produced no proposal")
        node = proposals[0]["node"]
        identifier = node["id"] if isinstance(node, dict) else node
        opened = engine.request("open", node=identifier)
        if not opened["detail"]["evidence"]:
            raise AssertionError("Codex proposed knowledge without evidence")
        if list((project / "wiki").glob("**/*.md")):
            raise AssertionError("An agent proposal became Markdown before review")
        engine.request("accept", node=identifier, caller={"model": "release-human-reviewer", "session": "explicit-review"})
        pages = list((project / "wiki").glob("**/*.md"))
        if not pages:
            raise AssertionError("Acceptance did not persist Markdown")
        second = session("recall", "This is a new independent session for the same project. Before implementing "
                         "network retry behavior, use the Athenaeum MCP wiki tools to retrieve the project's "
                         "existing retry-limit knowledge and its source evidence. Use repository release where required. "
                         "Explain the limit and why it exists. Do not read project files or modify anything.")
        answers = [json.loads(line).get("item", {}) for line in second.splitlines() if line.startswith("{")]
        final = " ".join(item.get("text", "") for item in answers if item.get("type") == "agent_message")
        if "17" not in final or "quota" not in final.lower():
            raise AssertionError("The later real-agent session did not recall the accepted fact and rationale")
        if "wiki_" not in first or "wiki_" not in second:
            raise AssertionError("Both sessions must actually invoke MCP wiki tools")
        result = {"status": "passed", "agent": "real authenticated Codex CLI", "sessions": 2,
                  "proposal": identifier, "evidence": True, "acceptedMarkdownPages": len(pages),
                  "laterRecall": True, "automation": "explicit session instructions; no Codex lifecycle hooks"}
        (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
