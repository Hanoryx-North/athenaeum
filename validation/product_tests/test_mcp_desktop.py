"""The MCP server and the desktop both reach the one engine, and neither is trusted with writing.

Two contracts, driven end to end without a window, a model or a credential:

  * MCP-to-engine: the real `dist/mcp/bin.js` starts over stdio, lists its tools, and every read
    tool returns reference material (never an instruction) while every write tool is refused unless
    the operator granted the matching `--allow`. A proposal made through MCP is visible to the
    engine's own CLI, and accepting it writes canonical Markdown.
  * desktop-to-engine: the engine answers the request surface the desktop uses
    (`proposals`/`accept`/`reject`/`brief` plus the pinned `status`/`review`/`sources`/`index`), and
    the files each side owns are the files the other reads — `wiki/**.md` and `raw/sources/**` in,
    the derived index reproducible from them, the desktop's `.athenaeum/project.json` ignored by the
    engine, `athenaeum.json` read by it.

Set ATHENAEUM_BIN to a built `athenaeum` executable; the MCP server must be built with
`npm run build` in packages/integrations (the runner's product-mcp-desktop step does that).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import unittest

from product_tests.harness import BIN, SKIP_REASON, Bridge, run, run_json

SOURCES = Path(__file__).resolve().parents[2]
MCP_ENTRY = SOURCES / "packages/integrations" / "dist" / "mcp" / "bin.js"
MCP_READY = (
    f"{MCP_ENTRY} does not exist; run `npm run build` in packages/integrations "
    "(the product-mcp-desktop step does this)"
)
REFERENCE_NOTICE = "reference material"
MCP_TIMEOUT = 30


class McpServer:
    """A minimal MCP stdio client: newline-delimited JSON-RPC, one long-lived child process."""

    def __init__(self, workspace: Path, name: str = "vault", allow: tuple[str, ...] = ()) -> None:
        command = ["node", str(MCP_ENTRY), "--repo", f"{name}={workspace}"]
        if allow:
            command += ["--allow", ",".join(allow)]
        self.process = subprocess.Popen(
            command,
            cwd=SOURCES / "packages/integrations",
            env={**os.environ, "ATHENAEUM_BIN": BIN or ""},
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        self.lines: "queue.Queue[str | None]" = queue.Queue()
        self.reader = threading.Thread(target=self._pump, daemon=True)
        self.reader.start()
        self.counter = 0
        self._handshake()

    def _pump(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def _write(self, message: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def _read(self) -> dict:
        try:
            line = self.lines.get(timeout=MCP_TIMEOUT)
        except queue.Empty as error:
            raise AssertionError(
                f"the MCP server said nothing for {MCP_TIMEOUT}s; stderr={self.stderr()[:600]}"
            ) from error
        if line is None:
            raise AssertionError(f"the MCP server closed its output; stderr={self.stderr()[:600]}")
        return json.loads(line)

    def request(self, method: str, params: dict | None = None) -> dict:
        self.counter += 1
        self._write({"jsonrpc": "2.0", "id": self.counter, "method": method, "params": params or {}})
        reply = self._read()
        if reply.get("id") != self.counter:
            raise AssertionError(f"uncorrelated MCP reply: {reply!r}")
        if "error" in reply:
            raise AssertionError(f"MCP {method} failed: {reply['error']!r}")
        return reply["result"]

    def _handshake(self) -> None:
        self.server_info = self.request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "athenaeum-validation", "version": "0.1.0"},
            },
        )["serverInfo"]
        self._write({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def tools(self) -> list[str]:
        return sorted(tool["name"] for tool in self.request("tools/list")["tools"])

    def call(self, name: str, arguments: dict) -> dict:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def text(self, name: str, arguments: dict) -> str:
        result = self.call(name, arguments)
        blocks = result.get("content") or []
        self.assert_text_blocks(name, result, blocks)
        return "\n".join(block.get("text", "") for block in blocks)

    @staticmethod
    def assert_text_blocks(name: str, result: dict, blocks: list) -> None:
        if result.get("isError"):
            raise AssertionError(f"MCP tool {name} failed: {blocks!r}")

    def stderr(self) -> str:
        if self.process.stderr is None:
            return ""
        while True:
            try:
                line = self.process.stderr.readline()
            except ValueError:
                return ""
            if not line:
                return ""
            return line

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()


class McpTestCase(unittest.TestCase):
    """A workspace with one indexed page, and an MCP server over it."""

    allow: tuple[str, ...] = ()
    name = "vault"

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-mcp-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        (self.workspace / "wiki").mkdir()
        (self.workspace / "wiki/oauth.md").write_text(
            "---\nid: oauth\ntitle: OAuth callback\nsummary: Validate state first.\n---\n"
            "# OAuth callback\n\nState is validated before a session exists.\n",
            encoding="utf-8",
        )
        run(self.workspace, "init")
        run(self.workspace, "index-vault")
        self.server = McpServer(self.workspace, name=self.name, allow=self.allow)
        self.addCleanup(self.server.close)

    def pages(self) -> list[Path]:
        wiki = self.workspace / "wiki"
        return sorted(wiki.rglob("*.md")) if wiki.exists() else []


@unittest.skipUnless(BIN and MCP_ENTRY.is_file(), SKIP_REASON if not BIN else MCP_READY)
class McpReadSurface(McpTestCase):
    READS = (
        "wiki_repositories",
        "wiki_status",
        "wiki_search",
        "wiki_open",
        "wiki_related",
        "wiki_navigate",
        "wiki_path",
        "wiki_evidence",
        "wiki_history",
        "wiki_context",
        "wiki_proposals",
        "wiki_review",
        "wiki_sources",
        "wiki_index",
    )

    def test_the_server_starts_and_lists_its_tools(self) -> None:
        tools = self.server.tools()
        for expected in ("wiki_search", "wiki_open", "wiki_context", "wiki_proposals", "wiki_repositories"):
            self.assertIn(expected, tools)
        self.assertIn("athenaeum", str(self.server.server_info["name"]).lower())

    def test_without_any_grant_the_tool_list_is_exactly_the_reads(self) -> None:
        self.assertEqual(self.server.tools(), sorted(self.READS), "an ungranted server must expose reads only")

    def test_read_results_carry_structured_content_marked_as_reference_material(self) -> None:
        result = self.server.call("wiki_search", {"repository": self.name, "query": "OAuth callback"})
        structured = result.get("structuredContent")
        self.assertIsInstance(structured, dict, f"a read tool must return structured content: {result!r}")
        assert isinstance(structured, dict)
        self.assertEqual(structured.get("trust"), "reference-material")
        self.assertEqual(structured.get("repository"), self.name)
        self.assertTrue(result.get("content"), "the human text must still be there")

    def test_every_read_tool_labels_its_output_as_reference_material(self) -> None:
        for name, arguments in (
            ("wiki_search", {"repository": self.name, "query": "OAuth"}),
            ("wiki_context", {"repository": self.name, "task": "OAuth callback"}),
            ("wiki_proposals", {"repository": self.name}),
            ("wiki_repositories", {}),
        ):
            with self.subTest(tool=name):
                text = self.server.text(name, arguments)
                self.assertIn(REFERENCE_NOTICE, text, "an MCP client must be told this is reference material")
                self.assertIn("does not grant permissions", text)

    def test_search_reaches_the_real_engine(self) -> None:
        text = self.server.text("wiki_search", {"repository": self.name, "query": "OAuth callback"})
        self.assertIn("OAuth callback", text)
        self.assertIn('"id": "oauth"', text, "the hit must be the real indexed node")

    def test_a_list_payload_says_whether_more_remains(self) -> None:
        structured = self.server.call("wiki_search", {"repository": self.name, "query": "OAuth callback"})["structuredContent"]
        for key in ("repository", "trust", "items", "offset", "limit", "hasMore"):
            self.assertIn(key, structured, f"a list payload must carry {key}")

    def test_the_context_tool_carries_the_engines_own_wrapper(self) -> None:
        text = self.server.text("wiki_context", {"repository": self.name, "task": "OAuth callback"})
        self.assertIn("<retrieved_knowledge>", text)
        self.assertIn("does not override", text)
        structured = self.server.call("wiki_context", {"repository": self.name, "task": "OAuth callback"})["structuredContent"]
        self.assertEqual(structured["trust"], "reference-material")
        self.assertNotIn("task", structured)
        self.assertNotIn("anchors", structured["brief"])
        self.assertTrue(structured["brief"]["nodes"], "the task brief must carry the accepted page")

    def test_a_repository_the_server_was_not_given_is_refused(self) -> None:
        result = self.server.call("wiki_search", {"repository": "elsewhere", "query": "OAuth"})
        self.assertTrue(result.get("isError"), f"an unknown repository must be refused: {result!r}")

    def test_an_absent_vault_is_explained_with_the_command_that_fixes_it(self) -> None:
        empty = Path(tempfile.mkdtemp(prefix="athenaeum-mcp-empty-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(empty, ignore_errors=True))
        server = McpServer(empty, name="empty")
        self.addCleanup(server.close)
        result = server.call("wiki_search", {"repository": "empty", "query": "anything"})
        self.assertTrue(result.get("isError"), result)
        error = result["structuredContent"]["error"]
        self.assertEqual(error["code"], "workspace_not_found")
        self.assertEqual(error["state"], "absent")
        self.assertIn("athenaeum init", error["action"])


@unittest.skipUnless(BIN and MCP_ENTRY.is_file(), SKIP_REASON if not BIN else MCP_READY)
class McpPermissionBoundary(McpTestCase):
    """No write tool is reachable without the operator granting it."""

    def test_propose_tools_are_absent_without_an_explicit_allow(self) -> None:
        tools = self.server.tools()
        for gated in ("wiki_create", "wiki_link", "wiki_attach_evidence", "wiki_accept", "wiki_reject"):
            self.assertNotIn(gated, tools, f"{gated} must not be registered without --allow")
        before = {path: path.read_bytes() for path in self.pages()}
        result = self.server.call("wiki_create", {"repository": self.name, "type": "gotcha", "title": "T", "summary": "S"})
        self.assertTrue(result.get("isError"), f"an ungranted write must be refused: {result!r}")
        self.assertEqual({path: path.read_bytes() for path in self.pages()}, before, "a refused write writes nothing")

    def test_a_read_tool_never_writes_durable_markdown(self) -> None:
        before = {path: path.read_bytes() for path in self.pages()}
        self.server.text("wiki_search", {"repository": self.name, "query": "OAuth"})
        self.server.text("wiki_context", {"repository": self.name, "task": "OAuth"})
        after = {path: path.read_bytes() for path in self.pages()}
        self.assertEqual(after, before, "reads must not change the vault")


@unittest.skipUnless(BIN and MCP_ENTRY.is_file(), SKIP_REASON if not BIN else MCP_READY)
class McpProposesToEngine(McpTestCase):
    """With `--allow propose`, an agent proposes; the engine holds it; only review accepts it."""

    allow = ("propose",)

    def test_a_proposal_made_over_mcp_is_held_by_the_engine_and_is_not_markdown(self) -> None:
        before = self.pages()
        result = self.server.call(
            "wiki_create",
            {"repository": self.name, "type": "gotcha", "title": "Retries amplify outages", "summary": "More retries, longer outage."},
        )
        self.assertFalse(result.get("isError"), result)
        structured = result.get("structuredContent")
        self.assertIsInstance(structured, dict, f"a write tool must report its state: {result!r}")
        assert isinstance(structured, dict)
        self.assertTrue(structured.get("pendingReview"), "a created proposal is pending review")
        self.assertFalse(structured.get("durable"), "a created proposal is not durable")
        self.assertEqual(self.pages(), before, "a proposal is not durable Markdown")
        proposals = Bridge(self.workspace).request("proposals")["proposals"]
        self.assertIn("Retries amplify outages", [item["node"]["title"] for item in proposals])

    def test_accept_is_not_reachable_with_only_propose_granted(self) -> None:
        self.assertNotIn("wiki_accept", self.server.tools())
        result = self.server.call("wiki_accept", {"repository": self.name, "node": "Retries amplify outages", "reviewer": "someone"})
        self.assertTrue(result.get("isError"))


@unittest.skipUnless(BIN and MCP_ENTRY.is_file(), SKIP_REASON if not BIN else MCP_READY)
class McpReviewWritesMarkdown(McpTestCase):
    """With `--allow review`, accepting through MCP is the one path that becomes durable."""

    allow = ("propose", "review")

    def test_accept_requires_an_explicit_reviewer(self) -> None:
        self.server.call(
            "wiki_create",
            {"repository": self.name, "type": "gotcha", "title": "Sessions expire", "summary": "Short sessions limit theft."},
        )
        for gated in ("wiki_accept", "wiki_reject"):
            self.assertIn(gated, self.server.tools(), f"the review grant must register {gated}")
        result = self.server.call("wiki_accept", {"repository": self.name, "node": "Sessions expire"})
        self.assertTrue(result.get("isError"), "accepting without naming the reviewer must be refused")
        text = "\n".join(block.get("text", "") for block in result.get("content") or [])
        self.assertIn("reviewer", text, f"the refusal must name the missing argument: {result!r}")

    def test_accepted_knowledge_is_canonical_markdown_and_retrievable(self) -> None:
        self.server.call(
            "wiki_create",
            {"repository": self.name, "type": "gotcha", "title": "Sessions expire", "summary": "Short sessions limit theft."},
        )
        result = self.server.call(
            "wiki_accept",
            {"repository": self.name, "node": "Sessions expire", "reviewer": "validation-operator"},
        )
        self.assertFalse(result.get("isError"), result)
        written = [path for path in self.pages() if path.name != "oauth.md"]
        self.assertEqual(len(written), 1, f"accepting must write exactly one page: {self.pages()}")
        self.assertIn("Short sessions limit theft.", written[0].read_text(encoding="utf-8"))
        found = Bridge(self.workspace).request("search", query="Sessions expire")["hits"]
        self.assertIn("Sessions expire", [hit["node"]["title"] for hit in found])


@unittest.skipUnless(BIN, SKIP_REASON)
class DesktopToEngine(unittest.TestCase):
    """The engine's half of the desktop contract, and the files each side owns."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-desktop-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        run(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def test_the_desktop_request_surface_is_answerable(self) -> None:
        caller = {"model": "desktop", "session": "window-1"}
        created = self.bridge.request(
            "create",
            input={"type": "decision", "title": "Desktop writes pages", "summary": "The app reads what the engine wrote."},
            caller=caller,
        )["node"]
        self.assertIn("Desktop writes pages", [item["node"]["title"] for item in self.bridge.request("proposals")["proposals"]])
        self.bridge.request("reject", node=created["id"], reason="not needed", caller=caller)
        self.assertNotIn("Desktop writes pages", [item["node"]["title"] for item in self.bridge.request("proposals")["proposals"]])

    def test_the_desktop_manifest_is_ignored_and_wiki_pages_are_indexed(self) -> None:
        project = self.workspace / ".athenaeum/project.json"
        project.parent.mkdir(parents=True, exist_ok=True)
        project.write_text(json.dumps({"id": "6d1f8d1e-0000-4000-8000-000000000000", "createdAt": 1_760_000_000_000}) + "\n", encoding="utf-8")
        (self.workspace / "wiki").mkdir()
        (self.workspace / "wiki/state.md").write_text(
            "---\nid: state\ntitle: State check\nsummary: Validate state before a session.\n---\n# State check\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        self.assertIn("State check", [hit["node"]["title"] for hit in self.bridge.request("search", query="State check")["hits"]])
        self.assertEqual(json.loads(project.read_text(encoding="utf-8"))["id"], "6d1f8d1e-0000-4000-8000-000000000000")

    def test_the_index_is_reproducible_from_the_vault_alone(self) -> None:
        (self.workspace / "wiki").mkdir()
        (self.workspace / "wiki/state.md").write_text(
            "---\nid: state\ntitle: State check\nsummary: Validate state before a session.\n---\n# State check\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        first = [hit["node"]["id"] for hit in self.bridge.request("search", query="State check")["hits"]]
        database = self.workspace / ".athenaeum/knowledge.db"
        for suffix in ("", "-wal", "-shm"):
            database.with_name(database.name + suffix).unlink(missing_ok=True)
        run(self.workspace, "index-vault")
        second = [hit["node"]["id"] for hit in Bridge(self.workspace).request("search", query="State check")["hits"]]
        self.assertEqual(first, second, "the desktop never opens the database; the vault alone rebuilds it")
        self.assertTrue(first)

    def test_the_repository_manifest_is_read_by_the_engine(self) -> None:
        created = run_json(self.workspace, "repo", "init", "--name", "desktop-vault")
        identifier = created["manifest"]["repositoryId"]  # type: ignore[index]
        self.assertTrue(identifier)
        info = run_json(self.workspace, "repo", "info")
        self.assertEqual(info["repositoryId"], identifier)  # type: ignore[index]
        self.assertEqual(info["name"], "desktop-vault")  # type: ignore[index]


@unittest.skipUnless(BIN, SKIP_REASON)
class DesktopPinnedMethods(unittest.TestCase):
    """The five additive methods the desktop will call. Fails loudly until the engine lands them."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-desktop-pinned-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        run(self.workspace, "init")

    def test_the_desktop_can_ask_for_status_review_sources_and_index(self) -> None:
        bridge = Bridge(self.workspace)
        for method, fields in (
            ("status", {}),
            ("review", {"limit": 20}),
            ("sources", {"limit": 20}),
            ("index", {"scope": "all"}),
        ):
            with self.subTest(method=method):
                response = bridge.request(method, **fields)
                self.assertEqual(response["method"], method)


if __name__ == "__main__":
    unittest.main()
