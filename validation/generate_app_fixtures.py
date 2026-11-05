#!/usr/bin/env python3
"""Regenerate the app's copy of the protocol v1 fixtures from the published contract and the engine.

`athenaeum/fixtures/wiki-protocol/v1/{requests,responses}.json` is consumed by the preserved Rust
test `crates/athenaeum-core/tests/protocol_fixtures.rs`, which requires every value to round-trip
canonically through the production types, the request-method set to equal
`WikiService::capabilities().operations`, and `capabilities_ok` to equal the live capabilities
envelope value-for-value.

So the copy is generated, never hand-edited:

  * the sixteen original request fixtures and the original response fixtures are kept exactly as
    they are (they already round-trip);
  * the additive methods published by `packages/protocol` (status, review, sources, index, path)
    are copied from its `fixtures/bridge/v1/{requests,portable-requests,responses}.json`;
  * `capabilities_ok` is captured from the real engine's own bridge output, verbatim.

Usage:
    ATHENAEUM_BIN=/path/to/athenaeum python3 generate_app_fixtures.py
    python3 generate_app_fixtures.py --check     # fail if the copy is stale, write nothing
"""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
SOURCES = HERE.parent
APP_FIXTURES = SOURCES / "fixtures" / "wiki-protocol" / "v1"
PROTOCOL_FIXTURES = SOURCES / "packages/protocol" / "fixtures" / "bridge" / "v1"
UTF8 = "utf-8"

ADDITIVE_METHODS = ("status", "review", "sources", "index", "path")
FLOAT_KEYS = {"confidence"}

ADDITIVE_RESPONSES = (
    "status_ok",
    "status_uninitialized_ok",
    "review_ok",
    "sources_ok",
    "index_ok",
    "path_ok",
    "path_not_found_ok",
)


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding=UTF8))
    if not isinstance(value, dict):
        raise SystemExit(f"{path} is not a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    ordered = {key: value[key] for key in sorted(value)}
    path.write_text(json.dumps(ordered, indent=2, ensure_ascii=False) + "\n", encoding=UTF8)


def canonicalize(value: Any, key: str | None = None) -> Any:
    """Rewrite numbers the production types hold as `f32` into the f64 value serde prints.

    `confidence` is an `f32` in the engine, and serde renders it through `Value` as the f64
    expansion of that f32 (0.82 becomes 0.8199999928474426). A fixture that keeps the readable
    0.82 is not canonical, and the preserved round-trip test rejects it — correctly. Only the
    keys this generator knows are floating point are rewritten; integers stay exactly as published.
    """
    if isinstance(value, dict):
        return {name: canonicalize(item, name) for name, item in value.items()}
    if isinstance(value, list):
        return [canonicalize(item, key) for item in value]
    if key in FLOAT_KEYS and isinstance(value, (int, float)) and not isinstance(value, bool):
        return struct.unpack("f", struct.pack("f", float(value)))[0]
    return value


def live_capabilities(binary: str) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="athenaeum-fixtures-") as directory:
        workspace = Path(directory)
        subprocess.run([binary, "-C", str(workspace), "init"], check=True, capture_output=True)
        envelope = {"protocolVersion": 1, "requestId": "capabilities-1", "request": {"method": "capabilities"}}
        process = subprocess.run(
            [binary, "-C", str(workspace), "bridge"],
            input=json.dumps(envelope),
            capture_output=True,
            text=True,
            encoding=UTF8,
            errors="strict",
            check=False,
        )
        if process.returncode != 0:
            raise SystemExit(f"the engine refused the capabilities request: {process.stdout}{process.stderr}")
        reply = json.loads(process.stdout)
        if reply.get("status") != "ok":
            raise SystemExit(f"the engine did not answer capabilities: {reply!r}")
        return reply


def generated(binary: str) -> tuple[dict[str, Any], dict[str, Any]]:
    requests = read_json(APP_FIXTURES / "requests.json")
    responses = read_json(APP_FIXTURES / "responses.json")
    published_requests = {
        **read_json(PROTOCOL_FIXTURES / "requests.json"),
        **read_json(PROTOCOL_FIXTURES / "portable-requests.json"),
    }
    published_responses = read_json(PROTOCOL_FIXTURES / "responses.json")

    # Every method comes from the published fixtures (the two-file split merged), so hand-edits
    # cannot survive a regeneration and `--check` notices them.
    missing = sorted((set(requests) | set(ADDITIVE_METHODS)) - set(published_requests))
    if missing:
        raise SystemExit(f"the protocol publishes no request fixture for: {', '.join(missing)}")
    requests = {method: published_requests[method] for method in sorted(published_requests)}
    for name in ADDITIVE_RESPONSES:
        fixture = published_responses.get(name)
        if fixture is None:
            raise SystemExit(f"the protocol has not published a response fixture for {name!r}")
        responses[name] = canonicalize(fixture)
    responses["capabilities_ok"] = live_capabilities(binary)

    methods = sorted(value["request"]["method"] for value in requests.values())
    operations = responses["capabilities_ok"]["response"]["capabilities"]["operations"]
    if methods != sorted(operations):
        raise SystemExit(
            f"the fixture methods and the engine's advertised operations disagree:\n"
            f"  fixtures:   {methods}\n  advertised: {sorted(operations)}"
        )
    return requests, responses


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default="", help="the engine to capture capabilities from (default ATHENAEUM_BIN)")
    parser.add_argument("--check", action="store_true", help="report drift instead of writing")
    options = parser.parse_args()

    import os

    binary = options.binary or os.environ.get("ATHENAEUM_BIN", "")
    if not binary:
        raise SystemExit("set ATHENAEUM_BIN (or pass --binary) to the built athenaeum executable")

    requests, responses = generated(binary)
    current_requests = read_json(APP_FIXTURES / "requests.json")
    current_responses = read_json(APP_FIXTURES / "responses.json")
    if options.check:
        drift = []
        if current_requests != requests:
            drift.append(f"requests.json: {sorted(set(requests) ^ set(current_requests)) or 'values differ'}")
        if current_responses != responses:
            drift.append(f"responses.json: {sorted(set(responses) ^ set(current_responses)) or 'values differ'}")
        for problem in drift:
            print(f"STALE {problem}")
        print("app fixtures are current" if not drift else "app fixtures are stale")
        return 1 if drift else 0

    write_json(APP_FIXTURES / "requests.json", requests)
    write_json(APP_FIXTURES / "responses.json", responses)
    print(f"wrote {len(requests)} request fixtures and {len(responses)} response fixtures to {APP_FIXTURES}")
    print(f"methods: {', '.join(sorted(value['request']['method'] for value in requests.values()))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
