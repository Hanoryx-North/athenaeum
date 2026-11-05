"""Black-box checks for the one engine's repository indexing.

Retired core sync/hosted identity tests remain in the external consolidation backup.
The owner has reserved that separate repository for later work.
"""

from __future__ import annotations

import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import unittest

from product_tests.harness import Bridge, deny_read


# --- repository indexing in the one engine -------------------------------------------------------

ENGINE_BIN = os.environ.get("ATHENAEUM_BIN") or os.environ.get("ATHENAEUM_CORE_BIN")
ENGINE_SKIP = "set ATHENAEUM_BIN to a built `athenaeum` executable (see product_tests/harness.py)"


def engine(workspace: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    process = subprocess.run(
        [ENGINE_BIN or "", "-C", str(workspace), *args],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False, timeout=120,
    )
    if check and process.returncode != 0:
        raise AssertionError(f"athenaeum {' '.join(args)} failed ({process.returncode}): {process.stdout}{process.stderr}")
    return process


def engine_json(workspace: Path, *args: str, check: bool = True) -> object:
    return json.loads(engine(workspace, *args, "--json", check=check).stdout)


def search(workspace: Path, query: str) -> list[dict]:
    return json.loads(engine(workspace, "search", query, "--json").stdout)


class RepositoryIndex(unittest.TestCase):
    """Files, symbols, imports and history are indexed incrementally and reported honestly."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-index-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        self.git("init", "-q", ".")
        self.git("config", "user.email", "validation@example.test")
        self.git("config", "user.name", "validation")
        self.write("src/state.ts", "export function validateState(value: string): string {\n  return value.trim()\n}\n")
        self.write("src/login.ts", 'import { validateState } from "./state"\n\nexport function login(value: string): string {\n  return validateState(value)\n}\n')
        self.write("wiki/page.md", "---\nid: page\ntitle: A page\nsummary: Reference material.\n---\n# A page\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "initial")
        engine(self.workspace, "init")

    def git(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["git", "-C", str(self.workspace), *args],
            capture_output=True, text=True, encoding="utf-8", errors="replace", check=True, timeout=60,
        )

    def write(self, relative: str, content: str) -> Path:
        path = self.workspace / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def index(self, *extra: str, check: bool = True) -> dict:
        return engine_json(self.workspace, "index", *extra, check=check)  # type: ignore[return-value]

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_identity_is_minted_once_and_reported_back(self) -> None:
        created = engine_json(self.workspace, "repo", "init", "--name", "indexed")
        identifier = created["manifest"]["repositoryId"]  # type: ignore[index]
        again = engine_json(self.workspace, "repo", "init", "--name", "indexed")
        self.assertEqual(again["manifest"]["repositoryId"], identifier)  # type: ignore[index]
        info = engine_json(self.workspace, "repo", "info")
        self.assertEqual(info["repositoryId"], identifier)  # type: ignore[index]
        self.assertTrue((self.workspace / "athenaeum.json").is_file())

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_the_first_index_reports_files_symbols_and_import_relationships(self) -> None:
        report = self.index()
        self.assertTrue(report["scanComplete"])
        self.assertEqual(report["warnings"], [])
        self.assertGreaterEqual(report["files"], 3)
        self.assertGreaterEqual(report["symbols"], 2)
        self.assertGreaterEqual(report["resolvedImports"], 1, "the import between the two files must become an edge")
        self.assertGreaterEqual(report["edgesCreated"], 1)
        self.assertGreaterEqual(report["commits"], 1, "local Git history is part of the index")

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_reindexing_an_unchanged_repository_creates_nothing(self) -> None:
        first = self.index()
        second = self.index()
        self.assertEqual(second["nodesCreated"], 0, "an unchanged repository must not be re-created")
        self.assertEqual(second["edgesCreated"], 0, "an unchanged repository must not gain edges")
        self.assertEqual(second["files"], first["files"])
        self.assertEqual(second["symbols"], first["symbols"])

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_a_symbol_keeps_its_identity_when_its_body_changes(self) -> None:
        self.index()
        before = [hit["node"]["id"] for hit in search(self.workspace, "validateState")]
        self.assertTrue(before, "the symbol must be searchable after indexing")
        self.write("src/state.ts", "export function validateState(value: string): string {\n  return value.trim().normalize()\n}\n")
        self.index()
        after = [hit["node"]["id"] for hit in search(self.workspace, "validateState")]
        self.assertEqual(after, before, "an edit updates the symbol; it does not mint a new one")

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_a_new_file_is_found_on_the_next_index(self) -> None:
        self.index()
        self.write("src/session.ts", "export function createSession(): string {\n  return 'session'\n}\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "session")
        report = self.index()
        self.assertGreaterEqual(report["files"], 4)
        self.assertTrue(search(self.workspace, "createSession"), "a new file must be found by the next index")

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_a_removed_file_marks_its_nodes_stale_rather_than_forgetting_them(self) -> None:
        self.index()
        (self.workspace / "src/login.ts").unlink()
        self.git("add", "-A")
        self.git("commit", "-qm", "remove")
        report = self.index()
        self.assertGreaterEqual(report["nodesMarkedStale"], 1, "history is not deleted when a file goes away")

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_a_moved_source_is_relocated_or_reported_not_lost(self) -> None:
        self.index()
        self.git("mv", "src/state.ts", "src/state-check.ts")
        self.git("commit", "-qm", "move")
        report = self.index()
        self.assertGreaterEqual(report["nodesMarkedStale"], 1, "a move must be noticed")
        relocation = engine_json(self.workspace, "relocate")
        followed = relocation["relocatedBySymbol"] + relocation["relocatedByFingerprint"] + relocation["exact"]  # type: ignore[index]
        self.assertGreater(followed, 0, f"the moved anchor must be followed, or reported: {relocation!r}")

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_a_budgeted_scan_is_reported_as_incomplete(self) -> None:
        report = self.index("--max-files", "1")
        self.assertFalse(report["scanComplete"], "a budgeted scan must not claim to be complete")
        self.assertTrue(
            any("limit" in warning.lower() for warning in report["warnings"]),
            f"the warning must name the limit that stopped the scan: {report['warnings']}",
        )

    @unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
    def test_an_unreadable_file_fails_loudly(self) -> None:
        secret = self.write("src/secret.ts", "export const secret = 1\n")
        self.addCleanup(deny_read(secret))
        process = engine(self.workspace, "index", check=False)
        self.assertNotEqual(process.returncode, 0, "an unreadable file must not be silently skipped")
        self.assertRegex((process.stdout + process.stderr).lower(), r"permission|access is denied")


@unittest.skipUnless(ENGINE_BIN, ENGINE_SKIP)
class PinnedIndexContract(unittest.TestCase):
    """The additive `index`/`sources`/`status` methods on a real repository.

    These fail loudly until the engine lands them.
    """

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-pinned-index-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        subprocess.run(["git", "-C", str(self.workspace), "init", "-q", "."], check=True)
        subprocess.run(["git", "-C", str(self.workspace), "config", "user.email", "validation@example.test"], check=True)
        subprocess.run(["git", "-C", str(self.workspace), "config", "user.name", "validation"], check=True)
        (self.workspace / "src").mkdir()
        (self.workspace / "src/state.ts").write_text("export const state = 1\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.workspace), "add", "-A"], check=True)
        subprocess.run(["git", "-C", str(self.workspace), "commit", "-qm", "initial"], check=True)
        engine(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def test_index_with_repository_scope_reports_repository_files(self) -> None:
        report = self.bridge.request("index", scope="repository")["index"]
        self.assertGreaterEqual(report["repositoryFiles"], 1)
        self.assertIsInstance(report["symbols"], int)
        self.assertIsInstance(report["skipped"], list)

    def test_sources_lists_a_repository_file_with_its_real_hash(self) -> None:
        self.bridge.request("index", scope="all")
        records = self.bridge.request("sources", limit=200)["sources"]
        entry = next((item for item in records if item["path"] == "src/state.ts"), None)
        self.assertIsNotNone(entry, f"sources must cover repository files too: {[item['path'] for item in records]}")
        assert entry is not None
        self.assertEqual(entry["sha256"], hashlib.sha256((self.workspace / "src/state.ts").read_bytes()).hexdigest())
        self.assertEqual(entry["bytes"], (self.workspace / "src/state.ts").stat().st_size)
        self.assertTrue(entry["scanned"], "a file this run read is scanned, not skipped")

    def test_one_record_per_scope_even_when_a_path_is_both(self) -> None:
        """A wiki page is a vault page and a repository file: sources keeps both attributions."""
        page = self.workspace / "wiki/state.md"
        page.parent.mkdir(exist_ok=True)
        page.write_text("---\nid: state\ntitle: State\nsummary: Validate state.\n---\n# State\n", encoding="utf-8")
        self.bridge.request("index", scope="all")
        records = [item for item in self.bridge.request("sources", limit=200)["sources"] if item["path"] == "wiki/state.md"]
        scopes = sorted(item["scope"] for item in records)
        self.assertEqual(scopes, ["repository", "vault"], f"one record per (scope, path): {records}")
        self.assertEqual(
            {item["sha256"] for item in records},
            {hashlib.sha256(page.read_bytes()).hexdigest()},
            "both attributions report the file's real hash",
        )

    def test_status_reports_a_stale_index_after_a_source_is_edited_or_deleted(self) -> None:
        """A recorded source that was edited or deleted makes the index stale.

        Known limitation, asserted nowhere: a brand-new file that no scan has seen yet is not
        noticed by `status` until the next scan runs. That is deliberate (status stats the recorded
        inventory, it does not walk the tree) and is not treated as staleness here.
        """
        self.bridge.request("index", scope="repository")
        fresh = self.bridge.request("status")["status"]
        self.assertFalse(fresh["indexStale"], "a just-built index is not stale")
        (self.workspace / "src/state.ts").write_text("export const state = 2\n", encoding="utf-8")
        edited = self.bridge.request("status")["status"]
        self.assertTrue(edited["indexStale"], "an edited source makes the index stale until it is rebuilt")
        self.bridge.request("index", scope="repository")
        (self.workspace / "src/state.ts").unlink()
        deleted = self.bridge.request("status")["status"]
        self.assertTrue(deleted["indexStale"], "a deleted source makes the index stale until it is rebuilt")

    def test_review_reports_an_incomplete_scan(self) -> None:
        engine(self.workspace, "index", "--max-files", "0", check=False)
        kinds = [finding["kind"] for finding in self.bridge.request("review", limit=100)["findings"]]
        self.assertIn("incomplete_scan", kinds, kinds)


if __name__ == "__main__":
    unittest.main()
