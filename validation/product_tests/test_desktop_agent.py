"""Exercise the desktop/native protocol seam without hosted model credentials."""
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DRIVER = Path(__file__).resolve().parents[2] / "src-tauri/resources/native-agent-driver.mjs"
NATIVE = Path(os.environ.get("ATHENAEUM_NATIVE_AGENT_BIN") or
    Path(__file__).resolve().parents[2] / "target/native-runtime/debug" /
    ("athenaeum-agent.exe" if os.name == "nt" else "athenaeum-agent"))
NODE = shutil.which("node")

FIXTURE = r'''
const input = require('node:readline').createInterface({ input: process.stdin });
const send = (message) => process.stdout.write(JSON.stringify(message) + '\n');
let thread = 0;
input.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.method === 'initialize') send({id:m.id,result:{}});
  if (m.method === 'thread/start') send({id:m.id,result:{thread:{id:'thread-'+(++thread)}}});
  if (m.method === 'turn/start') {
    send({id:m.id,result:{turn:{id:'turn-1'}}});
    send({id:99,method:'item/commandExecution/requestApproval',params:{threadId:m.params.threadId,command:'echo approved'}});
  }
  if (m.id === 99 && m.result) {
    send({method:'item/completed',params:{item:{id:'final',type:'agentMessage',text:m.result.decision}}});
    send({method:'turn/completed',params:{turn:{id:'turn-1',status:'completed'}}});
  }
  if (m.method === 'turn/interrupt') {
    send({id:m.id,result:{}});
    send({method:'turn/completed',params:{turn:{id:'turn-1',status:'interrupted'}}});
  }
});
'''


