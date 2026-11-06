"""Exercise the source-built native executable with a local scripted provider, no account."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import unittest


BIN = os.environ.get("ATHENAEUM_NATIVE_AGENT_BIN")


@unittest.skipUnless(BIN and Path(BIN).is_file(), "build the native runtime and set ATHENAEUM_NATIVE_AGENT_BIN")
class NativeAgent(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="athenaeum-native-Ω-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.workspace = self.root / "project"
        self.workspace.mkdir()
        self.env = {**os.environ, "ATHENAEUM_AGENT_HOME": str(self.home)}
        self.env.pop("OPENAI_API_KEY", None)

    def invoke(self, *args):
        result = subprocess.run([BIN, *args], cwd=self.workspace, env=self.env,
                                capture_output=True, text=True, encoding="utf-8", timeout=90)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def test_native_help_and_headless_entry_point(self):
        self.assertIn("athenaeum-agent", self.invoke("--help").stdout)
        self.assertIn("--json", self.invoke("exec", "--help").stdout)
        self.assertIn("app-server", self.invoke("app-server", "--help").stdout)

    def test_app_server_initializes_in_native_home(self):
        process = subprocess.Popen([BIN, "app-server"], cwd=self.workspace, env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, encoding="utf-8")
        messages = queue.Queue()
        def read():
            for line in process.stdout:
                messages.put(line)
            messages.put(None)
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        try:
            request = {"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "athenaeum-native-validation", "version": "0.1.0"},
                "capabilities": {"experimentalApi": True},
            }}
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            while True:
                line = messages.get(timeout=30)
                self.assertIsNotNone(line, "app-server exited before initialize")
                response = json.loads(line)
                if response.get("id") == 1:
                    break
            self.assertNotIn("error", response)
            self.assertIn("result", response)
            # Protocol field names stay compatible; the resolved directory is native.
            self.assertEqual(Path(response["result"]["agentHome"]).resolve(), self.home.resolve())
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
            process.stdout.close()
            process.stderr.close()
            reader.join(timeout=5)

    def scripted_provider(self, outputs, requests):
        class Provider(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                index = len(requests)
                requests.append(body)
                if index >= len(outputs):
                    self.send_error(500, "Unexpected additional model round")
                    return
                response_id = f"response-{index}"
                events = [
                    {"type": "response.created", "response": {"id": response_id}},
                    {"type": "response.output_item.done", "item": outputs[index]},
                    {"type": "response.completed", "response": {"id": response_id,
                        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}},
                ]
                data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(lambda: thread.join(timeout=5))
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server

    def test_real_loop_patches_and_reads_a_file_through_coding_tools(self):
        requests = []
        patch = "*** Begin Patch\n*** Add File: native-smoke.txt\n+native-runtime-ok\n*** End Patch"
        outputs = [
            {"type": "function_call", "call_id": "patch-1", "name": "apply_patch", "arguments": json.dumps({"patch": patch})},
            {"type": "function_call", "call_id": "shell-1", "name": "exec_command",
             "arguments": json.dumps({"cmd": "type native-smoke.txt" if os.name == "nt" else "cat native-smoke.txt",
                                      "login": False,
                                      "yield_time_ms": 1000, "max_output_tokens": 200})},
            {"type": "message", "id": "final-1", "role": "assistant",
             "content": [{"type": "output_text", "text": "Native coding tools completed."}]},
        ]

        server = self.scripted_provider(outputs, requests)
        try:
            # Exercise the native default MCP entry and its optional/missing-wiki path.
            self.env["ATHENAEUM_BIN"] = str(self.root / "not-installed-wiki")
            (self.home / "config.json").write_text(json.dumps({"model": "test-model", "api": "responses",
                "baseURL": f"http://127.0.0.1:{server.server_port}/v1"}), encoding="utf-8")
            # The scripted tool calls operate on the disposable test workspace.
            result = self.invoke("exec", "--json", "--ephemeral", "--yes",
                                 "Create native-smoke.txt, read it with the shell, then report completion.")
            self.assertTrue((self.workspace / "native-smoke.txt").is_file(), result.stdout + result.stderr)
            self.assertEqual((self.workspace / "native-smoke.txt").read_text(encoding="utf-8").strip(), "native-runtime-ok")
            self.assertEqual(len(requests), 3)
            self.assertIn("native-runtime-ok", json.dumps(requests[-1]))
            self.assertIn("Native coding tools completed.", result.stdout)
            self.assertIn("You are Athenaeum", json.dumps(requests[0]))
            self.assertNotIn("wiki_context", json.dumps(requests[0].get("tools", [])))
        finally:
            server.shutdown()

    def test_owned_loop_uses_real_wiki_tools_and_only_queues_a_proposal(self):
        wiki = Path(os.environ.get("ATHENAEUM_BIN") or
            Path(__file__).resolve().parents[2] / "target/debug" /
            ("wiki.exe" if os.name == "nt" else "wiki"))
        self.assertTrue(wiki.is_file(), "Build the wiki CLI before native smoke tests")
        initialized = subprocess.run([str(wiki), "-C", str(self.workspace), "init"],
            env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(initialized.returncode, 0, initialized.stdout + initialized.stderr)
        self.env["ATHENAEUM_BIN"] = str(wiki)
        requests = []
        outputs = [
            {"type": "function_call", "call_id": "context-1", "name": "wiki_context",
             "arguments": json.dumps({"task": "bounded retry convention"})},
            {"type": "function_call", "call_id": "proposal-1", "name": "wiki_create",
             "arguments": json.dumps({"type": "decision", "title": "Bound retry work",
                 "summary": "Use a bounded retry budget to protect the shared quota.", "scope": "project"})},
            {"type": "message", "id": "final-1", "role": "assistant",
             "content": [{"type": "output_text", "text": "Finding proposed for review."}]},
        ]
        server = self.scripted_provider(outputs, requests)
        (self.home / "config.json").write_text(json.dumps({"model": "test-model", "api": "responses",
            "baseURL": f"http://127.0.0.1:{server.server_port}/v1"}), encoding="utf-8")
        result = self.invoke("exec", "--json", "--ephemeral", "Propose the bounded retry convention.")
        self.assertIn("Finding proposed for review.", result.stdout)
        tools = {tool["name"] for tool in requests[0]["tools"]}
        self.assertTrue({"wiki_context", "wiki_create"} <= tools)
        self.assertNotIn("wiki_accept", tools)
        proposals = subprocess.run([str(wiki), "-C", str(self.workspace), "proposals", "--json"],
            env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(proposals.returncode, 0, proposals.stdout + proposals.stderr)
        self.assertEqual(len(json.loads(proposals.stdout)), 1)
        self.assertEqual(list((self.workspace / "wiki").rglob("*.md")), [])


if __name__ == "__main__":
    unittest.main()
