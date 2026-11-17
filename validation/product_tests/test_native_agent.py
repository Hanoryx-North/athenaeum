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

    def test_automatic_recall_reaches_the_model_quietly_and_inspection_survives_restart(self):
        wiki = Path(os.environ.get("ATHENAEUM_BIN") or
            Path(__file__).resolve().parents[2] / "target/debug" /
            ("wiki.exe" if os.name == "nt" else "wiki"))
        self.assertTrue(wiki.is_file(), "Build the wiki CLI before native smoke tests")

        def knowledge(*args):
            result = subprocess.run([str(wiki), "-C", str(self.workspace), *args, "--json"],
                env=self.env, capture_output=True, text=True, encoding="utf-8", timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout)

        knowledge("init")
        proposed = knowledge("create", "Retry budget is three", "--type", "decision",
            "--summary", "Retry budget is three to prevent outage amplification.")
        # Simulate a separate person reviewing the disposable fixture, never an
        # agent accepting a proposal as part of its turn.
        node = proposed.get("node", proposed)
        knowledge("accept", node["id"], "--as", "test-human-reviewer")
        self.env["ATHENAEUM_BIN"] = str(wiki)
        requests = []
        outputs = [{"type": "message", "id": "result", "role": "assistant",
                    "content": [{"type": "output_text", "text": "Ready to fix retries."}]}]
        server = self.scripted_provider(outputs, requests)
        (self.home / "config.json").write_text(json.dumps({"model": "test-model", "api": "responses",
            "baseURL": f"http://127.0.0.1:{server.server_port}/v1"}), encoding="utf-8")
        result = self.invoke("exec", "--json", "--session", "quiet-recall", "Fix the retry budget.")
        self.assertEqual(len(requests), 1, "Recall must not make an additional model call")
        call = next(item for item in requests[0]["input"] if item.get("name") == "wiki_context")
        recalled = next(item for item in requests[0]["input"] if item.get("type") == "function_call_output"
                        and item["call_id"] == call["call_id"])
        self.assertIn("Retry budget is three", recalled["output"])
        self.assertIn("reference", recalled["output"].lower())
        self.assertNotIn("wiki_context", result.stdout)
        self.assertNotIn("Retry budget is three", result.stdout)
        inspected = json.loads(self.invoke("inspect", "--session", "quiet-recall", "--json").stdout)
        self.assertEqual(inspected["memory"]["status"], "recalled")
        self.assertGreater(inspected["memory"]["nodes"], 0)
        self.assertNotIn("history", inspected)

    def test_background_learning_uses_its_model_and_only_queues_evidenced_knowledge(self):
        wiki = Path(os.environ.get("ATHENAEUM_BIN") or
            Path(__file__).resolve().parents[2] / "target/debug" /
            ("wiki.exe" if os.name == "nt" else "wiki"))
        self.assertTrue(wiki.is_file())
        self.env["ATHENAEUM_BIN"] = str(wiki)
        initialized = subprocess.run([str(wiki), "-C", str(self.workspace), "init"],
            env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(initialized.returncode, 0, initialized.stdout + initialized.stderr)
        (self.workspace / "retry.mjs").write_text("export const retryBudget = 3\n", encoding="utf-8")
        candidate = {"type": "gotcha", "title": "Bound retries to protect the shared quota",
            "summary": "The retry budget is three to prevent outage amplification.",
            "lesson": {"recommendation": "Keep retry work bounded to three attempts.",
                "rationale": "Unbounded retries amplify outages and exhaust the shared quota.",
                "whenToUse": "When modifying this project's retry policy.",
                "validation": "The source constant was inspected; outage behavior has not been tested.",
                "limits": "The constant alone does not prove concurrent outage behavior."},
            "evidence": [{"path": "retry.mjs", "excerpt": "export const retryBudget = 3"}]}
        outputs = [
            {"type": "function_call", "call_id": "read", "name": "read_file", "arguments": '{"path":"retry.mjs"}'},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Retry investigation completed."}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": json.dumps({"candidates": [candidate]})}]},
        ]
        requests = []
        server = self.scripted_provider(outputs, requests)
        (self.home / "config.json").write_text(json.dumps({"model": "coding-model", "api": "responses",
            "baseURL": f"http://127.0.0.1:{server.server_port}/v1",
            "backgroundMemory": {"enabled": True, "model": "cheap-model"}}), encoding="utf-8")
        result = self.invoke("exec", "--json", "--session", "learning", "Read retry.mjs and investigate the retry budget.")
        self.assertEqual([request["model"] for request in requests], ["coding-model", "coding-model", "cheap-model"])
        self.assertNotIn(candidate["title"], result.stdout)
        self.assertNotIn("candidates", result.stdout)
        proposals = subprocess.run([str(wiki), "-C", str(self.workspace), "proposals", "--json"],
            env=self.env, capture_output=True, text=True, encoding="utf-8", timeout=30)
        self.assertEqual(proposals.returncode, 0, proposals.stdout + proposals.stderr)
        queued = json.loads(proposals.stdout)
        self.assertEqual(len(queued), 1)
        self.assertEqual(queued[0]["node"]["title"], candidate["title"])
        self.assertIn("retry.mjs", json.dumps(queued[0]))
        self.assertEqual(list((self.workspace / "wiki").rglob("*.md")), [])
        jobs = json.loads(self.invoke("memory").stdout)["jobs"]
        self.assertEqual(jobs[0]["status"], "completed")
        self.assertEqual(len(jobs[0]["proposed"]), 1)
        self.assertNotIn("history", jobs[0])

    def test_text_intake_reaches_real_engine_with_session_capture_disabled(self):
        wiki = Path(os.environ["ATHENAEUM_BIN"])
        self.env["ATHENAEUM_BIN"] = str(wiki)
        initialized = subprocess.run([str(wiki), "-C", str(self.workspace), "init"],
            env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(initialized.returncode, 0, initialized.stdout + initialized.stderr)
        candidate = {"type": "procedure", "title": "Preserve command failures during validation",
            "summary": "Check every pipeline stage so a successful display command cannot conceal a failed compiler.",
            "lesson": {"recommendation": "Preserve failing exit codes through command pipelines.",
                "rationale": "A successful output filter can hide the compiler's failure.",
                "whenToUse": "When writing shell-based validation or agent tooling.",
                "validation": "Reproduce a failing first stage followed by a successful output filter.",
                "limits": "This is reference advice; it has not been verified in this project."},
            "evidence": [{"source": "text_1_1"}]}
        requests = []
        server = self.scripted_provider([{"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": json.dumps({"candidates": [candidate]})}]}], requests)
        (self.home / "config.json").write_text(json.dumps({"capture": "off", "model": "coding-model",
            "api": "responses", "baseURL": f"http://127.0.0.1:{server.server_port}/v1",
            "backgroundMemory": {"enabled": False, "includeCodex": False,
                "model": "memory-model", "wikiRoot": str(self.workspace)}}), encoding="utf-8")
        process = subprocess.Popen([BIN, "memory-server"], cwd=self.workspace, env=self.env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        messages = queue.Queue()

        def read():
            for line in process.stdout:
                messages.put(json.loads(line))
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        serial = 0

        def request(action, **params):
            nonlocal serial
            serial += 1
            process.stdin.write(json.dumps({"type": "memory", "requestId": str(serial), "action": action, **params}) + "\n")
            process.stdin.flush()
            response = messages.get(timeout=30)
            self.assertEqual(response["requestId"], str(serial))
            self.assertEqual(response["kind"], "result", response)
            return response["result"]
        try:
            queued = request("ingest", document={"title": "Tooling friction talk", "scope": "universal",
                "wikiRoot": str(self.workspace), "text": "Shell pipelines can hide failed compiler exit codes behind successful output filters. Preserve the real failure and add a regression test."})
            self.assertEqual(queued["status"], "queued")
            status = request("run")
            if status["jobs"][0]["status"] == "pending":
                status = request("run")
            self.assertEqual(status["jobs"][0]["status"], "completed", status)
            self.assertFalse(status["active"])
            self.assertEqual([item["model"] for item in requests], ["memory-model"])
            proposals = subprocess.run([str(wiki), "-C", str(self.workspace), "proposals", "--json"],
                env=self.env, capture_output=True, text=True, encoding="utf-8", timeout=30)
            self.assertEqual(proposals.returncode, 0, proposals.stdout + proposals.stderr)
            findings = json.loads(proposals.stdout)
            self.assertEqual(len(findings), 1)
            self.assertEqual(findings[0]["node"]["scope"], "universal")
            self.assertIn("## Why it matters", findings[0]["node"]["content"])
            self.assertNotIn("Sources:", findings[0]["node"]["content"])
            self.assertEqual(list((self.workspace / "wiki").rglob("*.md")), [])
            saved_job = json.loads(next((self.home / "memory-jobs").glob("*.json")).read_text(encoding="utf-8"))
            self.assertNotIn("document", saved_job)
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait(timeout=10)
            process.stdout.close(); process.stderr.close(); reader.join(timeout=5)


if __name__ == "__main__":
    unittest.main()
