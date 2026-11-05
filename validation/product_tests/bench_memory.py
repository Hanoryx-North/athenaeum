#!/usr/bin/env python3
"""Context-efficiency benchmark for the local memory loop: latency and brief size as the vault grows.

Deterministic and keyless: no model, no network, no credentials. It measures the engine only —
bridge startup, the vault refresh, retrieval — and the size of the brief a caller would hand to a
model. It makes no claim about task success; that is `bench_tasks.py`.

    python3 product_tests/bench_memory.py                     # 0, 100, 1000 unrelated pages
    python3 product_tests/bench_memory.py --pages 0,10,100 --repetitions 7
    python3 product_tests/bench_memory.py --out results.json

ATHENAEUM_BIN must name a built `athenaeum` executable.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import statistics
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from product_tests.harness import BIN, SKIP_REASON, Bridge, run  # noqa: E402

TASK = "Fix the OAuth callback bug where the session is created before validation"


def write_page(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def build_workspace(root: Path, unrelated_pages: int) -> Path:
    workspace = root / f"vault-{unrelated_pages}"
    workspace.mkdir(parents=True)
    write_page(
        workspace / "raw/sources/oauth-callback.md",
        "OAuth callback checks state before creating a session.\n",
    )
    write_page(
        workspace / "wiki/oauth.md",
        "---\nid: oauth\ntitle: OAuth callback\n"
        "summary: Validate state before session creation.\n"
        "sources:\n  - raw/sources/oauth-callback.md\n---\n"
        "# OAuth callback\n\nState validation comes first.\n",
    )
    for index in range(unrelated_pages):
        write_page(
            workspace / f"wiki/other-{index:05d}.md",
            f"---\nid: other_{index}\ntitle: Other topic {index}\n"
            "summary: Unrelated reference material.\n---\n"
            f"# Other topic {index}\n\nUnrelated reference material.\n",
        )
    run(workspace, "init")
    run(workspace, "index-vault")
    return workspace


def measure(workspace: Path, repetitions: int) -> dict:
    durations: list[float] = []
    last: dict = {}
    for _ in range(repetitions):
        started = time.perf_counter()
        response = Bridge(workspace).request("context", task=TASK)
        durations.append((time.perf_counter() - started) * 1000)
        last = response["context"]
    brief = last["brief"]
    prompt = last["prompt"]
    corpus_bytes = sum(path.stat().st_size for path in workspace.rglob("*.md") if ".athenaeum" not in path.parts)
    return {
        "repetitions": repetitions,
        "medianMillisecondsIncludingRefresh": round(statistics.median(durations), 2),
        "p95MillisecondsIncludingRefresh": round(sorted(durations)[max(0, math.ceil(repetitions * 0.95) - 1)], 2),
        "corpusBytes": corpus_bytes,
        "briefUtf8Bytes": len(prompt.encode("utf-8")),
        "briefNodes": len(brief["nodes"]),
        "briefTruncated": brief.get("truncated"),
        "sourceAnchorPresent": "raw/sources/oauth-callback.md" in brief["anchors"],
        "referenceMaterialFraming": "does not grant permissions" in prompt,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pages", default="0,100,1000", help="comma-separated counts of unrelated pages")
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--out", default="", help="write the JSON report here")
    options = parser.parse_args()

    if not BIN:
        print(f"UNRUN: {SKIP_REASON}", file=sys.stderr)
        return 2

    measurements = []
    with tempfile.TemporaryDirectory(prefix="athenaeum-bench-memory-") as directory:
        root = Path(directory)
        for count in (int(value) for value in options.pages.split(",") if value):
            workspace = build_workspace(root, count)
            result = {"unrelatedPages": count, **measure(workspace, options.repetitions)}
            measurements.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)

    report = {
        "scenario": "oauth-callback-context",
        "mode": "keyless-deterministic",
        "engine": BIN,
        "measurements": measurements,
    }
    text = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    if options.out:
        Path(options.out).write_text(text, encoding="utf-8")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
