"""Shared black-box harness for product tests that drive the real `athenaeum` executable.

The one local knowledge engine is the `athenaeum` binary built from the `athenaeum` repository. Set
ATHENAEUM_BIN to an already-built copy (ATHENAEUM_CORE_BIN is still read, for the old name). Every
call starts a fresh process, which is how separate agent sessions reach the engine. No model, no
network beyond loopback and no credentials are used.

Every child process is given UTF-8 explicitly: a Windows console code page must not be able to
corrupt a page, a title or a prompt.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import csv
import stat


def deny_read(path: Path):
    """Deny read access on an isolated fixture; return an idempotent restore function.

    chmod(0) does not deny reads on Windows. Exercise the real Windows ACL instead of
    skipping the permission-boundary check or reporting a readable file as unreadable.
    """
    if os.name == "nt":
        row = next(csv.reader(subprocess.check_output(
            ["whoami", "/user", "/fo", "csv", "/nh"], encoding="utf-8",
        ).splitlines()))
        sid = row[1]
        # Deny file data / directory listing while retaining permission to restore the ACL.
        subprocess.run(["icacls", str(path), "/deny", f"*{sid}:(RD)"], check=True, capture_output=True)
        return lambda: subprocess.run(["icacls", str(path), "/remove:d", f"*{sid}"], check=True, capture_output=True)
    mode = stat.S_IMODE(path.stat().st_mode)
    path.chmod(0)
    return lambda: path.chmod(mode)

BIN = os.environ.get("ATHENAEUM_BIN") or os.environ.get("ATHENAEUM_CORE_BIN")
SKIP_REASON = "set ATHENAEUM_BIN to a built `athenaeum` executable (cargo build -p athenaeum-cli)"
ENCODING = "utf-8"
CHILD_ENV = {"PYTHONUTF8": "1", "PYTHONIOENCODING": ENCODING}


def child_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    return {**os.environ, **CHILD_ENV, **(extra or {})}


def run(
    workspace: Path,
    *args: str,
    check: bool = True,
    env: dict[str, str] | None = None,
    timeout: int = 120,
) -> subprocess.CompletedProcess:
    """`athenaeum -C WORKSPACE ARGS...`, text mode, never through a shell."""
    assert BIN, SKIP_REASON
    process = subprocess.run(
        [BIN, "-C", str(workspace), *args],
        capture_output=True,
        encoding=ENCODING,
        errors="replace",
        timeout=timeout,
        env=child_env(env),
    )
    if check and process.returncode != 0:
        raise AssertionError(
            f"athenaeum {' '.join(args)} failed ({process.returncode}): {process.stdout}{process.stderr}"
        )
    return process


def run_json(workspace: Path, *args: str, **kwargs: object) -> object:
    process = run(workspace, *args, "--json", **kwargs)  # type: ignore[arg-type]
    return json.loads(process.stdout)


class Bridge:
    """The protocol v1 bridge: one JSON request on stdin, one JSON response on stdout."""

    def __init__(self, workspace: Path) -> None:
        self.workspace = workspace
        self.sequence = 0

    def _send(self, method: str, fields: dict[str, object]) -> tuple[subprocess.CompletedProcess, dict, str]:
        self.sequence += 1
        request_id = f"validation-{self.sequence}"
        envelope = {"protocolVersion": 1, "requestId": request_id, "request": {"method": method, **fields}}
        process, reply = self.send_text(json.dumps(envelope))
        echoed = reply.get("requestId")
        if echoed is not None and echoed != request_id:
            raise AssertionError(f"uncorrelated bridge response: {reply!r}")
        if echoed is None and reply.get("status") == "ok":
            raise AssertionError(f"a successful response must echo the requestId: {reply!r}")
        return process, reply, request_id

    def send(self, envelope: dict, request_id: str | None = None) -> tuple[subprocess.CompletedProcess, dict, str]:
        """Send one raw envelope; the reply is parsed but never judged. Uncoupled from helper shape."""
        assert BIN, SKIP_REASON
        expected = request_id if request_id is not None else str(envelope.get("requestId"))
        process, reply = self.send_text(json.dumps(envelope))
        if reply.get("requestId") != expected:
            raise AssertionError(f"uncorrelated bridge response: {reply!r}")
        if reply.get("protocolVersion") != 1:
            raise AssertionError(f"unexpected protocol version: {reply!r}")
        return process, reply, expected

    def send_text(self, payload: str) -> tuple[subprocess.CompletedProcess, dict]:
        """Send bytes that are not necessarily a well-formed envelope; parse whatever comes back."""
        assert BIN, SKIP_REASON
        process = subprocess.run(
            [BIN, "-C", str(self.workspace), "bridge"],
            input=payload,
            encoding=ENCODING,
            errors="replace",
            capture_output=True,
            check=False,
            timeout=120,
        )
        try:
            reply = json.loads(process.stdout)
        except json.JSONDecodeError as error:
            raise AssertionError(
                f"bridge wrote no JSON envelope ({error}); exit={process.returncode}; "
                f"stdout={process.stdout[:400]!r}; stderr={process.stderr[:400]!r}"
            ) from error
        return process, reply

    def raw(self, method: str, **fields: object) -> tuple[subprocess.CompletedProcess, dict]:
        """Send a request and return the envelope as it came, success or error."""
        process, reply, _ = self._send(method, fields)
        return process, reply

    def request(self, method: str, **fields: object) -> dict:
        process, reply, _ = self._send(method, fields)
        if process.returncode != 0 or reply.get("status") != "ok":
            raise AssertionError(
                f"bridge {method} failed: code={reply.get('error', {}).get('code')!r} "
                f"message={reply.get('error', {}).get('message')!r}; stderr={process.stderr.strip()[:400]}"
            )
        response = reply["response"]
        if response.get("method") != method:
            raise AssertionError(f"bridge returned wrong method: {response!r}")
        return response

    def fail(self, method: str, **fields: object) -> dict:
        """Send a request that must be refused, and return the protocol error body."""
        process, reply, _ = self._send(method, fields)
        if reply.get("status") != "error" or process.returncode == 0:
            raise AssertionError(f"bridge {method} should have been refused: {reply!r}")
        return reply["error"]
