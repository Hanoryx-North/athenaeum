#!/usr/bin/env python3
"""Verify every imported source file against the upstream and adaptation manifests."""
import hashlib
import json
import os
from pathlib import Path

root = Path(__file__).resolve().parent
if os.name == "nt" and not str(root).startswith("\\\\?\\"):
    root = Path("\\\\?\\" + str(root))
upstream = json.loads((root / "upstream.json").read_text(encoding="utf-8"))
port = json.loads((root / "port.json").read_text(encoding="utf-8"))
if port["upstreamRevision"] != upstream["revision"]:
    raise SystemExit("Port and upstream revisions do not match")
expected = dict(upstream["files"])
expected.update({name: data["sha256"] for name, data in port["modified"].items()})
expected.update(port["added"])
failures = []
for name, digest in expected.items():
    file = root / "codex" / name
    if not file.is_file() or hashlib.sha256(file.read_bytes()).hexdigest() != digest:
        failures.append(name)
if failures:
    raise SystemExit("Missing or modified source:\n" + "\n".join(failures))
print(f"Verified {len(expected)} files at upstream {upstream['revision']}; {len(port['modified'])} recorded adaptations")
