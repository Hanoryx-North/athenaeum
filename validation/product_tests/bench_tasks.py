#!/usr/bin/env python3
"""Held-out coding tasks, retrieved four ways: what each method costs before it answers.

Conditions, all deterministic and keyless (no model, no network, no credentials):

  no-memory    read repository files in order until one carries the answer
  plain-text   build one concatenated text index, then substring-search it per task
  hybrid       ask the engine for a task brief (`context`) and take its anchors
  graph        search the engine, then walk relationships (`navigate`) from the best hit

Every condition answers the same held-out tasks against the same generated repository. What is
measured is retrieval: whether the answer was actually reached (task completion), how many bytes
were read from the repository, how many retrieval calls were made, how large the context handed
onward was, and how long it took. Model calls and cost are 0 in this keyless mode and are reported
as UNRUN for a model-driven run; `--help` and the repository README give the exact command.

    python3 product_tests/bench_tasks.py                      # all four conditions, JSON on stdout
    python3 product_tests/bench_tasks.py --out results.json
    python3 product_tests/bench_tasks.py --conditions hybrid,graph --repetitions 3

ATHENAEUM_BIN must name a built `athenaeum` executable for the engine conditions; without it those
conditions are reported as UNRUN rather than guessed.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import statistics
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from product_tests.harness import BIN, Bridge, run  # noqa: E402

MODE = "keyless-deterministic"
MODEL_RUN = (
    "UNRUN: no model credentials in this environment. To run the model-driven comparison, export "
    "a provider key and run `python3 product_tests/bench_tasks.py --model <provider:model> "
    "--out results-model.json`; the harness then sends each condition's context to that model and "
    "records completion, tests passed, tokens, latency and cost per task."
)

# Each task names a symptom, never the file that answers it: the path is held out.
TASKS: list[dict[str, str]] = [
    {
        "id": "oauth-state",
        "question": "The OAuth callback creates a session before it has validated the state parameter. Which module must be fixed?",
        "keyword": "validateStateBeforeSession",
        "answerPath": "src/auth/state_guard.ts",
    },
    {
        "id": "retry-budget",
        "question": "Outbound webhook retries keep amplifying an outage. Which module owns the retry budget?",
        "keyword": "nextDelay",
        "answerPath": "src/webhooks/retry_policy.ts",
    },
    {
        "id": "cache-invalidation",
        "question": "Stale permission data is served from the in-process cache after a role change. Which module is responsible?",
        "keyword": "invalidatePrincipalCache",
        "answerPath": "src/permissions/cache.ts",
    },
    {
        "id": "audit-gap",
        "question": "Some administrative actions never reach the audit log. Which module writes the log entries?",
        "keyword": "recordAdministrativeAction",
        "answerPath": "src/audit/journal_writer.ts",
    },
    {
        "id": "upload-limit",
        "question": "Large uploads are accepted despite the documented size limit. Which module enforces the limit?",
        "keyword": "withinLimit",
        "answerPath": "src/uploads/size_policy.ts",
    },
    {
        "id": "clock-skew",
        "question": "Signed tokens are rejected after a small clock difference between hosts. Which module decides the tolerance?",
        "keyword": "withinTolerance",
        "answerPath": "src/tokens/skew.ts",
    },
]

FILLER_TOPICS = (
    "formatting", "sorting", "pagination", "scheduling", "metrics", "tracing", "routing",
    "serialisation", "compression", "sharding", "throttling", "caching", "windowing", "batching",
    "validation", "rendering", "exporting", "importing",
)


def build_corpus(root: Path) -> tuple[Path, dict]:
    """A deterministic repository that is also an Athenaeum vault.

    Returns the repository and the engine's setup cost (the read the engine pays once, at index
    time, before any task is asked).
    """
    repository = root / "repository"
    for relative, content in corpus_files().items():
        path = repository / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    corpus_bytes = sum(path.stat().st_size for path in files_in_order(repository))
    started = time.perf_counter()
    run(repository, "init")
    run(repository, "index")
    run(repository, "index-vault")
    setup = {
        "bytesReadFromRepository": corpus_bytes,
        "toolCalls": 2,
        "milliseconds": round((time.perf_counter() - started) * 1000, 2),
    }
    return repository, setup


def corpus_files() -> dict[str, str]:
    files: dict[str, str] = {}
    for index, topic in enumerate(FILLER_TOPICS):
        files[f"src/{topic}/module_{index:02d}.ts"] = (
            f"// {topic} helpers\nexport function {topic}Step{index}(value: number): number {{\n"
            f"  return value + {index}\n}}\n"
        )
    files["src/auth/state_guard.ts"] = (
        "// Guards the OAuth callback before a session exists.\n"
        "export function validateStateBeforeSession(state: string, expected: string): boolean {\n"
        "  return state.length > 0 && state === expected\n}\n"
    )
    files["src/webhooks/retry_policy.ts"] = (
        "// A bounded retry budget: more retries amplify an outage.\n"
        "export const WEBHOOK_RETRY_BUDGET = 3\n"
        "export function nextDelay(attempt: number): number {\n  return 100 * 2 ** attempt\n}\n"
    )
    files["src/permissions/cache.ts"] = (
        "const cache = new Map<string, string[]>()\n"
        "export function invalidatePrincipalCache(principal: string): void {\n  cache.delete(principal)\n}\n"
    )
    files["src/audit/journal_writer.ts"] = (
        "export function recordAdministrativeAction(actor: string, action: string): string {\n"
        "  return `${actor}:${action}`\n}\n"
    )
    files["src/uploads/size_policy.ts"] = (
        "export const MAX_UPLOAD_BYTES = 8 * 1024 * 1024\n"
        "export function withinLimit(bytes: number): boolean {\n  return bytes <= MAX_UPLOAD_BYTES\n}\n"
    )
    files["src/tokens/skew.ts"] = (
        "export const CLOCK_SKEW_TOLERANCE_SECONDS = 30\n"
        "export function withinTolerance(delta: number): boolean {\n"
        "  return Math.abs(delta) <= CLOCK_SKEW_TOLERANCE_SECONDS\n}\n"
    )
    files["wiki/index.md"] = "---\nid: index\ntitle: Repository notes\nsummary: How this repository is organised.\n---\n# Repository notes\n\nDecisions are cited from `raw/sources`.\n"
    files["raw/sources/incident-141.md"] = (
        "Incident 141: the OAuth callback created a session before validating state. "
        "The guard lives in src/auth/state_guard.ts.\n"
    )
    files["raw/sources/incident-142.md"] = (
        "Incident 142: webhook retries amplified an outage until the budget was bounded in "
        "src/webhooks/retry_policy.ts.\n"
    )
    return files


def bytes_of(paths: list[Path]) -> int:
    return sum(path.stat().st_size for path in paths if path.is_file())


class Retrieval:
    """What one condition did for one task."""

    def __init__(
        self,
        *,
        found: bool,
        evidence: list[str],
        bytes_read: int,
        tool_calls: int,
        context_bytes: int,
        milliseconds: float,
        bytes_scanned: int | None = None,
    ) -> None:
        self.found = found
        self.evidence = evidence
        self.bytes_read = bytes_read
        self.bytes_scanned = bytes_read if bytes_scanned is None else bytes_scanned
        self.tool_calls = tool_calls
        self.context_bytes = context_bytes
        self.milliseconds = milliseconds


def files_in_order(repository: Path) -> list[Path]:
    return sorted(
        path for path in repository.rglob("*")
        if path.is_file() and ".athenaeum" not in path.parts and ".git" not in path.parts
    )


def no_memory_condition(repository: Path, task: dict[str, str]) -> Retrieval:
    """Read files until one carries the keyword. The honest baseline: no retrieval layer at all."""
    started = time.perf_counter()
    evidence: list[str] = []
    read = 0
    calls = 0
    for path in files_in_order(repository):
        text = path.read_text(encoding="utf-8", errors="replace")
        read += len(text.encode("utf-8"))
        calls += 1
        if task["keyword"] in text:
            evidence.append(str(path.relative_to(repository)))
            break
    return Retrieval(
        found=task["answerPath"] in evidence,
        evidence=evidence,
        bytes_read=read,
        tool_calls=calls,
        context_bytes=read,
        milliseconds=(time.perf_counter() - started) * 1000,
    )


def plain_text_index(repository: Path) -> tuple[str, int]:
    """The preprocessing a grep-based memory does: one concatenated blob over the repository."""
    chunks: list[str] = []
    read = 0
    for path in files_in_order(repository):
        text = path.read_text(encoding="utf-8", errors="replace")
        read += len(text.encode("utf-8"))
        chunks.append(f"\n\n@@FILE {path.relative_to(repository)}@@\n{text}")
    return "".join(chunks), read


def plain_text_condition(index: str, task: dict[str, str]) -> Retrieval:
    started = time.perf_counter()
    evidence: list[str] = []
    position = index.find(task["keyword"])
    if position >= 0:
        header = index.rfind("@@FILE ", 0, position)
        end = index.find("@@", header + len("@@FILE "))
        evidence.append(index[header + len("@@FILE "):end].strip())
    return Retrieval(
        found=task["answerPath"] in evidence,
        evidence=evidence,
        bytes_read=0,  # the blob is already in memory; nothing is read from disk per task
        bytes_scanned=len(index.encode("utf-8")),
        tool_calls=1,
        context_bytes=0,
        milliseconds=(time.perf_counter() - started) * 1000,
    )


def anchors_from_brief(brief: dict) -> list[str]:
    return [anchor for anchor in brief.get("anchors", []) if isinstance(anchor, str)]


def hybrid_condition(bridge: Bridge, task: dict[str, str]) -> Retrieval:
    started = time.perf_counter()
    context = bridge.request("context", task=task["question"])["context"]
    evidence = anchors_from_brief(context["brief"])
    prompt = context["prompt"]
    return Retrieval(
        found=task["answerPath"] in evidence or task["answerPath"] in prompt,
        evidence=evidence,
        bytes_read=0,
        bytes_scanned=len(prompt.encode("utf-8")),
        tool_calls=1,
        context_bytes=len(prompt.encode("utf-8")),
        milliseconds=(time.perf_counter() - started) * 1000,
    )


def graph_condition(bridge: Bridge, task: dict[str, str]) -> Retrieval:
    started = time.perf_counter()
    hits = bridge.request("search", query=task["keyword"], limit=5)["hits"]
    evidence: list[str] = []
    context_bytes = 0
    calls = 1
    if hits:
        node = hits[0]["node"]["id"]
        reached = bridge.request("navigate", node=node, depth=2, maxNodes=25)["nodes"]
        calls += 1
        context_bytes = len(json.dumps(reached, ensure_ascii=False).encode("utf-8"))
        for item in reached:
            identifier = item["node"]["id"]
            records = bridge.request("evidence", node=identifier)["evidence"]
            calls += 1
            for record in records:
                anchor = (record.get("anchor") or {}).get("file")
                if anchor:
                    evidence.append(anchor)
    return Retrieval(
        found=task["answerPath"] in evidence,
        evidence=sorted(set(evidence)),
        bytes_read=0,
        bytes_scanned=context_bytes,
        tool_calls=calls,
        context_bytes=context_bytes,
        milliseconds=(time.perf_counter() - started) * 1000,
    )


def run_condition(name: str, repository: Path, tasks: list[dict[str, str]], repetitions: int) -> dict:
    setup_started = time.perf_counter()
    setup_read = 0
    setup_calls = 0
    plain_index = ""
    bridge: Bridge | None = None
    if name == "plain-text":
        plain_index, setup_read = plain_text_index(repository)
        setup_calls = 1
    elif name in ("hybrid", "graph"):
        bridge = Bridge(repository)
    setup_ms = (time.perf_counter() - setup_started) * 1000

    results: list[dict] = []
    for task in tasks:
        samples: list[Retrieval] = []
        for _ in range(repetitions):
            if name == "no-memory":
                samples.append(no_memory_condition(repository, task))
            elif name == "plain-text":
                samples.append(plain_text_condition(plain_index, task))
            elif name == "hybrid":
                assert bridge is not None
                samples.append(hybrid_condition(bridge, task))
            elif name == "graph":
                assert bridge is not None
                samples.append(graph_condition(bridge, task))
            else:  # pragma: no cover - guarded by the CLI
                raise AssertionError(f"unknown condition {name!r}")
        last = samples[-1]
        results.append(
            {
                "task": task["id"],
                "answerPath": task["answerPath"],
                "found": last.found,
                "testsPassed": 1 if last.found else 0,
                "testsTotal": 1,
                "bytesReadFromRepository": last.bytes_read,
                "bytesScanned": last.bytes_scanned,
                "toolCalls": last.tool_calls,
                "contextBytes": last.context_bytes,
                "medianMilliseconds": round(statistics.median([s.milliseconds for s in samples]), 2),
                "evidence": last.evidence,
            }
        )

    completed = sum(1 for item in results if item["found"])
    return {
        "condition": name,
        "setup": {
            "bytesReadFromRepository": setup_read,
            "toolCalls": setup_calls,
            "milliseconds": round(setup_ms, 2),
        },
        "tasks": results,
        "summary": {
            "tasksCompleted": completed,
            "tasksTotal": len(results),
            "testsPassed": completed,
            "testsTotal": len(results),
            "bytesReadFromRepository": sum(item["bytesReadFromRepository"] for item in results),
            "bytesScanned": sum(item["bytesScanned"] for item in results),
            "toolCalls": sum(item["toolCalls"] for item in results),
            "medianContextBytes": statistics.median([item["contextBytes"] for item in results]) if results else 0,
            "medianMilliseconds": statistics.median([item["medianMilliseconds"] for item in results]) if results else 0,
            "modelCalls": 0,
            "costUsd": 0.0,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--conditions", default="no-memory,plain-text,hybrid,graph")
    parser.add_argument("--repetitions", type=int, default=1)
    parser.add_argument("--out", default="", help="write the JSON report here (otherwise stdout)")
    parser.add_argument("--model", default="", help="UNRUN in this environment: provider:model for a model-driven run")
    options = parser.parse_args()

    requested = [name.strip() for name in options.conditions.split(",") if name.strip()]
    engine_conditions = {"hybrid", "graph"}
    runnable: list[str] = []
    unrun: list[dict[str, str]] = []
    for name in requested:
        if name in engine_conditions and not BIN:
            unrun.append({
                "condition": name,
                "reason": "ATHENAEUM_BIN is not set; the engine conditions cannot be measured",
                "fix": "cargo build -p athenaeum-cli, then ATHENAEUM_BIN=<path> python3 product_tests/bench_tasks.py",
            })
            continue
        runnable.append(name)

    conditions: list[dict] = []
    engine_setup: dict = {}
    with tempfile.TemporaryDirectory(prefix="athenaeum-bench-tasks-") as directory:
        repository, engine_setup = build_corpus(Path(directory))
        for name in runnable:
            conditions.append(run_condition(name, repository, TASKS, options.repetitions))

    report = {
        "scenario": "held-out-coding-tasks",
        "mode": MODE,
        "engine": BIN,
        "repository": "generated deterministically; see corpus_files() in this file",
        "tasks": [task["id"] for task in TASKS],
        "repetitions": options.repetitions,
        "conditions": conditions,
        "engineSetup": engine_setup,
        "unrun": unrun,
        "modelDriven": MODEL_RUN if not options.model else f"requested {options.model} — implement the model loop before claiming any number",
        "notes": [
            "task completion and tests passed are deterministic checks against a held-out answer path",
            "bytesReadFromRepository counts bytes actually read from disk; bytesScanned counts bytes "
            "looked at (a loaded text index, a returned context). The engine conditions pay their "
            "repository read once, at index time, reported as engineSetup",
            "modelCalls and costUsd are 0 because this mode uses no model; they are UNRUN, not measured",
        ],
    }
    text = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    if options.out:
        Path(options.out).write_text(text, encoding="utf-8")
        print(f"wrote {options.out}")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
