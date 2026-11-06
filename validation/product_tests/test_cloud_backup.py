"""Upload and restore wiki backups against a disposable native API and account."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.request

from product_tests.harness import BIN, SKIP_REASON, run, run_json


@unittest.skipUnless(BIN and os.environ.get("ATHENAEUM_API_BIN"), SKIP_REASON + "; set ATHENAEUM_API_BIN too")
class CloudBackup(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="athenaeum-cloud-backup-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.wiki = self.root / "wiki-project"
        self.wiki.mkdir()
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        self.server = f"http://127.0.0.1:{port}"
        env = {**os.environ, "HOST": "127.0.0.1", "PORT": str(port),
               "API_DB_PATH": str(self.root / "library.db"), "API_REGISTRATION": "open",
               "API_SECURE_COOKIES": "false", "API_WEB_URL": self.server,
               "API_ALLOWED_ORIGINS": self.server}
        api = subprocess.Popen([os.environ["ATHENAEUM_API_BIN"]], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(lambda: (api.terminate(), api.wait(timeout=10)))
        for _ in range(100):
            try:
                with urllib.request.urlopen(self.server + "/health", timeout=1):
                    break
            except OSError:
                if api.poll() is not None:
                    self.fail("Disposable API exited during startup")
                time.sleep(0.1)
        else:
            self.fail("Disposable API did not become healthy")
        credentials = {"username": "backup-test", "password": "Disposable-test-password-123"}
        request = urllib.request.Request(self.server + "/api/v1/auth/register",
            data=json.dumps(credentials).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=10):
            pass
        self.env = {"ATHENAEUM_CONFIG_DIR": str(self.root / "credentials"),
                    "ATHENAEUM_LIBRARY_TOKEN": "", "ATHENAEUM_LIBRARY_URL": self.server}
        signed_in = subprocess.run([BIN, "library", "sign-in", "--server", self.server, "--stdin", "--json"],
            input=json.dumps(credentials), capture_output=True, text=True,
            env={**os.environ, **self.env}, timeout=30)
        self.assertEqual(signed_in.returncode, 0, signed_in.stderr)
        run(self.wiki, "init", env=self.env)

    def command(self, *args, **kwargs):
        return run_json(self.wiki, *args, env=self.env, **kwargs)

    def test_private_backup_retry_restore_and_conflicts(self):
        files = {"README.md": "# My wiki\n", "purpose.md": "# Purpose\n",
                 "schema.md": "# Schema\n", "wiki/topics/a.md": "# A\n"}
        for path, body in files.items():
            file = self.wiki / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(body, encoding="utf-8")
        raw = self.wiki / "raw/sources/private.md"
        raw.parent.mkdir(parents=True)
        raw.write_text("This stays local", encoding="utf-8")
        self.command("sync", "backup", "--server", self.server)
        state = json.loads((self.wiki / ".athenaeum/sync/state.json").read_text())
        repo = self.command("api", "get", "repositories/" + state["remoteId"])["body"]
        self.assertEqual(repo["visibility"], "private")
        self.assertEqual(self.command("sync", "status")["position"], "up_to_date")
        self.assertTrue(self.command("sync", "backup")["push"]["nothingToPush"])
        restored = self.root / "restored"
        self.command("sync", "clone", state["remoteId"], str(restored), "--server", self.server)
        for path, body in files.items():
            self.assertEqual((restored / path).read_text(encoding="utf-8"), body)
        self.assertFalse((restored / "raw").exists())
        self.assertFalse((restored / ".athenaeum/automation.json").exists())
        (restored / "README.md").write_text("# Remote edit\n", encoding="utf-8")
        run(restored, "sync", "push", env=self.env)
        (self.wiki / "README.md").write_text("# Local edit\n", encoding="utf-8")
        outcome = run(self.wiki, "sync", "backup", "--json", env=self.env, check=False)
        self.assertNotEqual(outcome.returncode, 0)
        self.assertEqual((self.wiki / "README.md").read_text(), "# Local edit\n")
        self.assertIn("README.md", self.command("sync", "status")["conflicts"])

    def test_explicit_pause_and_disconnect_do_not_relink(self):
        self.command("automation", "--auto-sync", "false")
        self.assertEqual(self.command("sync", "backup", "--automatic")["status"], "paused")
        self.assertFalse((self.wiki / ".athenaeum/sync/state.json").exists())
        self.command("sync", "backup")
        self.command("sync", "unlink")
        self.assertEqual(self.command("sync", "backup", "--automatic")["status"], "paused")
        self.assertFalse((self.wiki / ".athenaeum/sync/state.json").exists())

    def test_automatic_backup_initializes_a_new_wiki(self):
        self.command("sync", "backup", "--automatic", "--server", self.server)
        self.assertEqual(self.command("sync", "status")["position"], "up_to_date")
        self.assertTrue(self.command("automation")["settings"]["autoSync"])

    def test_oversize_document_is_not_reported_as_a_complete_backup(self):
        (self.wiki / "README.md").write_text("x" * (1024 * 1024 + 1), encoding="utf-8")
        result = run(self.wiki, "sync", "backup", "--json", env=self.env, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)["error"]["code"], "backup_incomplete")
        self.assertIn("README.md", result.stdout)

