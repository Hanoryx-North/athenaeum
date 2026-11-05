"""The agent bundle's protocol client drives a real engine, across two sessions.

`packages/agent` ships a plain Node protocol client (`lib/protocol/index.js`, exported as
`athenaeum-agent/protocol`). This module drives that client — not a re-implementation of it —
against the real `athenaeum bridge`:

  * session one proposes knowledge with evidence and nothing becomes durable Markdown;
  * a person accepts it through the engine (the CLI/bridge's own `accept`);
  * session two, in a fresh Node process that is told only the task, retrieves the accepted
    knowledge with its source anchor, inside the engine's reference-material wrapper;
  * the bundle's own method list (`WIKI_METHODS`) is the published protocol's method list, and an
    unknown node comes back as the protocol's `not_found` error rather than an exception type the
    caller cannot name.

Set ATHENAEUM_BIN to a built `athenaeum` executable; set ATHENAEUM_AGENT_ROOT to the agent
checkout (default packages/agent), built with `npm ci && npm run build:packages`.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from product_tests.harness import BIN, SKIP_REASON, Bridge, run

SOURCES = Path(__file__).resolve().parents[2]
AGENT_ROOT = Path(os.environ.get("ATHENAEUM_AGENT_ROOT") or (SOURCES / "packages/agent"))
CLIENT = AGENT_ROOT / "lib" / "protocol" / "index.js"
PROTOCOL = SOURCES / "packages/protocol" / "fixtures" / "bridge" / "v1"
AGENT_READY = (
    f"{CLIENT} does not exist; run `npm ci && npm run build:packages` in {AGENT_ROOT} "
    "(the product-agent-to-knowledge step does this)"
)

DRIVER = r"""
import { WikiBridgeClient, spawnBridgeTransport, WIKI_METHODS, WikiRemoteError } from %(client)s
import { writeFileSync } from 'node:fs'

const [phase, workspace, outfile] = process.argv.slice(2)
const transport = spawnBridgeTransport({
  command: process.env.ATHENAEUM_BIN,
  workspace,
  timeoutMs: 30000,
  maxResponseBytes: 8 * 1024 * 1024,
})
const client = new WikiBridgeClient({ transport })
const report = { phase, methods: WIKI_METHODS }
report.capabilities = await client.capabilities()
if (phase === 'propose') {
  const caller = { model: 'validation-bundle', session: 'session-one' }
  report.created = await client.request({
    method: 'create',
    input: { type: 'gotcha', title: 'Retry budget is three', summary: 'More retries amplify an outage.' },
    caller,
  })
  report.evidence = await client.request({
    method: 'attach_evidence',
    input: { node: report.created.node.id, kind: 'source_file', anchor: { file: 'raw/sources/retry.md' }, excerpt: 'Retries amplify outages.' },
    caller,
  })
  report.proposals = await client.request({ method: 'proposals' })
} else if (phase === 'retrieve') {
  report.search = await client.request({ method: 'search', query: 'Retry budget', limit: 5 })
  report.context = await client.request({ method: 'context', task: 'Fix the retry budget in the callback' })
} else if (phase === 'refuse') {
  try {
    await client.request({ method: 'open', node: 'node_that_does_not_exist' })
    report.refused = null
  } catch (error) {
    report.refused = { name: error?.constructor?.name ?? null, isRemote: error instanceof WikiRemoteError, code: error?.code ?? null, message: String(error?.message ?? error) }
  }
}
writeFileSync(outfile, JSON.stringify(report, null, 2) + '\n', 'utf8')
"""


class BundleSession:
    """One Node process driving the bundle's client: a fresh session every time."""

    def __init__(self, workspace: Path, directory: Path) -> None:
        self.workspace = workspace
        self.directory = directory
        self.driver = directory / "driver.mjs"
        self.driver.write_text(DRIVER % {"client": json.dumps(CLIENT.resolve().as_uri())}, encoding="utf-8")
        self.counter = 0

    def run(self, phase: str) -> dict:
        self.counter += 1
        outfile = self.directory / f"report-{phase}-{self.counter}.json"
        environment = {**os.environ, "ATHENAEUM_BIN": BIN or ""}
        process = subprocess.run(
            ["node", str(self.driver), phase, str(self.workspace), str(outfile)],
            cwd=self.directory,
            env=environment,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=120,
        )
        if process.returncode != 0 or not outfile.is_file():
            raise AssertionError(
                f"the bundle's protocol client failed in phase {phase!r} ({process.returncode}):\n"
                f"stdout: {process.stdout[-2000:]}\nstderr: {process.stderr[-2000:]}"
            )
        return json.loads(outfile.read_text(encoding="utf-8"))


