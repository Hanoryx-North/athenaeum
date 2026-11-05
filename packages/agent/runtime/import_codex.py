#!/usr/bin/env python3
"""Import a pinned, tracked Codex runtime snapshot, never checkout state or secrets."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile


ROOT = Path(__file__).resolve().parent
if os.name == "nt" and not str(ROOT).startswith("\\\\?\\"):
    ROOT = Path("\\\\?\\" + str(ROOT))
PATHS = ("codex-rs", "third_party", "patches", ".cargo", "LICENSE", "NOTICE")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--revision", required=True, help="Exact upstream Git commit")
    parser.add_argument("--resume", action="store_true", help="Resume a partial, byte-identical import")
    args = parser.parse_args()
    revision = subprocess.check_output(
        ["git", "-C", str(args.source), "rev-parse", f"{args.revision}^{{commit}}"],
        text=True,
    ).strip()
    destination = ROOT / "codex"
    if destination.exists() and not args.resume:
        parser.error(f"{destination} already exists; refusing to overwrite the port")
    archive = subprocess.check_output(
        ["git", "-C", str(args.source), "archive", revision, *PATHS]
    )
    manifest = {}
    symlinks = {}
    with tarfile.open(fileobj=io.BytesIO(archive)) as snapshot:
        if destination.exists():
            expected = {entry.name: entry for entry in snapshot.getmembers() if entry.isfile() or entry.issym()}
            for existing in destination.rglob("*"):
                if existing.is_dir():
                    continue
                relative = existing.relative_to(destination).as_posix()
                if relative not in expected or existing.read_bytes() != snapshot.extractfile(expected[relative]).read():
                    parser.error(f"Refusing to overwrite non-upstream file: {relative}")
        for entry in snapshot.getmembers():
            if not entry.isfile() and not entry.issym():
                if entry.isdir():
                    continue
                raise ValueError(f"Unexpected non-file in source snapshot: {entry.name}")
            relative = Path(entry.name)
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"Unsafe archive path: {entry.name}")
            data = snapshot.extractfile(entry).read()
            if entry.issym():
                symlinks[entry.name] = entry.linkname
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            manifest[entry.name] = hashlib.sha256(data).hexdigest()
    provenance = {
        "repository": "https://github.com/openai/codex",
        "revision": revision,
        "license": "Apache-2.0",
        "paths": list(PATHS),
        "files": manifest,
        "materializedSymlinks": symlinks,
    }
    (ROOT / "upstream.json").write_text(
        json.dumps(provenance, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"Imported {len(manifest)} source files from Codex {revision}")


if __name__ == "__main__":
    main()
