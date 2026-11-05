"""The engine speaks exactly the published protocol v1 — no more, and no less.

`packages/protocol` publishes the contract: the method list, the request fixtures, the response
shapes and the error codes. This module drives the real engine with those fixtures and asserts that

  * `capabilities` advertises exactly the published method list (a method the engine cannot answer
    must not be advertised, and a published method the engine cannot answer is a failure);
  * every published request fixture is understood — answered, or refused with a published error
    code, never `unsupported_protocol` or `invalid_request`;
  * the response fixtures' required fields are the fields the engine actually sends;
  * the additive v1 methods (`status`, `review`, `sources`, `index`, `path`) answer the pinned
    shapes, and their vocabularies (review kinds, severities, index scopes) are the published ones;
  * the envelope rules hold: a mismatched protocol version, an unknown method, malformed JSON and an
    oversized request each produce the published error code, and an echoed `requestId`.

Set ATHENAEUM_BIN to a built `athenaeum` executable (see harness.py).
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from product_tests.harness import BIN, SKIP_REASON, Bridge, run

PROTOCOL = Path(__file__).resolve().parents[2] / "packages/protocol" / "fixtures" / "bridge" / "v1"


def fixture(name: str) -> dict:
    path = PROTOCOL / name
    if not path.is_file():
        raise AssertionError(f"the published protocol fixtures are missing: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def keys_of(value: object) -> set[str]:
    return set(value) if isinstance(value, dict) else set()


@unittest.skipUnless(BIN, SKIP_REASON)
class PublishedContract(unittest.TestCase):
    """The engine and the published fixtures agree."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-protocol-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        run(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def test_capabilities_advertise_exactly_the_published_methods(self) -> None:
        vocabulary = fixture("vocabulary.json")
        published: list[str] = vocabulary["methods"]
        advertised: list[str] = self.bridge.request("capabilities")["capabilities"]["operations"]
        self.assertEqual(
            advertised,
            published,
            "capabilities.operations must be the published method list, in order: "
            f"missing={sorted(set(published) - set(advertised))} "
            f"unpublished={sorted(set(advertised) - set(published))}",
        )

    def test_published_request_fixtures_are_understood(self) -> None:
        vocabulary = fixture("vocabulary.json")
        codes = set(vocabulary["errorCodes"])
        envelopes = {
            **fixture("requests.json"),
            **fixture("portable-requests.json"),
        }
        self.assertTrue(envelopes, "no request fixtures were published")
        for name, envelope in sorted(envelopes.items()):
            with self.subTest(fixture=name):
                process, reply = self.bridge.send_text(json.dumps(envelope))
                self.assertIn(reply["status"], ("ok", "error"), reply)
                if reply["status"] == "ok":
                    self.assertEqual(reply["response"]["method"], envelope["request"]["method"], reply)
                    self.assertEqual(reply["requestId"], envelope["requestId"], "a requestId must be echoed")
                else:
                    code = reply["error"]["code"]
                    self.assertIn(code, codes, f"{name}: {code!r} is not a published error code")
                    self.assertNotIn(
                        code,
                        ("unsupported_protocol", "invalid_request", "request_too_large"),
                        f"{name}: the engine cannot understand the published fixture request: {reply['error']['message']}",
                    )
                    self.assertNotEqual(process.returncode, 0, "an error envelope must exit non-zero")

    def test_capabilities_shape_matches_the_published_response_fixture(self) -> None:
        published = fixture("responses.json")["capabilities_ok"]["response"]["capabilities"]
        actual = self.bridge.request("capabilities")["capabilities"]
        for key in ("protocolVersion", "operations", "limits", "nodeTypes", "edgeTypes", "authorities"):
            self.assertIn(key, actual, f"capabilities must carry {key}")
        self.assertEqual(keys_of(published["limits"]), keys_of(actual["limits"]), "limit names drifted")
        self.assertEqual(sorted(published["nodeTypes"]), sorted(actual["nodeTypes"]), "node types drifted")
        self.assertEqual(sorted(published["edgeTypes"]), sorted(actual["edgeTypes"]), "edge types drifted")
        self.assertEqual(sorted(published["authorities"]), sorted(actual["authorities"]), "authorities drifted")

    def test_error_codes_are_the_published_set(self) -> None:
        codes = set(fixture("vocabulary.json")["errorCodes"])
        empty = Path(tempfile.mkdtemp(prefix="athenaeum-nowhere-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(empty, ignore_errors=True))
        observed = {
            "unknown method": Bridge(empty).fail("teleport"),
            "no knowledge base": Bridge(empty).fail("context", task="anything"),
        }
        problem = json.loads(self.bridge.send_text("not json")[0].stdout)["error"]
        observed["malformed envelope"] = problem
        reference = fixture("responses.json")["workspace_not_found"]["error"]["code"]
        self.assertEqual(observed["no knowledge base"]["code"], reference, "the absent-vault code drifted")
        for name, error in observed.items():
            self.assertIn(error["code"], codes, f"{name}: {error['code']!r} is not published")
            self.assertTrue(error["message"], f"{name}: an error must explain itself")


@unittest.skipUnless(BIN, SKIP_REASON)
class EnvelopeRules(unittest.TestCase):
    """Framing rules that hold whatever the payload is."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-envelope-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        run(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def test_a_mismatched_protocol_version_is_refused_and_the_request_id_survives(self) -> None:
        process, reply = self.bridge.send_text(
            json.dumps({"protocolVersion": 99, "requestId": "v99", "request": {"method": "capabilities"}})
        )
        self.assertEqual(reply["status"], "error")
        self.assertEqual(reply["error"]["code"], "unsupported_protocol")
        self.assertEqual(reply["requestId"], "v99", "the requestId must be echoed even on an error")
        self.assertIn("99", reply["error"]["message"])
        self.assertNotEqual(process.returncode, 0)

    def test_an_unknown_method_is_an_invalid_request(self) -> None:
        error = self.bridge.fail("teleport")
        self.assertEqual(error["code"], "invalid_request")

    def test_an_unknown_request_field_is_rejected_by_name(self) -> None:
        """Strict at the boundary, by decision.

        The bridge is an external boundary: a typo'd or unhonoured field must be refused with the
        field named, not silently ignored, because a silently dropped field can change behaviour.
        The cost is deliberate — a newer client against an older engine must upgrade the engine
        rather than have its extra field ignored, and that refusal is the protocol-mismatch signal
        the agent bundle and the MCP server classify.
        """
        error = self.bridge.fail("search", query="oauth", from_a_newer_client=True)
        self.assertEqual(error["code"], "invalid_request")
        self.assertIn("from_a_newer_client", error["message"], error)

    def test_malformed_json_is_an_invalid_request_with_one_envelope(self) -> None:
        process, reply = self.bridge.send_text("{ this is not json")
        self.assertEqual(reply["status"], "error")
        self.assertEqual(reply["error"]["code"], "invalid_request")
        self.assertNotEqual(process.returncode, 0)

    def test_an_oversized_request_is_refused_by_name(self) -> None:
        payload = json.dumps({"protocolVersion": 1, "requestId": "huge", "request": {"method": "search", "query": "x" * 2_000_000}})
        self.assertGreater(len(payload.encode("utf-8")), 1024 * 1024)
        process, reply = self.bridge.send_text(payload)
        self.assertEqual(reply["status"], "error")
        self.assertEqual(reply["error"]["code"], "request_too_large")
        self.assertNotEqual(process.returncode, 0)

    def test_capabilities_need_no_knowledge_base_but_other_methods_do(self) -> None:
        empty = Path(tempfile.mkdtemp(prefix="athenaeum-bare-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(empty, ignore_errors=True))
        capabilities = Bridge(empty).request("capabilities")["capabilities"]
        self.assertIn("search", capabilities["operations"])
        self.assertEqual(Bridge(empty).fail("search", query="anything")["code"], "workspace_not_found")
        self.assertFalse((empty / ".athenaeum").exists(), "a refused read must not create a knowledge base")


@unittest.skipUnless(BIN, SKIP_REASON)
class PinnedAdditiveMethods(unittest.TestCase):
    """The additive v1 methods, exactly as `packages/protocol` publishes them.

    These fail loudly until the engine lands them; they are not skipped, because a method the
    contract promises and the engine does not answer is a real gap.
    """

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-additive-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        self.source = self.workspace / "raw/sources/oauth.md"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("OAuth callback checks state before creating a session.\n", encoding="utf-8")
        run(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def test_status_reports_the_published_fields(self) -> None:
        self.workspace.joinpath("wiki").mkdir(exist_ok=True)
        self.workspace.joinpath("wiki/page.md").write_text(
            "---\nid: page\ntitle: A page\nsummary: Reference material.\n---\n# A page\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        status = self.bridge.request("status")["status"]
        expected = set(fixture("responses.json")["status_ok"]["response"]["status"])
        self.assertEqual(keys_of(status), expected, "status must carry exactly the published fields")
        self.assertEqual(status["protocolVersion"], 1)
        self.assertIsInstance(status["engineVersion"], str)
        self.assertIsInstance(status["workspacePath"], str)
        self.assertEqual(status["initialized"], True)
        self.assertEqual(status["vaultPages"], 1, "status must count the pages the vault holds")
        self.assertIsInstance(status["nodes"], int)
        self.assertIsInstance(status["nodes"], int)
        self.assertIsInstance(status["edges"], int)
        self.assertIsInstance(status["proposals"], int)
        self.assertIsInstance(status["findings"], int)
        self.assertIsInstance(status["indexStale"], bool)
        self.assertTrue(status["indexedAt"] is None or isinstance(status["indexedAt"], str))

    def test_review_reports_published_findings(self) -> None:
        vocabulary = fixture("vocabulary.json")
        response = self.bridge.request("review", limit=50)["findings"]
        for finding in response:
            with self.subTest(finding=finding.get("id")):
                self.assertEqual(
                    {"id", "kind", "severity", "title", "detail", "nodes", "evidence", "sourcePath", "confidence", "blocking"},
                    keys_of(finding),
                    "a finding must carry exactly the published fields",
                )
                self.assertIn(finding["kind"], vocabulary["reviewKinds"])
                self.assertIn(finding["severity"], vocabulary["reviewSeverities"])
                self.assertIsInstance(finding["nodes"], list)
                self.assertIsInstance(finding["evidence"], list)
                self.assertIsInstance(finding["blocking"], bool)
                self.assertGreaterEqual(finding["confidence"], 0.0)
                self.assertLessEqual(finding["confidence"], 1.0)

    def test_sources_reports_real_bytes_and_hashes(self) -> None:
        run(self.workspace, "index-vault")
        published = fixture("responses.json")["sources_ok"]["response"]["sources"][0]
        sources = self.bridge.request("sources", limit=100)["sources"]
        found = next((item for item in sources if item["path"] == "raw/sources/oauth.md"), None)
        self.assertIsNotNone(found, f"an indexed source must be listed; got {[item['path'] for item in sources]}")
        assert found is not None
        self.assertEqual(keys_of(found), keys_of(published), "sources entries must carry the published fields")
        self.assertEqual(found["bytes"], self.source.stat().st_size)
        self.assertEqual(found["sha256"], hashlib.sha256(self.source.read_bytes()).hexdigest())
        self.assertIsInstance(found["scanned"], bool)
        self.assertIsInstance(found["nodes"], int)
        self.assertIsInstance(found["anchors"], int)
        self.assertIsInstance(found["stale"], bool)

    def test_index_reports_the_published_fields_for_every_scope(self) -> None:
        vocabulary = fixture("vocabulary.json")
        published = keys_of(fixture("responses.json")["index_ok"]["response"]["index"])
        for scope in vocabulary["indexScopes"]:
            with self.subTest(scope=scope):
                report = self.bridge.request("index", scope=scope)["index"]
                self.assertEqual(keys_of(report), published, "the index report must carry the published fields")
                self.assertIsInstance(report["incomplete"], bool)
                self.assertIsInstance(report["skipped"], list)
                for key in ("vaultPages", "repositoryFiles", "symbols", "edges", "durationMs"):
                    self.assertIsInstance(report[key], int, f"{key} must be an integer")

    def test_an_unpublished_index_scope_is_refused(self) -> None:
        self.bridge.request("index", scope="vault")  # a missing method must not look like a bad scope
        error = self.bridge.fail("index", scope="elsewhere")
        self.assertEqual(error["code"], "invalid_request")

    def test_path_answers_between_two_pages(self) -> None:
        run(self.workspace, "index-vault")
        page = self.workspace / "wiki/oauth.md"
        page.parent.mkdir(exist_ok=True)
        page.write_text(
            "---\nid: oauth\ntitle: OAuth callback\nsummary: Validate state first.\n---\n# OAuth callback\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        response = self.bridge.request("path", **{"from": "OAuth callback", "to": "OAuth callback", "depth": 2, "maxNodes": 10})
        self.assertIn("path", response)
        # An unreachable pair is a normal ok envelope with found:false, not an error.
        published = fixture("responses.json")["path_not_found_ok"]["response"]
        self.assertIn("found", published["path"])
        self.assertFalse(published["path"]["found"])
        self.assertEqual(published["path"]["steps"], [])


if __name__ == "__main__":
    unittest.main()
