"""Evidence, authority, freshness, history, duplicates, contradictions and proposal review.

The engine's promise is narrow and testable: nothing becomes durable knowledge except by an
explicit acceptance; accepted knowledge keeps its evidence, its authority and its history; and
every shape of doubt — a stale anchor, a duplicate, a contradiction, a superseded claim, an
unreviewed proposal, an incomplete scan — is *reported* rather than resolved silently.

The pinned `review` contract (protocol v1, additive) is asserted here in its own TestCase, so a
gap in it fails loudly and is attributable, instead of hiding behind the older `check` output.

Set ATHENAEUM_BIN to a built `athenaeum` executable (see harness.py).
"""

from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest

from product_tests.harness import BIN, SKIP_REASON, Bridge, run, run_json

PROTOCOL = Path(__file__).resolve().parents[2] / "packages/protocol" / "fixtures" / "bridge" / "v1"
CALLER = {"model": "validation-curator", "session": "session-one"}


def vocabulary() -> dict:
    return json.loads((PROTOCOL / "vocabulary.json").read_text(encoding="utf-8"))


@unittest.skipUnless(BIN, SKIP_REASON)
class KnowledgeCase(unittest.TestCase):
    """A workspace with one source file, ready for proposals."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="athenaeum-knowledge-")
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        self.source = self.workspace / "raw/sources/retry.md"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("Retries amplify outages; the budget is three.\n", encoding="utf-8")
        run(self.workspace, "init")
        self.bridge = Bridge(self.workspace)

    def page_paths(self) -> list[Path]:
        wiki = self.workspace / "wiki"
        return sorted(wiki.rglob("*.md")) if wiki.exists() else []

    def propose(self, title: str, summary: str, kind: str = "decision") -> str:
        created = self.bridge.request("create", input={"type": kind, "title": title, "summary": summary}, caller=CALLER)
        return created["node"]["id"]

    def attach(self, node: str, excerpt: str | None = None) -> dict:
        anchor = {"file": "raw/sources/retry.md"}
        fields: dict[str, object] = {"node": node, "kind": "source_file", "anchor": anchor}
        if excerpt is not None:
            fields["excerpt"] = excerpt
        return self.bridge.request("attach_evidence", input=fields, caller=CALLER)

    def accept(self, node: str) -> dict:
        return self.bridge.request("accept", node=node, caller=CALLER)["accepted"]

    def accepted_page(self, node: str) -> Path:
        accepted = self.accept(node)
        return self.workspace / accepted["page"]["path"]

    def check_kinds(self) -> list[str]:
        report = run_json(self.workspace, "check", check=False)
        return [finding["kind"] for finding in report["findings"]]  # type: ignore[index]

    def detail(self, node: str) -> dict:
        return self.bridge.request("open", node=node)["detail"]


class EvidenceAnchors(KnowledgeCase):
    def test_evidence_and_its_anchor_survive_acceptance(self) -> None:
        node = self.propose("Retry budget is three", "Three retries, then fail.")
        self.attach(node, "Retries amplify outages")
        page = self.accepted_page(node)
        text = page.read_text(encoding="utf-8")
        self.assertIn("raw/sources/retry.md", text, "the accepted page cites its source")
        self.assertIn(node, text, "the accepted page carries the node id")
        detail = self.detail(node)
        anchors = [item["anchor"]["file"] for item in detail["evidence"] if item.get("anchor", {}).get("file")]
        self.assertIn("raw/sources/retry.md", anchors)

    def test_a_moved_source_is_reported_never_silently_lost(self) -> None:
        node = self.propose("Retries amplify outages", "A budget bounds the blast radius.")
        self.attach(node)
        self.accepted_page(node)
        moved = self.workspace / "raw/sources/retries.md"
        self.source.rename(moved)
        report = run_json(self.workspace, "relocate")
        followed = report["relocatedBySymbol"] + report["relocatedByFingerprint"] + report["exact"]  # type: ignore[index]
        findings = self.check_kinds()
        self.assertTrue(
            followed > 0 or "anchor_lost" in findings or "stale" in findings,
            f"a moved source must be followed or reported: relocate={report!r} findings={findings}",
        )
        self.assertTrue(self.detail(node)["evidence"], "the evidence must still be recorded, not dropped")

    def test_a_deleted_source_is_reported_as_a_lost_anchor(self) -> None:
        node = self.propose("Retries amplify outages", "A budget bounds the blast radius.")
        self.attach(node)
        self.accepted_page(node)
        self.source.unlink()
        run_json(self.workspace, "relocate", check=False)
        self.assertIn("anchor_lost", self.check_kinds())

    def test_an_accepted_page_is_edited_as_markdown_not_through_proposals(self) -> None:
        node = self.propose("Sessions expire after an hour", "Short sessions limit theft.")
        self.attach(node)
        self.accepted_page(node)
        for method, fields in (
            ("attach_evidence", {"input": {"node": node, "anchor": {"file": "raw/sources/retry.md"}}}),
            ("update", {"input": {"node": node, "summary": "Changed behind the file's back."}}),
        ):
            with self.subTest(method=method):
                error = self.bridge.fail(method, caller=CALLER, **fields)  # type: ignore[arg-type]
                self.assertEqual(error["code"], "invalid_request")
                self.assertTrue(
                    "canonical Markdown" in error["message"] or "change that file instead" in error["message"],
                    f"an edit to accepted knowledge must point at the file, not the index: {error['message']}",
                )

    def test_a_self_link_cannot_modify_a_saved_page(self) -> None:
        node = self.propose("A page cannot refer to itself", "Self links do not connect knowledge.")
        page = self.accepted_page(node)
        before = page.read_bytes()
        error = self.bridge.fail("link", input={"from": node, "type": "related_to", "to": node}, caller=CALLER)
        self.assertEqual(error["code"], "invalid_request")
        self.assertIn("itself", error["message"])
        self.assertEqual(page.read_bytes(), before)


class Authority(KnowledgeCase):
    def test_a_claim_with_no_evidence_cannot_be_verified(self) -> None:
        node = self.propose("Retry budget is three", "Three retries.")
        process = run(self.workspace, "verify", node, "--authority", "project_rule", check=False)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("no evidence", process.stderr)
        self.assertEqual(self.detail(node)["node"]["authority"], "unverified_observation")

    def test_a_raw_source_alone_does_not_establish_a_project_rule(self) -> None:
        node = self.propose("Retry budget is three", "Three retries.")
        self.attach(node)
        process = run(self.workspace, "verify", node, "--authority", "project_rule", check=False)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("establish", process.stderr)
        self.assertEqual(self.detail(node)["node"]["authority"], "unverified_observation")

    def test_knowledge_the_user_stated_can_be_a_project_rule(self) -> None:
        stated = run_json(self.workspace, "add", "-t", "rule", "Jitter the backoff", "-s", "Jitter avoids thundering herds.", "--stated")
        identifier = stated["id"]  # type: ignore[index]
        verified = run_json(self.workspace, "verify", identifier, "--authority", "project_rule")
        self.assertEqual(verified["authority"], "project_rule")  # type: ignore[index]
        self.assertEqual(verified["status"], "valid")  # type: ignore[index]

    def test_accepting_a_model_proposal_never_raises_its_authority(self) -> None:
        node = self.propose("Retry budget is three", "Three retries.")
        self.attach(node)
        before = self.detail(node)["node"]["authority"]
        self.assertEqual(before, "unverified_observation")
        self.accepted_page(node)
        self.assertEqual(self.detail(node)["node"]["authority"], before, "accepting is not verifying")


class FreshnessAndHistory(KnowledgeCase):
    def test_history_is_append_only_across_edits(self) -> None:
        stated = run_json(self.workspace, "add", "-t", "rule", "Jitter the backoff", "-s", "Jitter avoids thundering herds.", "--stated")
        identifier = stated["id"]  # type: ignore[index]
        for index in range(2, 5):
            run_json(self.workspace, "edit", identifier, "-s", f"Jitter avoids thundering herds, revision {index}.")
        history = run_json(self.workspace, "history", identifier)
        versions = [entry["version"] for entry in history]  # type: ignore[index]
        self.assertEqual(sorted(versions, reverse=True), versions, "history is newest-first")
        self.assertEqual(len(versions), 4, f"every edit is kept: {history!r}")
        self.assertIn("revision 2", json.dumps(history))
        self.assertIn("Jitter avoids thundering herds.", json.dumps(history))

    def test_history_of_an_unknown_node_is_a_named_error(self) -> None:
        process = run(self.workspace, "--json", "history", "node_not_here", check=False)
        self.assertNotEqual(process.returncode, 0)
        self.assertEqual(json.loads(process.stdout)["error"]["code"], "not_found")

    def test_a_superseded_claim_keeps_its_history_and_stops_being_offered(self) -> None:
        old = self.propose("Retry budget is three", "Three retries.")
        self.attach(old)
        self.accepted_page(old)
        new = self.propose("Retry budget is five", "Five retries.")
        self.attach(new)
        self.accepted_page(new)
        run_json(self.workspace, "supersede", old, new)
        self.assertEqual(self.detail(old)["node"]["status"], "superseded")
        self.assertEqual(self.detail(new)["node"]["status"], "valid")
        self.assertTrue(self.detail(old)["node"], "the old claim is kept, not deleted")
        brief = self.bridge.request("context", task="Retry budget is three")["context"]["brief"]
        offered = [item["id"] for item in brief["nodes"]]
        self.assertNotIn(old, offered, "a superseded claim is not offered as current knowledge")
        self.assertTrue(self.bridge.request("search", query="Retry budget is three")["hits"], "explicit search still finds it")

    def test_an_unresolved_contradiction_is_reported_and_stops_offering_both(self) -> None:
        first = self.propose("Retry budget is three", "Three retries.")
        self.attach(first)
        self.accepted_page(first)
        second = self.propose("Retry budget is five", "Five retries.")
        self.attach(second)
        self.accepted_page(second)
        run_json(self.workspace, "contradict", first, second)
        self.assertEqual(self.detail(first)["node"]["status"], "contradicted")
        self.assertEqual(self.detail(second)["node"]["status"], "contradicted")
        self.assertIn("contradicted", self.check_kinds())
        brief = self.bridge.request("context", task="Retry budget")["context"]["brief"]
        self.assertNotIn(first, [item["id"] for item in brief["nodes"]])
        self.assertNotIn(second, [item["id"] for item in brief["nodes"]])


class Duplicates(KnowledgeCase):
    def test_a_duplicate_proposal_names_the_node_to_extend(self) -> None:
        first = self.propose("Retry budget three", "Three retries.")
        error = self.bridge.fail(
            "create",
            input={"type": "decision", "title": "retry-budget: three!", "summary": "Said again."},
            caller=CALLER,
        )
        self.assertEqual(error["code"], "invalid_request")
        self.assertEqual(error["details"]["existingNodes"], [first])

    def test_a_duplicate_that_was_written_by_hand_is_still_retrievable(self) -> None:
        """The creation guard refuses a duplicate proposal; a hand-written page is never merged away."""
        first = self.propose("Retry budget three", "Three retries.")
        self.attach(first)
        self.accepted_page(first)
        wiki = self.workspace / "wiki"
        (wiki / "retry-budget-three.md").write_text(
            "---\nid: retry_budget_three\ntitle: Retry budget is 3\nsummary: Three retries, then fail.\n---\n"
            "# Retry budget is 3\n\nThree retries, then fail.\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        titles = {hit["node"]["title"] for hit in self.bridge.request("search", query="Retry budget")["hits"]}
        self.assertIn("Retry budget is 3", titles)
        self.assertTrue((wiki / "retry-budget-three.md").is_file(), "the engine never rewrites a person's page")


class ProposalReview(KnowledgeCase):
    def test_acceptance_is_the_only_path_to_durable_markdown(self) -> None:
        node = self.propose("Retry budget is three", "Three retries.")
        self.attach(node)
        run(self.workspace, "index-vault")
        self.bridge.request("search", query="Retry budget")
        self.bridge.request("context", task="Retry budget")
        self.assertEqual(self.page_paths(), [], "reading a proposal never writes Markdown")
        self.assertIn(node, [item["node"]["id"] for item in self.bridge.request("proposals")["proposals"]])
        page = self.accepted_page(node)
        self.assertTrue(page.is_file())
        self.assertNotIn(node, [item["node"]["id"] for item in self.bridge.request("proposals")["proposals"]])
        self.assertEqual(len(self.page_paths()), 1, "accepting writes exactly one page")

    def test_rejection_writes_nothing_and_is_not_offered_again(self) -> None:
        node = self.propose("Rejected idea", "Nobody wants this.", kind="concept")
        self.bridge.request("reject", node=node, reason="not useful", caller=CALLER)
        self.assertEqual(self.page_paths(), [])
        self.assertNotIn(node, [item["node"]["id"] for item in self.bridge.request("proposals")["proposals"]])
        detail = self.detail(node)
        self.assertEqual(detail["node"]["status"], "archived", "a rejected proposal is archived, not deleted")
        self.assertEqual(detail["node"]["title"], "Rejected idea")
        self.assertEqual(self.bridge.request("search", query="Rejected idea")["hits"], [], "a rejected proposal is not offered")

    def test_accepting_twice_is_reported_not_repeated(self) -> None:
        node = self.propose("Sessions expire after an hour", "Short sessions limit theft.")
        self.attach(node)
        first = self.accept(node)
        self.assertFalse(first["alreadyAccepted"])
        second = self.accept(node)
        self.assertTrue(second["alreadyAccepted"])
        self.assertEqual(len(self.page_paths()), 1, "accepting twice must not write a second page")

    def test_accepting_an_unknown_proposal_is_refused(self) -> None:
        error = self.bridge.fail("accept", node="node_that_never_existed", caller=CALLER)
        self.assertEqual(error["code"], "not_found")
        self.assertEqual(self.page_paths(), [])

    def test_a_link_between_proposals_is_a_proposal_and_writes_nothing(self) -> None:
        first = self.propose("Session cookies are HttpOnly", "Mark them HttpOnly.")
        second = self.propose("Sessions expire after an hour", "Short sessions limit theft.")
        linked = self.bridge.request(
            "link",
            input={"from": first, "type": "related_to", "to": second},
            caller=CALLER,
        )
        self.assertEqual(self.page_paths(), [], "a proposed relationship is not durable Markdown")
        self.assertTrue(linked.get("edge") or linked.get("proposedEdge") or linked)
        run(self.workspace, "index-vault")
        self.assertEqual(self.page_paths(), [], "indexing a proposed edge writes no page")


class PinnedReviewContract(KnowledgeCase):
    """The additive `status`/`review`/`sources`/`index` methods, on seeded doubt.

    These fail loudly until the engine lands them; nothing here is softened to go green.
    """

    def seed_contradiction(self) -> tuple[str, str]:
        first = self.propose("Retry budget is three", "Three retries.")
        self.attach(first)
        self.accepted_page(first)
        second = self.propose("Retry budget is five", "Five retries.")
        self.attach(second)
        self.accepted_page(second)
        run_json(self.workspace, "contradict", first, second)
        return first, second

    def findings(self) -> list[dict]:
        return self.bridge.request("review", limit=100)["findings"]

    def test_status_counts_what_the_vault_holds(self) -> None:
        self.seed_contradiction()
        status = self.bridge.request("status")["status"]
        self.assertTrue(status["initialized"])
        self.assertTrue(status["hasVault"])
        self.assertGreaterEqual(status["nodes"], 3)
        self.assertGreaterEqual(status["proposals"], 0)
        self.assertGreaterEqual(status["edges"], 1)
        self.assertEqual(status["protocolVersion"], 1)

    def test_an_unresolved_contradiction_is_a_blocking_finding(self) -> None:
        first, second = self.seed_contradiction()
        findings = self.findings()
        contradiction = [finding for finding in findings if finding["kind"] == "contradiction"]
        self.assertTrue(contradiction, f"expected a contradiction finding, got {[f['kind'] for f in findings]}")
        self.assertTrue(
            any(first in finding["nodes"] or second in finding["nodes"] for finding in contradiction),
            contradiction,
        )
        self.assertTrue(any(finding["blocking"] for finding in contradiction))

    def test_an_unaccepted_proposal_is_an_unreviewed_proposal_finding(self) -> None:
        node = self.propose("An unreviewed thought", "Nobody has looked at this.")
        kinds = [finding["kind"] for finding in self.findings()]
        self.assertIn("unreviewed_proposal", kinds, kinds)
        self.assertTrue(any(node in finding["nodes"] for finding in self.findings() if finding["kind"] == "unreviewed_proposal"))

    def test_a_stale_anchor_is_reported_with_its_source_path(self) -> None:
        node = self.propose("Retries amplify outages", "A budget bounds the blast radius.")
        self.attach(node)
        self.accepted_page(node)
        self.source.unlink()
        run_json(self.workspace, "relocate", check=False)
        findings = [finding for finding in self.findings() if finding["kind"] == "stale_anchor"]
        self.assertTrue(findings, f"expected a stale_anchor finding: {[f['kind'] for f in self.findings()]}")
        self.assertTrue(any((finding["sourcePath"] or "").endswith("retry.md") for finding in findings), findings)

    def test_sources_lists_the_raw_source_and_marks_it_after_a_move(self) -> None:
        self.propose("Retries amplify outages", "A budget bounds the blast radius.")
        run(self.workspace, "index-vault")
        listed = {item["path"]: item for item in self.bridge.request("sources", limit=100)["sources"]}
        self.assertIn("raw/sources/retry.md", listed)
        entry = listed["raw/sources/retry.md"]
        self.assertEqual(entry["bytes"], self.source.stat().st_size)
        self.assertFalse(entry["stale"])

    def test_an_index_budget_reports_the_incomplete_scan_instead_of_hiding_it(self) -> None:
        for index in range(5):
            page = self.workspace / f"wiki/page-{index}.md"
            page.parent.mkdir(parents=True, exist_ok=True)
            page.write_text(f"---\nid: page_{index}\ntitle: Page {index}\nsummary: Page {index}.\n---\n# Page {index}\n", encoding="utf-8")
        report = run_json(self.workspace, "index", "--max-files", "1")
        self.assertFalse(report["scanComplete"], "a budgeted scan must not claim to be complete")  # type: ignore[index]
        skipped = report["skipped"]  # type: ignore[index]
        self.assertTrue(skipped, "an incomplete scan must name what it skipped")
        self.assertTrue(all(entry.get("path") and entry.get("reason") for entry in skipped), skipped)
        # The same doubt must reach the review surface, not only the scan report.
        kinds = [finding["kind"] for finding in self.findings()]
        self.assertIn("incomplete_scan", kinds, kinds)

    def test_two_pages_that_say_the_same_thing_are_a_duplicate_finding(self) -> None:
        first = self.propose("Retry budget three", "Three retries.")
        self.attach(first)
        self.accepted_page(first)
        wiki = self.workspace / "wiki"
        (wiki / "retry-budget-three.md").write_text(
            "---\nid: retry_budget_three\ntitle: Retry budget is 3\nsummary: Three retries, then fail.\n---\n"
            "# Retry budget is 3\n\nThree retries, then fail.\n",
            encoding="utf-8",
        )
        run(self.workspace, "index-vault")
        kinds = [finding["kind"] for finding in self.findings()]
        self.assertTrue(
            {"duplicate", "near_duplicate"} & set(kinds),
            f"two accepted pages that say the same thing must be reported, got {kinds}",
        )

    def test_review_kinds_and_severities_are_the_published_vocabulary(self) -> None:
        published = vocabulary()
        for finding in self.findings():
            self.assertIn(finding["kind"], published["reviewKinds"], finding)
            self.assertIn(finding["severity"], published["reviewSeverities"], finding)


if __name__ == "__main__":
    unittest.main()
