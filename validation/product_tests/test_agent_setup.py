"""AthenaeumAgent setup and the installed memory connection, without a model or credentials."""
from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from product_tests.harness import BIN, SKIP_REASON, child_env, run
from product_tests.mcp_fixture import McpSession, build_bundle

NODE = shutil.which("node")


@unittest.skipUnless(BIN, SKIP_REASON)
@unittest.skipUnless(NODE, "needs Node for the installed MCP integration")
class AthenaeumAgentMemorySetup(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.bundle = build_bundle()

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="agent wiki memory ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.project = self.root / "project"
        self.project.mkdir()
        self.agent = self.root / "custom agent home"
        self.agent.mkdir()
        self.config = self.agent / "config.json"
        self.config.write_text('{"model":"user-model"}\n', encoding="utf-8")
        self.instructions = self.agent / "AGENT.md"
        self.instructions.write_text("Keep my coding conventions.\n", encoding="utf-8")
        self.env = child_env({"HOME": str(self.root / "home"), "USERPROFILE": str(self.root / "home"),
                              "ATHENAEUM_AGENT_HOME": str(self.agent), "ATHENAEUM_BIN": str(BIN)})
        for name in ("CLAUDE_CONFIG_DIR", "ATHENAEUM_INTEGRATIONS", "ATHENAEUM_LIBRARY_URL", "ATHENAEUM_LIBRARY_TOKEN"):
            self.env.pop(name, None)
        run(self.project, "setup", "--yes", "--no-library", "--no-agents", "--skip-index", env=self.env)

    def integration(self, command: str, *args: str, check: bool = True) -> dict:
        result = subprocess.run([NODE, str(self.bundle), command, "--agent", "athenaeum", "--athenaeum", BIN,
                                 "--json", *args], cwd=self.project, env=self.env,
                                capture_output=True, encoding="utf-8", timeout=30)
        if check:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(result.stdout)

    def setup_wiki(self) -> None:
        result = run(self.project, "setup", "--yes", "--no-library", "--skip-index", "--agents", "athenaeum",
                     "--capture", "instruct", "--integrations", str(self.bundle), "--json", env=self.env)
        self.assertTrue(json.loads(result.stdout)["ok"])

    def test_setup_repeat_disconnect_preserves_user_settings(self) -> None:
        self.setup_wiki()
        installed = self.instructions.read_text(encoding="utf-8")
        self.assertTrue(installed.startswith("Keep my coding conventions.\n"))
        self.assertIn("at the start of each task", installed)
        self.assertIn("Record before finishing", installed)
        self.assertIn("Do not start a subagent", installed)
        self.assertIn("Do not accept your own proposals", installed)
        self.assertIn("Keep memory quiet", installed)
        self.assertNotIn("briefly identify any proposals", installed)
        self.assertNotIn("mention the limitation once", installed)
        self.assertEqual(self.integration("connect")["agents"][0]["status"], "unchanged")
        self.assertEqual(self.instructions.read_text(encoding="utf-8"), installed)
        self.assertEqual(Path(str(self.instructions) + ".athenaeum-backup").read_text(encoding="utf-8"), "Keep my coding conventions.\n")
        self.integration("disconnect")
        self.assertEqual(self.instructions.read_text(encoding="utf-8"), "Keep my coding conventions.\n")
        self.assertEqual(json.loads(self.config.read_text(encoding="utf-8")), {"model": "user-model"})

    def test_manual_mcp_configuration_is_preserved(self) -> None:
        config = {"model": "user-model", "mcpServers": {"athenaeum": {"command": "my-server"}}}
        self.config.write_text(json.dumps(config), encoding="utf-8")
        report = self.integration("connect", check=False)
        self.assertFalse(report["ok"])
        self.assertEqual(report["agents"][0]["status"], "conflict")
        self.assertEqual(json.loads(self.config.read_text(encoding="utf-8")), config)

    def test_doctor_detects_missing_and_outdated_memory_instructions(self) -> None:
        self.integration("connect")
        for text in ("Keep my coding conventions.\n", self.instructions.read_text(encoding="utf-8").replace("Recall before work", "Old recall guidance")):
            self.instructions.write_text(text, encoding="utf-8")
            report = self.integration("doctor", check=False)
            check = next(c for c in report["checks"] if c["id"] == "agent-athenaeum")
            self.assertFalse(check["ok"])
            self.assertIn("memory instructions", check["detail"])
            self.integration("connect")
        self.assertTrue(next(c for c in self.integration("doctor")["checks"] if c["id"] == "agent-athenaeum")["ok"])

    def test_capture_off_survives_reconnect_until_explicitly_enabled(self) -> None:
        self.integration("connect", "--capture", "off")
        self.integration("connect")
        self.assertNotIn("wiki_context", self.instructions.read_text(encoding="utf-8"))
        self.assertTrue(self.integration("doctor")["ok"])
        self.integration("connect", "--capture", "instruct")
        self.assertIn("wiki_context", self.instructions.read_text(encoding="utf-8"))

    def test_dry_run_and_malformed_instructions_do_not_change_settings(self) -> None:
        self.integration("connect", "--dry-run")
        self.assertEqual(self.instructions.read_text(encoding="utf-8"), "Keep my coding conventions.\n")
        self.assertEqual(json.loads(self.config.read_text(encoding="utf-8")), {"model": "user-model"})
        damaged = "My rules.\n<!-- athenaeum:begin (managed by `athenaeum setup`) -->\nunfinished\n"
        self.instructions.write_text(damaged, encoding="utf-8")
        self.assertFalse(self.integration("connect", check=False)["ok"])
        self.assertEqual(self.instructions.read_text(encoding="utf-8"), damaged)
        self.assertEqual(json.loads(self.config.read_text(encoding="utf-8")), {"model": "user-model"})

    def test_installed_agent_server_proposes_then_recalls_accepted_memory(self) -> None:
        self.setup_wiki()
        entry = json.loads(self.config.read_text(encoding="utf-8"))["mcpServers"]["athenaeum"]
        self.assertIn(str(self.project), entry["args"])
        # Local hosts need not launch their MCP server inside the selected wiki.
        nested = self.root / "unrelated coding folder"
        nested.mkdir()
        session = McpSession(entry, nested, self.env)
        try:
            initialized = session.start()
            self.assertIn("call wiki_context once", initialized["instructions"])
            self.assertIn("Do not narrate memory activity", initialized["instructions"])
            tools = {t["name"] for t in session.call("tools/list")["result"]["tools"]}
            self.assertTrue({"wiki_context", "wiki_search", "wiki_create"} <= tools)
            self.assertNotIn("wiki_accept", tools)
            empty = session.call("tools/call", {"name": "wiki_context", "arguments": {"task": "retry limit and shared quota"}})
            self.assertFalse(empty["result"].get("isError"), empty)
            self.assertEqual(empty["result"]["structuredContent"]["brief"]["nodes"], [])
            reply = session.call("tools/call", {"name": "wiki_create", "arguments": {
                "type": "decision", "title": "Retry limit protects the shared quota",
                "summary": "Use at most 17 retries because upstream counts each retry against the shared quota.", "scope": "project"}})
            self.assertFalse(reply["result"].get("isError"), reply)
        finally:
            session.close()
        proposals = json.loads(run(self.project, "proposals", "--json", env=self.env).stdout)
        self.assertEqual(len(proposals), 1)
        self.assertEqual(list((self.project / "wiki").rglob("*.md")), [])
        identifier = proposals[0].get("id") or proposals[0]["node"]["id"]
        run(self.project, "accept", identifier, "--as", "human-reviewer", env=self.env)
        self.assertTrue(list((self.project / "wiki").rglob("*.md")))
        later = McpSession(entry, nested, self.env)
        try:
            later.start()
            reply = later.call("tools/call", {"name": "wiki_context", "arguments": {"task": "implement retry limit and shared quota"}})
            self.assertFalse(reply["result"].get("isError"), reply)
            self.assertIn("17 retries", json.dumps(reply))
        finally:
            later.close()

    def test_selected_wiki_survives_reconnect_and_is_checked_from_elsewhere(self) -> None:
        self.integration("connect", "--workspace", str(self.project))
        self.assertEqual(self.integration("connect")["agents"][0]["status"], "unchanged")
        report = self.integration("doctor", "--cwd", str(self.root))
        check = next(c for c in report["checks"] if c["id"] == "agent-athenaeum")
        self.assertTrue(check["ok"], check)
        self.assertIn(str(self.project), check["detail"])
        (self.project / ".athenaeum").rename(self.project / "moved-wiki-state")
        report = self.integration("doctor", "--cwd", str(self.root), check=False)
        self.assertFalse(next(c for c in report["checks"] if c["id"] == "agent-athenaeum")["ok"])

    def test_registered_tools_without_a_wiki_are_not_memory_ready(self) -> None:
        self.integration("connect")
        report = self.integration("doctor", "--cwd", str(self.root), check=False)
        check = next(c for c in report["checks"] if c["id"] == "agent-athenaeum")
        self.assertFalse(check["ok"])
        self.assertIn("no wiki", check["detail"])

    def test_capture_off_applies_to_mcp_guidance_without_local_instructions(self) -> None:
        self.integration("connect", "--capture", "off", "--workspace", str(self.project))
        entry = json.loads(self.config.read_text(encoding="utf-8"))["mcpServers"]["athenaeum"]
        session = McpSession(entry, self.root, self.env)
        try:
            initialized = session.start()
            self.assertIn("Automatic memory is off", initialized["instructions"])
            self.assertNotIn("call wiki_context once", initialized["instructions"])
        finally:
            session.close()

    def test_doctor_detects_connection_to_a_different_selected_wiki(self) -> None:
        self.integration("connect", "--workspace", str(self.project))
        other = self.root / "other wiki"
        other.mkdir()
        run(other, "setup", "--yes", "--no-library", "--no-agents", "--skip-index", env=self.env)
        report = self.integration("doctor", "--cwd", str(other), check=False)
        check = next(c for c in report["checks"] if c["id"] == "agent-athenaeum")
        self.assertFalse(check["ok"])
        self.assertIn("not the selected wiki", check["detail"])


if __name__ == "__main__":
    unittest.main()
