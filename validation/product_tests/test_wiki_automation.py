"""Real CLI/bridge checks for automatic page saving and portable relationships."""
import json
from pathlib import Path
import tempfile
import unittest
import time

from product_tests.harness import BIN, SKIP_REASON, Bridge, run, run_json


@unittest.skipUnless(BIN, SKIP_REASON)
class WikiAutomation(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="athenaeum-automation-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        run(self.root, "init")
        self.bridge = Bridge(self.root)

    def propose(self, title):
        return self.bridge.request("create", input={"type": "decision", "title": title,
            "summary": title, "content": "A durable decision.", "scope": "project"},
            caller={"model": "validation"})["node"]["id"]

    def test_defaults_and_settings_survive_another_process(self):
        settings = run_json(self.root, "automation")["settings"]
        self.assertEqual(settings, {"acceptProposals": True, "autoSync": False})
        run(self.root, "automation", "--accept-proposals", "true", "--auto-sync", "true")
        self.assertEqual(run_json(self.root, "automation")["settings"],
                         {"acceptProposals": True, "autoSync": True})

    def test_accept_all_preserves_links_regardless_of_order_and_rebuild(self):
        first = self.propose("First proposal")
        second = self.propose("Second proposal")
        self.bridge.request("link", input={"from": first, "to": second, "type": "depends_on"}, caller={"model": "validation"})
        batch = run_json(self.root, "automation", "--accept-all")["batch"]
        self.assertEqual(len(batch["accepted"]), 2)
        self.assertEqual(batch["errors"], [])
        self.assertEqual(self.bridge.request("proposals")["proposals"], [])
        for accepted in batch["accepted"]:
            self.assertTrue((self.root / accepted["page"]["path"]).is_file())
            self.assertEqual(accepted["node"]["authority"], "unverified_observation")
        database = self.root / ".athenaeum/knowledge.db"
        for suffix in ("", "-wal", "-shm"):
            database.with_name(database.name + suffix).unlink(missing_ok=True)
        run(self.root, "index-vault")
        detail = self.bridge.request("open", node=first)
        self.assertTrue(any(edge["id"] == second for edge in detail["detail"]["relationships"]))
        self.assertEqual(run_json(self.root, "automation", "--accept-all")["batch"]["accepted"], [])

    def test_agent_can_link_saved_pages_and_reindex_keeps_the_link(self):
        first, second = self.propose("Saved source"), self.propose("Saved target")
        run(self.root, "automation", "--accept-all")
        self.bridge.request("link", input={"from": first, "to": second, "type": "related_to"}, caller={"model": "validation"})
        run(self.root, "index-vault")
        self.assertTrue(any(edge["id"] == second for edge in self.bridge.request("open", node=first)["detail"]["relationships"]))

    def test_default_policy_saves_settled_proposals_on_next_recall(self):
        first = self.propose("Settled automatic capture")
        self.assertEqual(self.bridge.request("proposals")["proposals"][0]["node"]["id"], first)
        time.sleep(16)
        self.bridge.request("context", task="Settled automatic capture")
        self.assertEqual(self.bridge.request("proposals")["proposals"], [])
        self.assertTrue(self.bridge.request("open", node=first)["detail"]["page"]["path"].endswith(".md"))