@unittest.skipUnless(NODE and DRIVER.exists(), "Node and the desktop adapter are required")
class DesktopAgent(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="athenaeum-desktop-agent-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "app-server").write_text(FIXTURE, encoding="utf-8")

    def start(self, binary=NODE, extra_env=None):
        process = subprocess.Popen([NODE, str(DRIVER), str(binary)], cwd=self.root,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", env={**os.environ, **(extra_env or {})})
        messages = queue.Queue()
        diagnostics = []
        def read():
            for line in process.stdout:
                messages.put(json.loads(line))
        threading.Thread(target=read, daemon=True).start()
        threading.Thread(target=lambda: diagnostics.extend(process.stderr), daemon=True).start()
        def stop():
            process.stdin.close()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
            process.stdout.close(); process.stderr.close()
        self.addCleanup(stop)
        return process, messages

    def send(self, process, message):
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()

    def until(self, messages, predicate):
        while True:
            message = messages.get(timeout=30)
            if predicate(message): return message

    def run_approval(self, decision):
        process, messages = self.start()
        self.send(process, {"type":"run", "requestId":"run", "runId":"run",
            "projectPath":str(self.root), "sessionId":"conversation", "contentBlocks":[{"type":"text","text":"hello"}]})
        approval = self.until(messages, lambda m: m.get("notification",{}).get("params",{}).get("event",{}).get("type") == "approval/request")
        self.assertEqual(approval["notification"]["params"]["event"]["data"]["id"], "99")
        self.send(process, {"type":"approval", "requestId":"allow", "approvalId":"99", "decision":decision})
        result = self.until(messages, lambda m: m.get("kind") == "result")
        self.assertEqual(result["result"]["finalResponse"], decision)

    def test_approval_can_be_allowed_during_a_running_turn(self): self.run_approval("accept")
    def test_approval_can_be_denied_during_a_running_turn(self): self.run_approval("decline")

    def test_shutdown_exits_without_waiting_for_the_parent_to_close_stdin(self):
        process, messages = self.start()
        self.send(process, {"type": "check", "requestId": "check", "projectPath": str(self.root)})
        self.assertEqual(self.until(messages, lambda m: m.get("requestId") == "check")["kind"], "ready")
        self.send(process, {"type": "shutdown", "requestId": "shutdown"})
        self.until(messages, lambda m: m.get("kind") == "shutdown")
        self.assertEqual(process.wait(timeout=4), 0)

    def test_shutdown_terminates_a_runtime_that_ignores_eof_and_sigterm(self):
        stubborn = self.root / "stubborn.mjs"
        stubborn.write_text("process.stdin.resume(); process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);\n", encoding="utf-8")
        process, messages = self.start(stubborn)
        self.send(process, {"type": "shutdown", "requestId": "shutdown"})
        self.until(messages, lambda m: m.get("kind") == "shutdown")
        self.assertEqual(process.wait(timeout=4), 0)

    def test_stop_interrupts_a_turn_waiting_for_approval(self):
        process, messages = self.start()
        self.send(process, {"type":"run", "requestId":"run", "runId":"run", "projectPath":str(self.root),
                            "sessionId":"conversation", "contentBlocks":[{"type":"text","text":"hello"}]})
        self.until(messages, lambda m: m.get("notification",{}).get("params",{}).get("event",{}).get("type") == "approval/request")
        self.send(process, {"type":"cancel", "requestId":"stop", "runId":"run"})
        self.assertEqual(self.until(messages, lambda m: m.get("kind") == "cancelled")["runId"], "run")

    @unittest.skipUnless(NATIVE.exists(), "Build the native runtime first")
    def test_real_native_service_initializes_with_separate_home(self):
        home = self.root / "native-home"
        home.mkdir()
        process, messages = self.start(NATIVE, {"ATHENAEUM_AGENT_HOME":str(home)})
        self.send(process, {"type":"check", "requestId":"check", "projectPath":str(self.root)})
        result = self.until(messages, lambda m: m.get("requestId") == "check")
        self.assertEqual(result["kind"], "ready", result)
        self.assertEqual(result["provider"], "Athenaeum Agent")

    @unittest.skipUnless(NATIVE.exists(), "Build the native runtime first")
    def test_real_native_coding_tool_turn_through_desktop_adapter(self):
        home = self.root / "native-home"
        home.mkdir()
        requests = []
        outputs = [
            {"type":"function_call", "call_id":"patch-1", "name":"apply_patch",
             "arguments":json.dumps({"patch":"*** Begin Patch\n*** Add File: desktop-tool.txt\n+desktop-native-ok\n*** End Patch"})},
            {"type":"message", "id":"final-1", "role":"assistant",
             "content":[{"type":"output_text","text":"Desktop native tools completed."}]},
        ]
        class Provider(BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def do_POST(self):
                requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
                index = len(requests) - 1
                if index >= len(outputs): self.send_error(500); return
                events = [
                    {"type":"response.created","response":{"id":f"response-{index}"}},
                    {"type":"response.output_item.done","item":outputs[index]},
                    {"type":"response.completed","response":{"id":f"response-{index}","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}},
                ]
                data = "".join("data: " + json.dumps(e) + "\n\n" for e in events).encode()
                self.send_response(200); self.send_header("Content-Type","text/event-stream")
                self.send_header("Content-Length",str(len(data))); self.end_headers(); self.wfile.write(data)
        server = ThreadingHTTPServer(("127.0.0.1",0), Provider)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        (home / "config.json").write_text(json.dumps({"model": "test-model", "api": "responses",
                "baseURL": f"http://127.0.0.1:{server.server_port}/v1"}), encoding="utf-8")
        process, messages = self.start(NATIVE, {"ATHENAEUM_AGENT_HOME":str(home),
            "ATHENAEUM_BIN":str(self.root / "missing-wiki")})
        self.send(process, {"type":"run","requestId":"run","runId":"run", "projectPath":str(self.root),
            "sessionId":"conversation","contentBlocks":[{"type":"text","text":"Create desktop-tool.txt and report completion."}]})
        while True:
            message = messages.get(timeout=60)
            event = message.get("notification",{}).get("params",{}).get("event",{})
            if event.get("type") == "approval/request":
                self.send(process, {"type":"approval","requestId":"approve","approvalId":event["data"]["id"],"decision":"accept"})
            if message.get("kind") in ("error", "result"):
                self.assertEqual(message["kind"], "result", message)
                self.assertEqual(message["result"]["finalResponse"], "Desktop native tools completed.")
                break
        self.assertEqual((self.root / "desktop-tool.txt").read_text().strip(), "desktop-native-ok")
        self.assertEqual(len(requests), 2)
