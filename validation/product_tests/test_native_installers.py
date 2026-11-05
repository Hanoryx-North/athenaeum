"""Native setup resources, exercised with real binaries in an isolated home.

Set ATHENAEUM_NATIVE_TOOLS to the directory written by package-tools.mjs.
The base install is denied Node network access; the optional agent smoke test
uses npm and never sends a model request. No real agent settings are changed.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOLS = os.environ.get("ATHENAEUM_NATIVE_TOOLS")


class NativePackaging(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not TOOLS or not Path(TOOLS).is_dir():
            raise unittest.SkipTest("set ATHENAEUM_NATIVE_TOOLS to the architecture-matched src-tauri/tools directory produced by package-tools.mjs")
        cls.work = tempfile.TemporaryDirectory(prefix="athenaeum native install ")
        cls.root = Path(cls.work.name)
        cls.resources = cls.root / "resources"
        shutil.copytree(ROOT / "install", cls.resources / "install")
        # Shared read-only build artifacts; only the isolated home gets installed files.
        (cls.resources / "tools").symlink_to(Path(TOOLS).resolve(), target_is_directory=True)
        cls.node = Path(TOOLS) / "node" / ("node.exe" if sys.platform == "win32" else "bin/node")

    @classmethod
    def tearDownClass(cls):
        if hasattr(cls, "work"):
            if os.name == "nt" and cls.root.exists():
                # Installed Harness assets exceed MAX_PATH under a test home with spaces.
                # rmtree does not traverse directory symlinks/junctions on Python 3.8+.
                target = cls.root.resolve()
                if target.parent != Path(tempfile.gettempdir()).resolve() or not target.name.startswith("athenaeum native install "):
                    raise RuntimeError(f"Refusing to clean a non-fixture path: {target}")
                shutil.rmtree("\\\\?\\" + str(target))
            cls.work.cleanup()

    def env_for(self, name):
        home = self.root / name
        home.mkdir()
        env = dict(os.environ)
        # Keep all config, caches, profiles, credentials and launchers in the fixture.
        for key in list(env):
            if key.startswith(("ATHENAEUM_", "DSH_", "CODEX_", "CLAUDE_")):
                env.pop(key)
        env.update(HOME=str(home), USERPROFILE=str(home), LOCALAPPDATA=str(home / "AppData/Local"),
                   XDG_DATA_HOME=str(home / ".local/share"), XDG_CONFIG_HOME=str(home / ".config"),
                   XDG_CACHE_HOME=str(home / ".cache"), SHELL="/bin/zsh",
                   CODEX_HOME=str(home / ".codex"), CLAUDE_CONFIG_DIR=str(home / ".claude"))
        if sys.platform == "darwin":
            prefix = home / "Library/Application Support/Athenaeum"
        elif sys.platform == "win32":
            prefix = home / "AppData/Local/Athenaeum"
        else:
            prefix = home / ".local/share/athenaeum"
        return home, prefix, env

    def invoke(self, command, env):
        return subprocess.run(list(map(str, command)), env=env, capture_output=True, text=True, timeout=600)

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stdout[-6000:] + result.stderr[-6000:])

    def test_agent_archive_contains_its_protocol_dependency(self):
        with tarfile.open(Path(TOOLS) / "agent/athenaeum-agent.tgz") as archive:
            names = archive.getnames()
            self.assertIn("package/node_modules/@athenaeum/protocol/src/bridge.js", names)
            package = json.load(archive.extractfile("package/package.json"))
            self.assertFalse(package["dependencies"]["@athenaeum/protocol"].startswith("file:"))
            self.assertIn("@athenaeum/protocol", package["bundleDependencies"])

    def test_base_install_and_update_work_offline_without_touching_agent_settings(self):
        home, prefix, env = self.env_for("offline home")
        deny = self.root / "deny-network.mjs"
        deny.write_text("import http from 'node:http'; import https from 'node:https'; import net from 'node:net';\n"
                        "const deny = () => { throw Error('Network forbidden in native base install') };\n"
                        "globalThis.fetch = deny; http.request = deny; https.request = deny; net.connect = deny; net.createConnection = deny;\n")
        env["NODE_OPTIONS"] = "--import=" + deny.as_uri()
        command = [self.node, self.resources / "install/native.mjs", *(["--no-link"] if sys.platform == "win32" else [])]
        self.assert_ok(self.invoke(command, env))
        manifest = json.loads((prefix / "install.json").read_text())
        self.assertTrue(Path(manifest["node"]).resolve().is_relative_to(prefix.resolve()))
        self.assertTrue(Path(manifest["node"]).is_file())
        cli = prefix / "bin" / ("athenaeum.exe" if sys.platform == "win32" else "athenaeum")
        project = home / "project"
        project.mkdir()
        self.assert_ok(self.invoke([cli, "-C", project, "setup", "--yes", "--no-agents", "--no-library"], env))
        page = project / "wiki/keep.md"
        page.parent.mkdir(exist_ok=True)
        page.write_text("# Keep local work\n")
        self.assert_ok(self.invoke(command, env))
        self.assertEqual(page.read_text(), "# Keep local work\n")
        self.assertFalse((home / ".claude.json").exists())
        self.assertFalse((home / ".codex/config.toml").exists())
        if sys.platform == "darwin":
            self.assertEqual((home / ".zprofile").read_text().count("# Athenaeum command-line tools"), 1)
        self.assert_ok(self.invoke([manifest["node"], prefix / "lib/athenaeum-integrations.mjs", "--help"], env))

    def test_agent_install_and_profile_initialize_without_model_request(self):
        home, prefix, env = self.env_for("agent home")
        self.assert_ok(self.invoke([self.node, self.resources / "install/native.mjs", "--agent", *(["--no-link"] if sys.platform == "win32" else [])], env))
        agent = prefix / "agent"
        manifest = json.loads((agent / "install.json").read_text())
        self.assertTrue(Path(manifest["node"]).resolve().is_relative_to(agent.resolve()))
        name = "athenaeum-agent.cmd" if sys.platform == "win32" else "athenaeum-agent"
        self.assertTrue((prefix / "bin" / name).is_file())
        if sys.platform != "win32":
            self.assertTrue((home / ".local/bin/athenaeum-agent").is_symlink())
        # Same public SDK initialization probe the owning repository ships.
        self.assert_ok(self.invoke([manifest["node"], ROOT / "packages/agent/install/probe.mjs", agent / "bin" / name, home], env))

    if sys.platform == "darwin":
        def test_macos_installer_contains_native_wizard_and_application(self):
            app = self.root / "package fixture/Athenaeum.app"
            (app / "Contents/MacOS").mkdir(parents=True)
            (app / "Contents/Info.plist").write_bytes(plistlib.dumps({
                "CFBundleIdentifier": "com.hanoryx.athenaeum", "CFBundleExecutable": "athenaeum",
                "CFBundleName": "Athenaeum", "CFBundlePackageType": "APPL",
                "CFBundleVersion": "0.1.0", "CFBundleShortVersionString": "0.1.0",
            }))
            # Real Mach-O; the test packages it, never launches or installs it.
            with tarfile.open(Path(TOOLS) / "athenaeum-cli.tar.gz") as archive:
                entry = next(m for m in archive if m.name.lstrip("./") == "athenaeum")
                (app / "Contents/MacOS/athenaeum").write_bytes(archive.extractfile(entry).read())
            (app / "Contents/MacOS/athenaeum").chmod(0o755)
            out = self.root / "packages"
            env = {k: v for k, v in os.environ.items() if not k.startswith("APPLE_")}
            self.assert_ok(self.invoke(["node", ROOT / "scripts/release/package-macos.mjs", app, "0.1.0", "aarch64", out], env))
            expanded = self.root / "expanded"
            self.assert_ok(self.invoke(["pkgutil", "--expand", out / "Athenaeum_0.1.0_aarch64.pkg", expanded], env))
            distribution = (expanded / "Distribution").read_text()
            self.assertIn("welcome.html", distribution)
            self.assertIn("conclusion.html", distribution)
            self.assertIn("com.hanoryx.athenaeum", distribution)
            self.assertTrue(list(expanded.rglob("Payload")))


if __name__ == "__main__":
    unittest.main()
