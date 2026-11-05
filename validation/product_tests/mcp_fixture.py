"""Real standalone integration bundle and minimal stdio MCP test client."""
import json
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "install" / "install.mjs"
INTEGRATIONS = ROOT / "packages/integrations"
NODE = shutil.which("node")


def build_bundle() -> Path:
    """The same file the release ships: `npm run bundle` in packages/integrations."""
    if not (ROOT / "node_modules").is_dir():
        subprocess.run([shutil.which("npm"), "install", "--no-audit", "--no-fund"], cwd=ROOT, check=True, capture_output=True)
    subprocess.run([NODE, "scripts/bundle.mjs"], cwd=INTEGRATIONS, check=True, capture_output=True)
    bundle = INTEGRATIONS / "dist" / "athenaeum-integrations.mjs"
    assert bundle.is_file(), "the bundle was not written"
    return bundle


def paths_in(value: object) -> set[str]:
    """Every file path a revision manifest names, wherever it nests them."""
    found: set[str] = set()
    if isinstance(value, dict):
        for key, item in value.items():
            if key == "path" and isinstance(item, str):
                found.add(item)
            else:
                found |= paths_in(item)
    elif isinstance(value, list):
        for item in value:
            found |= paths_in(item)
    return found


class McpSession:
    """A minimal MCP client: newline-delimited JSON-RPC over the server's stdio."""

    def __init__(self, entry: dict, cwd: Path, env: dict[str, str]) -> None:
        self.process = subprocess.Popen(
            [entry["command"], *entry["args"]], cwd=cwd, env=env, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
        )
        self.replies: queue.Queue = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.next_id = 0

    def _read(self) -> None:
        for line in self.process.stdout:  # type: ignore[union-attr]
            try:
                self.replies.put(json.loads(line))
            except json.JSONDecodeError:
                pass

    def call(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        self._write({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}})
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                reply = self.replies.get(timeout=1)
            except queue.Empty:
                if self.process.poll() is not None:
                    raise AssertionError(f"MCP server exited: {self.process.stderr.read()}")  # type: ignore[union-attr]
                continue
            if reply.get("id") == self.next_id:
                return reply
        raise AssertionError(f"no answer to {method}")

    def _write(self, message: dict) -> None:
        self.process.stdin.write(json.dumps(message) + "\n")  # type: ignore[union-attr]
        self.process.stdin.flush()  # type: ignore[union-attr]

    def start(self) -> dict:
        reply = self.call("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "validation", "version": "0"}})
        assert "result" in reply, reply
        self._write({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return reply["result"]

    def close(self) -> None:
        self.process.kill()
        self.process.wait(timeout=10)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream:
                stream.close()


