"""Black-box tests for the installed wiki command. No UI, model, or hosted credentials."""
from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

from product_tests.harness import BIN, SKIP_REASON, child_env, Bridge


@unittest.skipUnless(BIN, SKIP_REASON)
class WikiCli(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="wiki-cli-Ω-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.settings = self.root / "app-state.json"
        self.environment = {"WIKI_SETTINGS_FILE": str(self.settings), "ATHENAEUM_LIBRARY_TOKEN": "test-secret",
                            "WIKI_DEV_ROOT": str(self.root)}

    def cli(self, *args, ok=True, input=None, env=None, timeout=120):
        result = subprocess.run([BIN, *map(str, args)], cwd=self.root, env=child_env({**self.environment, **(env or {})}),
                                input=input, text=True, encoding="utf-8", capture_output=True, timeout=timeout)
        if ok:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def data(self, *args, **kwargs):
        return json.loads(self.cli(*args, "--json", **kwargs).stdout)

    def config(self, *args, **kwargs):
        return self.data("config", *args, **kwargs)

    def workspace(self):
        app = self.root
        (app / "packages/protocol").mkdir(parents=True)
        (app / "packages/protocol/package.json").write_text("{}")
        (app / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        return app

    def test_catalog_describes_real_parser_and_every_help_path_works(self):
        rows = self.data("commands")["commands"]
        names = {row["command"] for row in rows}
        for required in ["wiki create", "wiki setup", "wiki dev build", "wiki config set", "wiki api routes", "wiki sync resolve"]:
            self.assertTrue(required in names or any(required.replace("create", "add") == r["command"] for r in rows), required)
        self.assertGreater(len(rows), 75)
        for row in rows:
            path = row["command"].split()[1:]
            if path and path[-1] != "help":
                self.cli(*path, "--help")

    def test_aliases_and_reviewed_markdown_round_trip(self):
        self.data("-C", self.root, "repo", "init", "--name", "Research")
        node = self.data("-C", self.root, "create", "Architecture Ω", "--content", "Durable knowledge.")
        self.data("-C", self.root, "accept", node["id"])
        self.assertTrue(list((self.root / "wiki").rglob("*.md")))
        opened = self.data("-C", self.root, "open", node["id"])
        self.assertEqual(opened["title"], "Architecture Ω")

    def test_completions_are_generated_for_all_shells(self):
        for shell in ["bash", "zsh", "fish", "powershell", "elvish"]:
            self.assertIn("wiki", self.cli("completions", shell).stdout)

    def test_account_sign_in_rejects_malformed_input_without_echoing_credentials(self):
        for value in ['{"password":"private-test-password"', '{"username":"user","password":"private-test-password","extra":true}', 'x' * 17000]:
            result = self.cli("library", "sign-in", "--server", "http://127.0.0.1:1", "--stdin", "--json", input=value, ok=False)
            self.assertNotIn("private-test-password", result.stdout + result.stderr)
            self.assertEqual(json.loads(result.stdout)["error"]["code"], "invalid_request")

    def test_settings_preserve_unrelated_fields_and_backup_first_original(self):
        original = {"other": {"keep": 42}, "llmConfig": {"model": "old", "apiKey": "secret-key"}}
        self.settings.write_text(json.dumps(original), encoding="utf-8")
        self.config("set", "llmConfig.model", "new")
        self.config("set", "llmConfig.temperature", "0.3")
        actual = json.loads(self.settings.read_text(encoding="utf-8"))
        self.assertEqual(actual["other"], original["other"])
        self.assertEqual(actual["llmConfig"]["apiKey"], "secret-key")
        self.assertEqual(actual["llmConfig"]["temperature"], 0.3)
        self.assertEqual(json.loads(self.settings.with_suffix(".json.wiki-backup").read_text()), original)
        self.assertFalse(self.settings.with_suffix(".wiki-lock").exists())

    def test_secrets_are_redacted_in_reads_and_export(self):
        self.config("set", "providerConfigs", input='{"openai":{"apiKey":"secret-key","model":"test"},"customHeaders":{"Authorization":"bearer secret"}}', *["--stdin"])
        self.assertNotIn("secret-key", json.dumps(self.config("list")))
        self.assertEqual(self.config("get", "providerConfigs.openai.apiKey"), "[REDACTED]")
        self.assertEqual(self.config("--reveal", "get", "providerConfigs.openai.apiKey"), "secret-key")
        output = self.root / "export.json"
        self.config("export", output)
        self.assertNotIn("secret-key", output.read_text())
        self.config("import", output)
        self.assertEqual(self.config("--reveal", "get", "providerConfigs.openai.apiKey"), "secret-key")

    def test_settings_pointer_stdin_file_and_unset(self):
        self.config("set", "/providerConfigs/preset.with.dot/model", "--stdin", input='"模型 Ω"')
        self.assertEqual(self.config("get", "/providerConfigs/preset.with.dot/model"), "模型 Ω")
        value_file = self.root / "value.json"
        value_file.write_text('{"enabled":true}', encoding="utf-8")
        self.config("set", "apiConfig", "--value-file", value_file)
        self.assertTrue(self.config("get", "apiConfig.enabled"))
        self.assertTrue(self.config("unset", "apiConfig.enabled")["removed"])
        self.config("get", "apiConfig.enabled", ok=False)

    def test_corrupt_settings_and_scalar_paths_are_never_overwritten(self):
        self.settings.write_text("broken json", encoding="utf-8")
        self.assertEqual(Path(self.config("path")["path"]), self.settings)
        self.config("set", "llmConfig.model", "new", ok=False)
        self.assertEqual(self.settings.read_text(), "broken json")
        self.settings.write_text('{"llmConfig":"old"}', encoding="utf-8")
        self.config("set", "llmConfig.model", "new", ok=False)
        self.assertEqual(json.loads(self.settings.read_text()), {"llmConfig": "old"})
        self.config("import", self.root / "missing.json", ok=False)

    def test_settings_reject_concurrent_cli_writer(self):
        self.settings.with_suffix(".wiki-lock").write_text("")
        self.config("set", "foo", "bar", ok=False)
        self.assertFalse(self.settings.exists())

    def test_dev_build_plan_preserves_selection_and_release(self):
        self.workspace()
        (self.root / "packages/integrations").mkdir(parents=True)
        (self.root / "packages/integrations/package.json").write_text(json.dumps({"scripts": {"build": "tsc"}}))
        plan = self.data("dev", "--root", self.root, "--dry-run", "build", "engine,cli", "--release")
        self.assertEqual([s["component"] for s in plan["steps"]], ["engine", "cli"])
        self.assertTrue(all("--release" in s["args"] for s in plan["steps"]))
        self.assertTrue(all(s["program"] == "cargo" for s in plan["steps"]))

    def test_dev_test_plan_uses_central_runner_and_fails_on_skips(self):
        self.workspace()
        validation = self.root / "validation"
        validation.mkdir()
        (validation / "verify_all.py").write_text("", encoding="utf-8")
        plan = self.data("dev", "--root", self.root, "--dry-run", "test", "cli")
        self.assertIn("--fail-on-skip", plan["steps"][0]["args"])
        self.assertIn("product-cli", plan["steps"][0]["args"][2])
        explicit = self.data("dev", "--root", self.root, "--dry-run", "test", "--suite", "product-cli")
        self.assertEqual(len(explicit["steps"]), 1)

    def test_desktop_build_includes_frontend_and_never_bundles_installers(self):
        app = self.workspace()
        (app / "package.json").write_text(json.dumps({"scripts": {"tauri": "tauri"}}))
        for release in [False, True]:
            plan = self.data("dev", "--root", self.root, "--dry-run", "build", "desktop",
                             *(["--release"] if release else []))
            self.assertEqual(len(plan["steps"]), 1)
            self.assertEqual(plan["steps"][0]["args"],
                             ["run", "tauri", "--", "build", "--no-bundle"] + ([] if release else ["--debug"]))

    def test_start_builds_only_if_missing_or_explicitly_requested(self):
        app = self.workspace()
        (app / "package.json").write_text(json.dumps({"scripts": {"tauri": "tauri", "desktop:build": "node scripts/desktop-build.mjs"}}))
        environment = {"WIKI_DEV_ROOT": str(self.root), "CARGO_TARGET_DIR": str(self.root / "cache")}
        live = self.data("start", "--dry-run", env=environment)
        self.assertTrue(live["steps"][0]["args"][0].endswith("live-desktop.mjs"))
        missing = self.data("start", "--standalone", "--dry-run", env=environment)
        self.assertTrue(missing["buildRequired"])
        self.assertEqual(missing["steps"][0]["args"], ["run", "desktop:build"])
        binary = Path(missing["launch"]["program"])
        self.assertEqual(binary.parent.resolve(), (self.root / "cache" / "standalone").resolve())
        self.assertFalse(binary.exists())  # dry-run has no build or launch side effects
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture")
        cached = self.data("start", "--standalone", "--dry-run", env=environment)
        self.assertFalse(cached["buildRequired"])
        self.assertEqual(cached["steps"], [])
        refresh = self.data("start", "--build", "--dry-run", env=environment)
        self.assertTrue(refresh["buildRequired"])
        self.assertEqual(len(refresh["steps"]), 1)

    @unittest.skipUnless(shutil.which("node"), "requires node for the snapshot builder")
    def test_standalone_builder_embeds_ui_and_survives_live_changes_and_build_failure(self):
        app = self.workspace()
        builder = Path(__file__).resolve().parents[2] / "scripts" / "desktop-build.mjs"
        (app / "scripts").mkdir()
        shutil.copyfile(builder, app / "scripts" / builder.name)
        cli = app / "node_modules" / "@tauri-apps" / "cli" / "tauri.js"
        cli.parent.mkdir(parents=True)
        cli.write_text("const fs = require('node:fs'); const path = require('node:path'); "
                       "fs.writeFileSync('build-args.json', JSON.stringify(process.argv.slice(2))); "
                       "if (process.env.FAIL_BUILD) process.exit(7); "
                       "const out = path.join(process.env.CARGO_TARGET_DIR, 'debug'); "
                       "fs.mkdirSync(out, {recursive: true}); "
                       "fs.writeFileSync(path.join(out, process.platform === 'win32' ? 'athenaeum-app.exe' : 'athenaeum-app'), 'embedded-app'); "
                       "fs.writeFileSync(path.join(out, 'harness-driver.mjs'), 'driver');", encoding="utf-8")
        target = self.root / "cache with spaces Ω"
        environment = child_env({"CARGO_TARGET_DIR": str(target)})
        result = subprocess.run(["node", str(app / "scripts" / builder.name)], cwd=app,
                                env=environment, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads((app / "build-args.json").read_text()),
                         ["build", "--debug", "--no-bundle"])
        name = "athenaeum-app.exe" if os.name == "nt" else "athenaeum-app"
        snapshot = target / "standalone" / name
        self.assertEqual(snapshot.read_text(), "embedded-app")
        self.assertEqual((snapshot.parent / "harness-driver.mjs").read_text(), "driver")
        (target / "debug" / name).write_text("live-vite-app")
        self.assertEqual(snapshot.read_text(), "embedded-app")
        failed = subprocess.run(["node", str(app / "scripts" / builder.name)], cwd=app,
                                env={**environment, "FAIL_BUILD": "1"}, capture_output=True, text=True, timeout=20)
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual(snapshot.read_text(), "embedded-app")
        self.assertFalse(list(target.glob("standalone-*.tmp")))

    def test_start_reuses_legacy_release_and_prefers_standalone(self):
        app = self.workspace()
        target = self.root / "cache"
        name = "athenaeum-app.exe" if os.name == "nt" else "athenaeum-app"
        release = target / "release" / name
        release.parent.mkdir(parents=True)
        release.write_bytes(b"release")
        environment = {"CARGO_TARGET_DIR": str(target)}
        legacy = self.data("start", "--standalone", "--dry-run", env=environment)
        self.assertFalse(legacy["buildRequired"])
        self.assertEqual(Path(legacy["launch"]["program"]), release)
        snapshot = target / "standalone" / name
        snapshot.parent.mkdir()
        snapshot.write_bytes(b"standalone")
        ready = self.data("start", "--standalone", "--dry-run", env=environment)
        self.assertFalse(ready["buildRequired"])
        self.assertEqual(ready["steps"], [])
        self.assertEqual(Path(ready["launch"]["program"]), snapshot)

    def test_start_dev_uses_live_desktop_even_when_release_is_missing(self):
        app = self.workspace()
        (app / "package.json").write_text(json.dumps({"scripts": {"tauri": "tauri"}}))
        live = self.data("start", "--root", self.root, "--dev", "--dry-run")
        serve = self.data("dev", "--root", self.root, "--dry-run", "serve", "desktop")
        self.assertEqual(live["steps"], serve["steps"])
        self.assertEqual(live["steps"][0]["args"], ["run", "tauri", "--", "dev"])
        rejected = self.cli("start", "--root", self.root, "--dev", "--json", ok=False)
        self.assertEqual(json.loads(rejected.stdout)["error"]["code"], "invalid_request")
        self.cli("start", "--root", self.root, "--dev", "--build", ok=False)

    @unittest.skipUnless(shutil.which("node") and shutil.which("npm"), "requires node/npm")
    def test_start_dev_runs_foreground_script_and_propagates_failure(self):
        app = self.workspace()
        (app / "package.json").write_text(json.dumps({"scripts": {
            "tauri": 'node -e "console.log(process.argv.slice(1).join(\',\')); process.exit(7)" --'
        }}))
        result = self.cli("start", "--root", self.root, "--dev", ok=False)
        self.assertIn("dev", result.stdout)

    def test_start_failed_build_does_not_launch_and_keeps_json_stdout_clean(self):
        app = self.workspace()
        (app / "package.json").write_text(json.dumps({"scripts": {
            "desktop:build": 'node -e "console.log(\'build-output\'); process.exit(7)" --'
        }}))
        result = self.cli("start", "--root", self.root, "--build", "--json", ok=False,
                          env={"CARGO_TARGET_DIR": str(self.root / "cache")})
        self.assertEqual(result.stdout, "")
        self.assertIn("build-output", result.stderr)

    def test_desktop_transport_uses_authentication_json_stdin_and_no_proxy(self):
        seen = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                seen.append((self.headers.get('Authorization'), self.path, body))
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps({'result': {'received': body}}).encode())
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        connection = self.root / 'desktop-control.json'
        connection.write_text(json.dumps({'url': f'http://127.0.0.1:{server.server_port}', 'token': 'private-desktop-token'}))
        result = self.data('desktop', 'call', 'wiki.setActiveView', '--stdin', input='["graph"]',
                           env={'HTTP_PROXY': 'http://127.0.0.1:1', 'ALL_PROXY': 'http://127.0.0.1:1'})
        self.assertEqual(seen[0][0], 'Bearer private-desktop-token')
        self.assertEqual(seen[0][1], '/command')
        self.assertEqual(result['received']['payload']['request'], {'action':'wiki.setActiveView', 'args':['graph']})
        self.assertNotIn('private-desktop-token', json.dumps(result))
        self.data('desktop', 'invoke', 'read_file', '--stdin', input='["wrong shape"]', ok=False)
        self.assertEqual(len(seen), 1)

    def test_desktop_transport_rejects_remote_and_credentialed_descriptors(self):
        for url in ['https://example.com:443/', 'http://localhost:1420/', 'http://127.0.0.1:1@outside.invalid/',
                    'http://user:password@127.0.0.1:1420/', 'http://127.0.0.1:1420/?token=leak']:
            (self.root / 'desktop-control.json').write_text(json.dumps({'url':url, 'token':'private-desktop-token'}))
            result = self.cli('desktop', 'status', '--json', ok=False)
            self.assertEqual(json.loads(result.stdout)['error']['code'], 'invalid_config')
            self.assertNotIn('private-desktop-token', result.stdout + result.stderr)

    def test_desktop_schema_and_label_contracts(self):
        script = Path(__file__).with_name('desktop_control.test.mjs')
        result = subprocess.run(['node', '--test', str(script)], env=child_env(),
                                text=True, encoding='utf-8', capture_output=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_managed_live_launcher_starts_and_reuses_a_session_from_unicode_path(self):
        app = self.workspace()
        scripts = app / 'scripts'
        scripts.mkdir()
        source = Path(__file__).resolve().parents[2] / 'scripts' / 'live-desktop.mjs'
        shutil.copy2(source, scripts / source.name)
        tauri = app / 'node_modules' / '@tauri-apps' / 'cli'
        tauri.mkdir(parents=True)
        (tauri / 'tauri.js').write_text("const fs = require('fs'); fs.writeFileSync('started.json', JSON.stringify(process.argv.slice(2))); const timer = setInterval(() => { if (fs.existsSync('stop')) { clearInterval(timer); process.exit(0); } }, 50); setTimeout(() => process.exit(0), 15000).unref();", encoding='utf-8')
        first = self.data('start', '--root', self.root)
        self.assertTrue(first['started'])
        until = time.monotonic() + 10
        while not (app / 'started.json').exists() and time.monotonic() < until:
            time.sleep(0.05)
        self.assertEqual(json.loads((app / 'started.json').read_text()), ['dev'])
        try:
            second = self.cli('start', '--root', self.root, '--json')
            self.assertTrue(json.loads(second.stdout)['reused'])
        finally:
            (app / 'stop').write_text('stop')
        # The fake child exits itself. The supervisor must clean up and close logs.
        descriptor = app / '.athenaeum-dev' / 'live.json'
        until = time.monotonic() + 10
        while descriptor.exists() and time.monotonic() < until:
            time.sleep(0.05)
        self.assertFalse(descriptor.exists())

    def test_disconnect_saved_links_survives_reindex_and_preserves_code(self):
        self.data('-C', self.root, 'repo', 'init', '--name', 'Disconnect')
        wiki = self.root / 'wiki'
        wiki.mkdir(exist_ok=True)
        (wiki / 'source.md').write_text('---\nid: node_cli_source\ntitle: Source\nrelations:\n  - "depends_on [[target]]"\nrelated:\n  - "[[target]]"\n---\n# Source\nRead [[target|the target]] and `[[target]]`.\n\n```md\n[[target]]\n```\n', encoding='utf-8')
        (wiki / 'target.md').write_text('---\nid: node_cli_target\ntitle: Target\n---\n# Target\n', encoding='utf-8')
        self.data('-C', self.root, 'index-vault')
        for _ in range(3):
            relationships = Bridge(self.root).request('open', node='node_cli_source')['detail']['relationships']
            edge = next(item for item in relationships if item['id'] == 'node_cli_target')
            self.data('-C', self.root, 'unlink', edge['edgeId'])
            self.data('-C', self.root, 'index-vault')
        self.assertIsNone(self.data('-C', self.root, 'path', 'node_cli_source', 'node_cli_target'))
        contents = (wiki / 'source.md').read_text(encoding='utf-8')
        self.assertIn('Read the target and `[[target]]`.', contents)
        self.assertIn('```md\n[[target]]\n```', contents)
        (self.root / '.athenaeum' / 'knowledge.db').unlink()
        self.data('-C', self.root, 'index-vault')
        self.assertIsNone(self.data('-C', self.root, 'path', 'node_cli_source', 'node_cli_target'))

    @unittest.skipUnless(shutil.which("rustc"), "requires rustc for a native desktop fixture")
    def test_start_returns_while_native_desktop_remains_open(self):
        app = self.workspace()
        target = self.root / "cache with spaces" / "standalone"
        target.mkdir(parents=True)
        binary = target / ("athenaeum-app.exe" if os.name == "nt" else "athenaeum-app")
        source = self.root / "desktop.rs"
        source.write_text('fn main() { std::fs::write("desktop-pid", std::process::id().to_string()).unwrap(); '
                          'std::thread::sleep(std::time::Duration::from_secs(30)); }', encoding="utf-8")
        subprocess.run(["rustc", str(source), "-o", str(binary)], check=True, capture_output=True, timeout=60)
        try:
            report = json.loads(self.cli("start", "--standalone", "--root", self.root, "--json", timeout=10,
                                env={"CARGO_TARGET_DIR": str(target.parent)}).stdout)
            self.assertTrue(report["started"])
            self.assertFalse(report["built"])
            if os.name != "nt":
                self.assertNotEqual(os.getpgid(report["pid"]), os.getpgrp())
            for _ in range(40):
                if (app / "desktop-pid").exists():
                    break
                time.sleep(0.05)
            self.assertEqual(int((app / "desktop-pid").read_text()), report["pid"])
        finally:
            if (app / "desktop-pid").exists():
                try:
                    os.kill(int((app / "desktop-pid").read_text()), signal.SIGTERM)
                except ProcessLookupError:
                    pass

    def test_dev_missing_root_and_unsupported_task_fail(self):
        self.data("dev", "--root", self.root / "missing", "components", ok=False)
        self.workspace()
        self.data("dev", "--root", self.root, "--dry-run", "serve", "engine", ok=False)
        self.data("dev", "--root", self.root, "--dry-run", "run", "cli", "nonexistent", ok=False)
        (self.root / "validation").mkdir()
        self.data("dev", "--root", self.root, "--dry-run", "build", "validation", ok=False)

    def test_dev_check_and_format_are_scoped_to_the_selected_crate(self):
        self.workspace()
        plan = self.data("dev", "--root", self.root, "--dry-run", "check", "cli")
        self.assertIn("athenaeum-cli", plan["steps"][0]["args"])
        self.assertIn("--check", plan["steps"][0]["args"])
        formatted = self.data("dev", "--root", self.root, "--dry-run", "fmt", "cli")
        self.assertEqual(formatted["steps"][0]["args"], ["fmt", "-p", "athenaeum-cli"])

    def test_validation_fail_on_skip_includes_individual_tests(self):
        script = """
import sys
import verify_all as runner
def partial():
    runner.run([sys.executable, '-c', 'import unittest; unittest.TextTestRunner(verbosity=2).run(unittest.FunctionTestCase(lambda: (_ for _ in ()).throw(unittest.SkipTest("intentional fixture skip"))))'])
runner.STEPS = {'probe': ('partial test execution', partial)}
sys.argv = ['verify_all.py', '--only', 'probe', '--fail-on-skip']
sys.exit(runner.main())
"""
        result = subprocess.run([sys.executable, "-c", script], cwd=Path(__file__).resolve().parents[1],
                                capture_output=True, text=True, encoding="utf-8", env=child_env(), timeout=30)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("intentional fixture skip", result.stdout)
        self.assertIn("skipped", result.stdout)

    def test_api_can_read_a_separate_public_schema_without_credentials(self):
        server, seen = self.server()
        report = self.data("api", "--server", "http://127.0.0.1:1", "--schema-server", server, "routes")
        self.assertEqual(report["count"], 3)
        self.assertIsNone(seen[0]["authorization"])

    @unittest.skipUnless(shutil.which("npm") and shutil.which("node"), "requires npm and node")
    def test_dev_run_executes_real_script_and_keeps_stdout_machine_readable(self):
        app = self.workspace()
        (app / "runner.cjs").write_text("console.log('child-output'); process.exit(Number(process.argv[2] || 0));", encoding="utf-8")
        (app / "package.json").write_text(json.dumps({"scripts": {"probe": "node runner.cjs"}}), encoding="utf-8")
        report = self.data("dev", "--root", self.root, "run", "app", "probe")
        self.assertTrue(report["ok"])
        # Global --json precedes the delimiter; subsequent arguments belong to the child.
        result = self.cli("--json", "dev", "--root", self.root, "run", "app", "probe", "--", "7", ok=False)
        self.assertEqual(json.loads(result.stdout)["steps"][0]["exitCode"], 7)
        self.assertIn("child-output", result.stderr)

    def server(self):
        seen = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self): self.respond()
            def do_POST(self): self.respond()
            def respond(self):
                length = int(self.headers.get("content-length", "0"))
                body = self.rfile.read(length).decode() if length else None
                seen.append({"method": self.command, "path": self.path, "authorization": self.headers.get("authorization"), "body": body})
                if self.path == "/openapi.json":
                    data = {"openapi":"3.1.0", "paths":{"/api/v1/repositories":{"get":{"summary":"List repositories"}, "post":{"summary":"Create repository"}}, "/api/v1/admin/status":{"get":{"summary":"Operator status"}}}}
                elif self.path.endswith("redirect"):
                    self.send_response(302)
                    self.send_header("Location", "/api/v1/leaked")
                    self.end_headers()
                    return
                elif self.path.endswith("denied"):
                    self.send_response(403)
                    self.end_headers()
                    self.wfile.write(b'{"error":{"code":"forbidden","message":"denied"}}')
                    return
                else: data = {"ok":True}
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(data).encode())
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f"http://127.0.0.1:{server.server_port}", seen

    def test_api_discovers_every_server_route_and_can_filter(self):
        server, seen = self.server()
        report = self.data("api", "--server", server, "routes")
        self.assertEqual(report["count"], 3)
        self.assertEqual(self.data("api", "--server", server, "routes", "operator")["count"], 1)
        self.assertIsNone(seen[0]["authorization"])

    def test_api_uses_stored_transport_rules_and_encodes_query_and_body(self):
        server, seen = self.server()
        result = self.data("api", "--server", server, "post", "repositories", "--stdin", "-q", "search=one & two", input='{"name":"demo"}')
        self.assertEqual(result["status"], 200)
        self.assertEqual(seen[-1]["authorization"], "Bearer test-secret")
        self.assertEqual(json.loads(seen[-1]["body"]), {"name":"demo"})
        self.assertIn("search=one+%26+two", seen[-1]["path"])

    def test_api_denial_redirect_and_unsafe_paths_fail(self):
        server, seen = self.server()
        self.data("api", "--server", server, "get", "denied", ok=False)
        self.data("api", "--server", server, "get", "redirect", ok=False)
        self.assertFalse(any(r["path"].endswith("leaked") for r in seen))
        count = len(seen)
        for path in ["https://evil.invalid", "../escape", "%2e%2e/escape", "repositories?x=y"]:
            self.data("api", "--server", server, "get", path, ok=False)
        self.assertEqual(len(seen), count)

    def test_api_dry_run_has_no_network_or_secrets(self):
        report = self.data("api", "--server", "http://127.0.0.1:1", "post", "repositories", "--dry-run", "--body", '{"apiKey":"never-print"}')
        self.assertTrue(report["dryRun"])
        self.assertNotIn("never-print", json.dumps(report))

    def test_agent_and_mcp_keep_their_own_stream_protocol(self):
        self.data("agent", ok=False)
        self.data("mcp", ok=False)

    @unittest.skipUnless(shutil.which("node"), "requires node")
    def test_launchers_forward_arguments_and_the_same_engine_in_the_project(self):
        self.data("-C", self.root, "init")
        nested = self.root / "nested"
        nested.mkdir()
        launcher = self.root / "probe.mjs"
        launcher.write_text("console.log(JSON.stringify({args:process.argv.slice(2),engine:process.env.ATHENAEUM_BIN,cwd:process.cwd()}))", encoding="utf-8")
        forwarded = ["space in value", 'quote"value', "literal;$(value)"]
        for command, key, prefix in [("agent", "ATHENAEUM_AGENT_BIN", []), ("mcp", "ATHENAEUM_INTEGRATIONS", ["mcp"])]:
            report = json.loads(self.cli("-C", nested, command, "--", *forwarded, env={key:str(launcher)}).stdout)
            self.assertEqual(report["args"], prefix + forwarded)
            self.assertEqual(Path(report["engine"]).resolve(), Path(BIN).resolve())
            self.assertEqual(Path(report["cwd"]).resolve(), self.root.resolve())


if __name__ == "__main__":
    unittest.main()
