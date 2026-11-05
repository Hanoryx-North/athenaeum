/**
 * The desktop's own request builders, driven against the real engine.
 *
 * `athenaeum/src/lib/engine-client.ts` is the desktop's pure half of bridge protocol v1: it builds
 * the request envelopes and parses the answers, with no Tauri import, so the contract can be
 * checked without launching the app. This test imports those builders directly (not a copy of
 * them), sends each envelope to a real `athenaeum bridge`, and asserts both halves:
 *
 *   - every envelope carries protocolVersion 1, the right method and only the fields the method
 *     takes, and the engine answers it;
 *   - `parseEngineResponse` accepts the engine's real reply for the nine methods the knowledge UI
 *     uses, and refuses any other method, so the desktop cannot grow a second knowledge path.
 *
 * Keyless: a temporary vault, a real binary, no window, no model, no network.
 */
import { execFileSync } from "node:child_process"
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { after, before, describe, it } from "node:test"
import assert from "node:assert/strict"

import {
  ENGINE_PROTOCOL_VERSION,
  briefRequest,
  historyRequest,
  indexRequest,
  openRequest,
  parseEngineResponse,
  pathRequest,
  proposalsRequest,
  reviewRequest,
  sourcesRequest,
  statusRequest,
} from "../../src/lib/engine-client.ts"

const BIN = process.env.ATHENAEUM_BIN
const describeWithEngine = BIN ? describe : describe.skip

let workspace = ""

function bridge(envelope: object): { reply: Record<string, unknown>; status: number } {
  const stdout = execFileSync(BIN as string, ["-C", workspace, "bridge"], {
    input: JSON.stringify(envelope),
    encoding: "utf-8",
    maxBuffer: 16 * 1024 * 1024,
    stdio: ["pipe", "pipe", "pipe"],
  })
  return { reply: JSON.parse(stdout), status: 0 }
}

function call(request: { method: string }, id: string) {
  const { reply } = bridge({ protocolVersion: ENGINE_PROTOCOL_VERSION, requestId: id, request })
  return reply
}

describeWithEngine("desktop ↔ engine bridge v1", () => {
  before(() => {
    workspace = mkdtempSync(join(tmpdir(), "athenaeum-desktop-contract-"))
    mkdirSync(join(workspace, "wiki"), { recursive: true })
    writeFileSync(
      join(workspace, "wiki", "state.md"),
      "---\nid: state\ntitle: State check\nsummary: Validate state before a session.\n---\n# State check\n",
      "utf-8",
    )
    execFileSync(BIN as string, ["-C", workspace, "init"], { encoding: "utf-8" })
    execFileSync(BIN as string, ["-C", workspace, "index-vault"], { encoding: "utf-8" })
  })

  after(() => {
    rmSync(workspace, { recursive: true, force: true })
  })

  it("builds envelopes the engine answers, for every method the knowledge UI uses", () => {
    const requests: Array<{ request: { method: string }; answerable: boolean }> = [
      { request: statusRequest(), answerable: true },
      { request: reviewRequest(20), answerable: true },
      { request: sourcesRequest(20), answerable: true },
      { request: indexRequest({ scope: "all" }), answerable: true },
      { request: proposalsRequest(20), answerable: true },
      { request: briefRequest("Fix the state check", { maxNodes: 10 }), answerable: true },
      { request: openRequest("State check"), answerable: true },
      { request: historyRequest("State check"), answerable: true },
      { request: pathRequest("State check", "State check", { maxNodes: 5 }), answerable: true },
    ]
    for (const { request, answerable } of requests) {
      assert.equal((request as { protocolVersion?: number }).protocolVersion, undefined, "a builder returns the request, not the envelope")
      const reply = call(request, `desktop-${request.method}`)
      assert.equal(reply["protocolVersion"], ENGINE_PROTOCOL_VERSION, `${request.method}: protocol version`)
      assert.equal(reply["requestId"], `desktop-${request.method}`, `${request.method}: correlation`)
      if (answerable) {
        assert.equal(reply["status"], "ok", `${request.method} must be answerable: ${JSON.stringify(reply)}`)
      }
    }
  })

  it("parses the engine's own answers into protocol values", () => {
    for (const [request, id] of [
      [statusRequest(), "parse-status"],
      [reviewRequest(20), "parse-review"],
      [sourcesRequest(20), "parse-sources"],
      [indexRequest({ scope: "all" }), "parse-index"],
      [proposalsRequest(20), "parse-proposals"],
      [briefRequest("Fix the state check"), "parse-context"],
      [openRequest("State check"), "parse-open"],
      [historyRequest("State check"), "parse-history"],
      [pathRequest("State check", "State check"), "parse-path"],
    ] as const) {
      const reply = call(request, id) as { status: string; response: Record<string, unknown> }
      assert.equal(reply["status"], "ok", `${id}: ${JSON.stringify(reply)}`)
      const parsed = parseEngineResponse(reply.response)
      assert.ok(parsed, `${id}: the engine's own answer must parse`)
    }
  })

  it("refuses a method outside the knowledge UI's closed vocabulary", () => {
    const reply = call({ method: "capabilities" } as { method: string }, "parse-refused") as {
      status: string
      response: Record<string, unknown>
    }
    assert.equal(reply["status"], "ok")
    assert.throws(
      () => parseEngineResponse(reply.response),
      (error: unknown) => (error as { code?: string }).code === "engine_protocol_error",
      "a method the UI does not own must not parse into a knowledge value",
    )
  })

  it("refuses an answer whose field has drifted, instead of rendering it as knowledge", () => {
    const drifted = { method: "status", status: { ...(call(statusRequest(), "drift") as { response: { status: Record<string, unknown> } }).response.status, edges: "many" } }
    assert.throws(() => parseEngineResponse(drifted), /edges/)
  })
})
