"""Black-box tests for the single local engine: it opens existing vaults unchanged, rebuilds from
Markdown without losing accepted knowledge, drafts proposals from sources, and carries the
proposals of the retired `athenaeum-core` index forward without touching that index.

Set ATHENAEUM_BIN to a built `athenaeum` executable (see harness.py).
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from product_tests.harness import BIN, SKIP_REASON, Bridge, run, run_json

LEGACY_SCHEMA = """
CREATE TABLE nodes(id TEXT PRIMARY KEY, title TEXT NOT NULL, title_fold TEXT NOT NULL, path TEXT NOT NULL UNIQUE, body TEXT NOT NULL);
CREATE TABLE edges(id TEXT PRIMARY KEY, from_id TEXT NOT NULL, to_id TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE evidence(id TEXT PRIMARY KEY, node_id TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE proposals(id TEXT PRIMARY KEY, state TEXT NOT NULL, body TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE proposal_edges(id TEXT PRIMARY KEY, body TEXT NOT NULL);
CREATE TABLE proposal_evidence(id TEXT PRIMARY KEY, body TEXT NOT NULL);
"""


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree(root: Path) -> dict[str, str]:
    """Every file outside the derived index, with its hash."""
    return {
        str(path.relative_to(root)): sha256(path)
        for path in sorted(root.rglob("*"))
        if path.is_file() and ".athenaeum" not in path.relative_to(root).parts
    }


@unittest.skipUnless(BIN, SKIP_REASON)
class EngineMigration(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-engine-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)

    def write(self, relative: str, text: str) -> Path:
        path = self.workspace / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    # -- existing vaults continue to open ------------------------------------------------------

    def test_a_vault_written_before_any_manifest_opens_and_is_not_rewritten(self) -> None:
        self.write("purpose.md", "# Purpose\n\nKeep what we learn about auth.\n")
        self.write("wiki/auth.md", "# Auth\n\nSessions are created after [[state-check]] passes.\n")
        self.write("wiki/state-check.md", "---\ntitle: State check\nauthority: project_decision\n---\n# State check\n\nValidate OAuth state first.\n")
        before = tree(self.workspace)

        run(self.workspace, "init")
        run(self.workspace, "index-vault")
        hits = run_json(self.workspace, "search", "oauth state")
        self.assertTrue(any("State check" in json.dumps(hit) for hit in hits), hits)  # type: ignore[union-attr]

        # Opening and indexing must not change a single byte of the person's Markdown.
        self.assertEqual(tree(self.workspace), {**before, **{k: v for k, v in tree(self.workspace).items() if k == "athenaeum.json"}})
        # Identity is added on request, never required: a legacy vault has no manifest until then.
        self.assertFalse((self.workspace / "athenaeum.json").exists())
        info = run_json(self.workspace, "repo", "init")
        self.assertTrue(info["created"] or info["manifest"])  # type: ignore[index]
        self.assertEqual({k: v for k, v in tree(self.workspace).items() if k != "athenaeum.json"}, before)

    # -- rebuild from the vault ----------------------------------------------------------------

    def test_deleting_the_index_keeps_accepted_ids_links_evidence_and_authority(self) -> None:
        self.write("raw/sources/adr.md", "ADR-42: the retry budget is three.\n")
        run(self.workspace, "init")
        bridge = Bridge(self.workspace)
        caller = {"model": "validation-curator", "session": "one"}
        node = bridge.request(
            "create",
            input={"type": "decision", "title": "Retry budget is three", "summary": "Three retries, then fail."},
            caller=caller,
        )["node"]
        bridge.request("attach_evidence", input={"node": node["id"], "kind": "documentation", "anchor": {"file": "raw/sources/adr.md"}, "excerpt": "ADR-42"}, caller=caller)
        accepted = bridge.request("accept", node=node["id"], caller=caller)["accepted"]
        # A hand-written page links to the accepted one and declares its own authority.
        self.write(
            "wiki/policy.md",
            "---\nid: retry-policy\ntitle: Retry policy\nauthority: project_rule\nsources:\n  - raw/sources/adr.md\n---\n"
            f"# Retry policy\n\nSee [[{accepted['page']['path'][:-3]}]] for the budget.\n",
        )
        run(self.workspace, "index-vault")
        # A proposal nobody accepted is, by design, only in the index.
        pending = bridge.request("create", input={"type": "gotcha", "title": "Unreviewed thought", "summary": "Only a proposal."}, caller=caller)["node"]

        def snapshot() -> dict:
            out = {}
            for reference in (node["id"], "retry-policy"):
                detail = Bridge(self.workspace).request("open", node=reference)["detail"]
                out[reference] = {
                    "id": detail["node"]["id"],
                    "authority": detail["node"]["authority"],
                    "title": detail["node"]["title"],
                    "related": sorted((r["id"], r["edgeType"], r["outbound"]) for r in detail["relationships"]),
                    "evidence": sorted(
                        (e["kind"], (e.get("anchor") or {}).get("file")) for e in detail["evidence"] if (e.get("anchor") or {}).get("file", "").startswith("raw/")
                    ),
                    "page": detail["page"]["path"],
                }
            return out

        before = snapshot()
        self.assertEqual(before["retry-policy"]["authority"], "project_rule")
        self.assertIn(("raw/sources/adr.md"), [f for _, f in before["retry-policy"]["evidence"]])
        self.assertIn(node["id"], [r[0] for r in before["retry-policy"]["related"]], "the wikilink became an edge")

        files_before = tree(self.workspace)
        database = self.workspace / ".athenaeum/knowledge.db"
        for suffix in ("", "-wal", "-shm"):
            database.with_name(database.name + suffix).unlink(missing_ok=True)
        run(self.workspace, "index-vault")

        self.assertEqual(snapshot(), before, "ids, links, evidence references and authority survive a rebuild")
        self.assertEqual(tree(self.workspace), files_before, "rebuilding never edits the vault")
        self.assertNotIn(pending["id"], [p["node"]["id"] for p in Bridge(self.workspace).request("proposals")["proposals"]], "unaccepted proposals are index-only by design")

    # -- drafting from a source ----------------------------------------------------------------

    def test_draft_source_proposes_with_hash_evidence_and_refuses_outside_the_sources_tree(self) -> None:
        source = self.write("raw/sources/oauth.md", "OAuth callback checks state before creating a session.\nMore detail.\n")
        self.write("notes/private.md", "not a source\n")
        run(self.workspace, "init")
        drafted = run_json(self.workspace, "draft-source", "--source", "raw/sources/oauth.md", "--title", "OAuth state first")
        self.assertEqual(drafted["node"]["summary"], "OAuth callback checks state before creating a session.")  # type: ignore[index]
        self.assertEqual(drafted["evidence"]["anchor"]["contentHash"], hashlib.sha256(source.read_bytes()).hexdigest())  # type: ignore[index]
        self.assertEqual(list((self.workspace / "wiki").rglob("*.md")) if (self.workspace / "wiki").exists() else [], [])
        proposals = Bridge(self.workspace).request("proposals")["proposals"]
        self.assertEqual([p["node"]["title"] for p in proposals], ["OAuth state first"])

        for bad in ("notes/private.md", "../outside.md", "raw/sources/../../etc/passwd", "/etc/passwd"):
            process = run(self.workspace, "draft-source", "--source", bad, "--title", "Nope", check=False)
            self.assertNotEqual(process.returncode, 0, bad)

        link = self.workspace / "raw/sources/link.md"
        try:
            link.symlink_to(self.workspace / "notes/private.md")
        except OSError:
            self.skipTest("symlinks unavailable")
        self.assertNotEqual(run(self.workspace, "draft-source", "--source", "raw/sources/link.md", "--title", "Via link", check=False).returncode, 0)

    # -- the retired athenaeum-core index ------------------------------------------------------

    def legacy_index(self) -> None:
        run(self.workspace, "init")
        path = self.workspace / ".athenaeum/local-core.db"
        database = sqlite3.connect(path)
        database.executescript(LEGACY_SCHEMA)

        def node(identifier: str, kind: str, title: str, summary: str) -> str:
            return json.dumps({"id": identifier, "type": kind, "title": title, "summary": summary, "content": "", "scope": "project", "authority": "unverified_observation", "status": "proposed", "provenance": "model:old-curator", "path": "", "imported": False, "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-01T00:00:00Z"})

        for identifier, state, kind, title, summary in (
            ("node_old_one", "pending", "gotcha", "Retry budget is three", "More retries amplify an outage."),
            ("node_old_two", "pending", "decision", "Use jittered backoff", "Jitter avoids thundering herds."),
            ("node_old_three", "rejected", "concept", "Rejected idea", "nope"),
        ):
            database.execute("INSERT INTO proposals VALUES (?,?,?,?)", (identifier, state, node(identifier, kind, title, summary), "2026-10-01T00:00:00Z"))
        database.execute("INSERT INTO proposal_evidence VALUES (?,?)", ("ev_old", json.dumps({"id": "ev_old", "node": "node_old_one", "kind": "documentation", "path": "raw/sources/retry.md", "excerpt": "Retries amplify outages.", "note": None, "sha256": None, "anchor": {"file": "raw/sources/retry.md"}, "createdAt": "2026-10-01T00:00:00Z"})))
        database.execute("INSERT INTO proposal_edges VALUES (?,?)", ("edge_old", json.dumps({"id": "edge_old", "from": "node_old_one", "to": "node_old_two", "type": "related_to", "established": False, "source": "model:old-curator", "createdAt": "2026-10-01T00:00:00Z"})))
        database.commit()
        database.close()

    def test_proposals_in_the_retired_core_index_are_imported_once_and_the_old_database_is_untouched(self) -> None:
        self.write("raw/sources/retry.md", "Retries amplify outages.\n")
        self.legacy_index()
        legacy = self.workspace / ".athenaeum/local-core.db"
        before = sha256(legacy)

        dry = run_json(self.workspace, "import-legacy-core")
        self.assertTrue(dry["dryRun"] and dry["found"])  # type: ignore[index]
        self.assertEqual(sorted(i["title"] for i in dry["imported"]), ["Retry budget is three", "Use jittered backoff"])  # type: ignore[index]
        self.assertEqual(Bridge(self.workspace).request("proposals")["proposals"], [], "a dry run changes nothing")

        applied = run_json(self.workspace, "import-legacy-core", "--apply")
        self.assertFalse(applied["dryRun"])  # type: ignore[index]
        proposals = {p["node"]["title"]: p for p in Bridge(self.workspace).request("proposals")["proposals"]}
        self.assertEqual(sorted(proposals), ["Retry budget is three", "Use jittered backoff"])
        retry = proposals["Retry budget is three"]
        self.assertEqual(retry["node"]["provenance"]["detail"], "old-curator")
        self.assertEqual([e["anchor"]["file"] for e in retry["evidence"]], ["raw/sources/retry.md"])
        self.assertEqual([r["title"] for r in retry["relationships"]], ["Use jittered backoff"])
        self.assertTrue(any("Rejected idea" in note for note in applied["leftAlone"]))  # type: ignore[index]

        again = run_json(self.workspace, "import-legacy-core", "--apply")
        self.assertEqual(again["imported"], [], "re-running never duplicates")  # type: ignore[index]
        self.assertEqual(len(Bridge(self.workspace).request("proposals")["proposals"]), 2)
        # sqlite may add -wal/-shm beside it, but the database file itself is never changed.
        self.assertEqual(sha256(legacy), before)

    def test_nothing_to_import_is_reported_not_an_error(self) -> None:
        run(self.workspace, "init")
        report = run_json(self.workspace, "import-legacy-core")
        self.assertFalse(report["found"])  # type: ignore[index]


if __name__ == "__main__":
    unittest.main()



class VaultLifecycle(unittest.TestCase):
    """The vault is the product: it can be exported, imported, rebuilt and never overwritten."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-lifecycle-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.workspace = self.root / "vault"
        self.workspace.mkdir()

    def write(self, relative: str, text: str) -> Path:
        path = self.workspace / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_init_is_idempotent_and_never_rewrites_the_vault(self) -> None:
        self.write("wiki/state.md", "---\nid: state\ntitle: State check\nsummary: Validate state first.\n---\n# State check\n")
        before = tree(self.workspace)
        run(self.workspace, "init")
        run(self.workspace, "init")
        self.assertEqual(tree(self.workspace), before, "init must not rewrite the person's Markdown")
        self.assertTrue((self.workspace / ".athenaeum").is_dir())

    def test_an_export_and_import_round_trip_keeps_ids_authority_and_evidence(self) -> None:
        run(self.workspace, "init")
        self.write("raw/sources/oauth.md", "OAuth callback checks state.\n")
        caller = {"model": "validation-curator", "session": "one"}
        bridge = Bridge(self.workspace)
        node = bridge.request(
            "create",
            input={"type": "decision", "title": "State before session", "summary": "Validate state first."},
            caller=caller,
        )["node"]
        bridge.request(
            "attach_evidence",
            input={"node": node["id"], "kind": "source_file", "anchor": {"file": "raw/sources/oauth.md"}},
            caller=caller,
        )
        accepted = bridge.request("accept", node=node["id"], caller=caller)["accepted"]
        page = self.workspace / accepted["page"]["path"]
        original = page.read_bytes()

        archive = self.root / "round-trip.zip"
        run(self.workspace, "export", str(archive))
        destination = self.root / "restored"
        run(self.workspace, "import", str(archive), str(destination))

        restored_page = destination / accepted["page"]["path"]
        self.assertEqual(restored_page.read_bytes(), original, "a round trip preserves the page bytes")
        detail = Bridge(destination).request("open", node=node["id"])["detail"]
        self.assertEqual(detail["node"]["authority"], "unverified_observation")
        anchors = [item["anchor"]["file"] for item in detail["evidence"] if item.get("anchor", {}).get("file")]
        self.assertIn("raw/sources/oauth.md", anchors, "the citation travels with the knowledge")

    def test_stated_knowledge_is_written_and_a_hand_written_page_is_never_overwritten(self) -> None:
        run(self.workspace, "init")
        stated = run_json(self.workspace, "add", "-t", "rule", "Jitter the backoff", "-s", "Jitter avoids thundering herds.", "--stated")
        identifier = stated["id"]  # type: ignore[index]
        pages = sorted((self.workspace / "wiki").rglob("*.md"))
        self.assertTrue(pages, "user-stated knowledge is durable without an explicit accept")
        mine = self.write("wiki/mine.md", "---\nid: mine\ntitle: Mine\nsummary: Hand written.\n---\n# Mine\n")
        before = mine.read_bytes()
        report = run_json(self.workspace, "vault", "materialize", "--apply")
        self.assertTrue(all(not path.endswith("mine.md") for path in report["written"]))  # type: ignore[index]
        self.assertEqual(mine.read_bytes(), before, "materialize never overwrites a page")
        self.assertEqual(run_json(self.workspace, "show", identifier)["id"], identifier)  # type: ignore[index]


if __name__ == "__main__":
    unittest.main()