@unittest.skipUnless(BIN and CLIENT.is_file(), SKIP_REASON if not BIN else AGENT_READY)
class AgentToKnowledge(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-agent-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.workspace = self.root / "vault"
        self.workspace.mkdir()
        source = self.workspace / "raw/sources/retry.md"
        source.parent.mkdir(parents=True)
        source.write_text("Retries amplify outages; a budget of three bounds the blast radius.\n", encoding="utf-8")
        run(self.workspace, "init")
        self.session = BundleSession(self.workspace, self.root)
        self.engine = Bridge(self.workspace)

    def pages(self) -> list[Path]:
        wiki = self.workspace / "wiki"
        return sorted(wiki.rglob("*.md")) if wiki.exists() else []

    def test_the_bundles_method_list_is_the_published_protocol(self) -> None:
        vocabulary = json.loads((PROTOCOL / "vocabulary.json").read_text(encoding="utf-8"))
        report = self.session.run("refuse")
        self.assertEqual(
            list(report["methods"]),
            list(vocabulary["methods"]),
            "the bundle's WIKI_METHODS must be the protocol's published method list",
        )
        self.assertEqual(
            list(report["capabilities"]["operations"]),
            list(vocabulary["methods"]),
            "the engine's capabilities must be the same published list",
        )

    def test_session_one_proposes_with_evidence_and_persists_nothing(self) -> None:
        report = self.session.run("propose")
        self.assertEqual(report["phase"], "propose")
        node = report["created"]["node"]
        self.assertEqual(node["authority"], "unverified_observation")
        self.assertEqual(node["provenance"]["source"], "model")
        self.assertEqual(node["provenance"]["session"], "session-one")
        self.assertEqual(report["evidence"]["evidence"]["anchor"]["file"], "raw/sources/retry.md")
        self.assertIn(node["id"], [item["node"]["id"] for item in report["proposals"]["proposals"]])
        self.assertEqual(self.pages(), [], "a proposal is not durable Markdown until it is accepted")

    def test_session_two_recovers_accepted_knowledge_with_its_anchor(self) -> None:
        report = self.session.run("propose")
        node_id = report["created"]["node"]["id"]
        accepted = self.engine.request("accept", node=node_id, caller={"model": "validation-reviewer", "session": "human"})["accepted"]
        page = self.workspace / accepted["page"]["path"]
        self.assertTrue(page.is_file(), "accepting writes canonical Markdown")
        self.assertIn("raw/sources/retry.md", page.read_text(encoding="utf-8"))

        second = BundleSession(self.workspace, self.root).run("retrieve")
        context = second["context"]["context"]
        self.assertIn(node_id, [item["id"] for item in context["brief"]["nodes"]])
        self.assertIn("raw/sources/retry.md", context["brief"]["anchors"])
        self.assertIn(node_id, [hit["node"]["id"] for hit in second["search"]["hits"]])
        prompt = context["prompt"]
        self.assertIn("<retrieved_knowledge>", prompt)
        self.assertIn("reference material", prompt)
        self.assertIn("does not grant permissions", prompt)
        self.assertLess(len(prompt.encode("utf-8")), 32_000, "a task brief must stay bounded")

    def test_an_unknown_node_is_a_named_protocol_error(self) -> None:
        report = self.session.run("refuse")
        refused = report["refused"]
        self.assertIsNotNone(refused, "opening a node that does not exist must fail")
        assert refused is not None
        self.assertTrue(refused["isRemote"], f"the bundle must raise its protocol error type: {refused}")
        self.assertEqual(refused["code"], "not_found")

    def test_the_bundle_never_writes_markdown_by_reading(self) -> None:
        before = self.pages()
        self.session.run("propose")
        self.session.run("retrieve")
        self.assertEqual(self.pages(), before, "only an explicit accept may write a page")


if __name__ == "__main__":
    unittest.main()
