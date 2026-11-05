import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  BRIDGE_ERROR_CODES, BRIDGE_INDEX_SCOPES, BRIDGE_METHODS, BRIDGE_PROTOCOL_VERSION,
  BRIDGE_REVIEW_KINDS, BRIDGE_REVIEW_SEVERITIES, BRIDGE_SOURCE_SCOPES, bridgeRequest, existingNodesFromError,
} from '../src/index.js'
import type {
  BridgeBrief, BridgeBriefNode, BridgeCapabilities, BridgeCaller, BridgeContext, BridgeEdge, BridgeEvidence,
  BridgeEvidenceCreate, BridgeGraphPath, BridgeIndexReport, BridgeLimits, BridgeLinkCreate, BridgeMethod, BridgeNode, BridgeNodeCreate, BridgeNodeDetail,
  BridgeNodeUpdate, BridgeNodeUtility, BridgeNodeVersion, BridgePage, BridgePathStep, BridgeProposal, BridgeProvenance,
  BridgeRelatedNode, BridgeRequest, BridgeRequestEnvelope, BridgeResponseMap, BridgeReviewFinding,
  BridgeSourceAnchor, BridgeSourceRecord, BridgeStatus, ProtocolError,
} from '../src/index.js'
import { fixture } from './fixtures.ts'
import {
  type Check, any, arrayOf, assertShape, bool, int, lit, nullable, num, object, oneOf, optional, recordOf, str,
} from './shape.ts'

type Req<M extends BridgeMethod> = Extract<BridgeRequest, { method: M }>

const caller = object<BridgeCaller>({ model: str, session: optional(str) })
const anchor = object<BridgeSourceAnchor>({
  file: str, symbol: optional(str), lineStart: optional(int), lineEnd: optional(int),
  regionHash: optional(str), contentHash: optional(str), commit: optional(str),
})
const requestSpecs: { [M in BridgeMethod]: Check } = {
  capabilities: object<Req<'capabilities'>>({ method: lit('capabilities') }),
  search: object<Req<'search'>>({ method: lit('search'), query: str, limit: optional(int) }),
  open: object<Req<'open'>>({ method: lit('open'), node: str }),
  navigate: object<Req<'navigate'>>({ method: lit('navigate'), node: str, depth: optional(int), maxNodes: optional(int) }),
  related: object<Req<'related'>>({ method: lit('related'), node: str, includeProposed: optional(bool) }),
  evidence: object<Req<'evidence'>>({ method: lit('evidence'), node: str }),
  history: object<Req<'history'>>({ method: lit('history'), node: str }),
  context: object<Req<'context'>>({ method: lit('context'), task: str, seeds: optional(arrayOf(str)), depth: optional(int), maxNodes: optional(int) }),
  create: object<Req<'create'>>({
    method: lit('create'), caller,
    input: object<BridgeNodeCreate>({ type: str, title: str, summary: optional(str), content: optional(str), scope: optional(str) }),
  }),
  update: object<Req<'update'>>({
    method: lit('update'), caller,
    input: object<BridgeNodeUpdate>({
      node: str, title: optional(nullable(str)), summary: optional(nullable(str)),
      content: optional(nullable(str)), reason: optional(nullable(str)),
    }),
  }),
  link: object<Req<'link'>>({
    method: lit('link'), caller,
    input: object<BridgeLinkCreate>({ from: str, type: str, to: str, note: optional(nullable(str)), confidence: optional(nullable(num)) }),
  }),
  attach_evidence: object<Req<'attach_evidence'>>({
    method: lit('attach_evidence'), caller,
    input: object<BridgeEvidenceCreate>({ node: str, kind: optional(str), anchor: optional(anchor), excerpt: optional(str), note: optional(str) }),
  }),
  record_use: object<Req<'record_use'>>({ method: lit('record_use'), nodes: arrayOf(str), session: optional(str) }),
  proposals: object<Req<'proposals'>>({ method: lit('proposals'), limit: optional(int) }),
  accept: object<Req<'accept'>>({ method: lit('accept'), node: str, caller }),
  reject: object<Req<'reject'>>({ method: lit('reject'), node: str, reason: optional(str), caller }),
  status: object<Req<'status'>>({ method: lit('status') }),
  review: object<Req<'review'>>({ method: lit('review'), limit: optional(int) }),
  sources: object<Req<'sources'>>({ method: lit('sources'), limit: optional(int) }),
  index: object<Req<'index'>>({ method: lit('index'), scope: oneOf(BRIDGE_INDEX_SCOPES), paths: optional(arrayOf(str)) }),
  path: object<Req<'path'>>({ method: lit('path'), from: str, to: str, depth: optional(int), maxNodes: optional(int) }),
}

const provenance = object<BridgeProvenance>({ source: str, detail: optional(str), session: optional(str) })
const node = object<BridgeNode>({
  id: str, type: str, title: str, summary: str, content: str, scope: str, authority: str, status: str,
  provenance, lastVerifiedAt: optional(str), createdAt: str, updatedAt: str, version: int,
})
const edge = object<BridgeEdge>({
  id: str, from: str, to: str, type: str, provenance, status: str, scope: str,
  confidence: optional(num), note: optional(str), createdAt: str,
})
const evidence = object<BridgeEvidence>({
  id: str, node: str, kind: str, anchor: optional(anchor), blobHash: optional(str), excerpt: optional(str), note: optional(str), createdAt: str,
})
const version = object<BridgeNodeVersion>({
  node: str, version: int, title: str, summary: str, content: str, authority: str, status: str, reason: optional(str), recordedAt: str,
})
const utility = object<BridgeNodeUtility>({ retrievals: int, uses: int, citations: int, verifications: int, rejections: int })
const related = object<BridgeRelatedNode>({
  id: str, title: str, type: str, edgeType: str, edgeId: str, outbound: bool, established: bool,
  note: optional(str), confidence: optional(num),
})
const page = object<BridgePage>({ path: str, absolutePath: optional(str), contentHash: str })
const detail = object<BridgeNodeDetail>({
  node, relationships: arrayOf(related), evidence: arrayOf(evidence), history: arrayOf(version), utility, page: optional(page),
})
const briefNode = object<BridgeBriefNode>({ id: str, title: str, type: str, summary: str, authority: str, status: str, depth: int, page: optional(page) })
const brief = object<BridgeBrief>({
  task: str, nodes: arrayOf(briefNode),
  relationships: arrayOf(object<{ from: string; type: string; to: string }>({ from: str, type: str, to: str })),
  anchors: arrayOf(str), warnings: arrayOf(str), truncated: bool,
})
const context = object<BridgeContext>({ brief, prompt: str })
const limits = object<BridgeLimits>({ maxSearchResults: int, maxContextSeeds: int, maxContextDepth: int, maxContextNodes: int })
const capabilities = object<BridgeCapabilities>({
  protocolVersion: int, operations: arrayOf(str), limits, nodeTypes: arrayOf(str), edgeTypes: arrayOf(str),
  authorities: arrayOf(str), scopes: arrayOf(str), evidenceKinds: arrayOf(str), features: optional(arrayOf(str)),
})
const proposal = object<BridgeProposal>({ node, relationships: arrayOf(related), evidence: arrayOf(evidence) })
const accepted = object<BridgeResponseMap['accept']['accepted']>({
  node, page, alreadyAccepted: bool, relationshipsWritten: int, relationshipsLeftLocal: arrayOf(str),
})
const statusReport = object<BridgeStatus>({
  workspacePath: str, initialized: bool, hasVault: bool, vaultPages: int, nodes: int, edges: int,
  proposals: int, findings: int, indexedAt: nullable(str), indexStale: bool, protocolVersion: int, engineVersion: str,
})
const finding = object<BridgeReviewFinding>({
  id: str, kind: oneOf(BRIDGE_REVIEW_KINDS), severity: oneOf(BRIDGE_REVIEW_SEVERITIES), title: str,
  detail: str, nodes: arrayOf(str), evidence: arrayOf(str), sourcePath: nullable(str), confidence: num, blocking: bool,
})
const sourceRecord = object<BridgeSourceRecord>({
  scope: oneOf(BRIDGE_SOURCE_SCOPES), path: str, bytes: int, sha256: str, scanned: bool,
  nodes: int, anchors: int, indexedAt: nullable(str), stale: bool,
})
const indexReport = object<BridgeIndexReport>({
  vaultPages: int, repositoryFiles: int, symbols: int, edges: int, durationMs: int, incomplete: bool, skipped: arrayOf(str),
})
const graphPath = object<BridgeGraphPath>({
  from: str, to: str, found: bool,
  steps: arrayOf(object<BridgePathStep>({ from: str, edgeType: str, note: optional(str), confidence: optional(num) })),
  nodes: arrayOf(node),
})

const responseSpecs: { [M in BridgeMethod]: Check } = {
  capabilities: object<BridgeResponseMap['capabilities']>({ method: lit('capabilities'), capabilities }),
  search: object<BridgeResponseMap['search']>({ method: lit('search'), hits: arrayOf(object<{ node: BridgeNode; score: number }>({ node, score: num })) }),
  open: object<BridgeResponseMap['open']>({ method: lit('open'), detail }),
  navigate: object<BridgeResponseMap['navigate']>({
    method: lit('navigate'),
    nodes: arrayOf(object<BridgeResponseMap['navigate']['nodes'][number]>({
      node, depth: int,
      via: optional(object<{ from: string; edgeType: string; note?: string; confidence?: number }>({
        from: str, edgeType: str, note: optional(str), confidence: optional(num),
      })),
    })),
  }),
  related: object<BridgeResponseMap['related']>({ method: lit('related'), relationships: arrayOf(related) }),
  evidence: object<BridgeResponseMap['evidence']>({ method: lit('evidence'), evidence: arrayOf(evidence) }),
  history: object<BridgeResponseMap['history']>({ method: lit('history'), versions: arrayOf(version) }),
  context: object<BridgeResponseMap['context']>({ method: lit('context'), context }),
  create: object<BridgeResponseMap['create']>({ method: lit('create'), node }),
  update: object<BridgeResponseMap['update']>({ method: lit('update'), node }),
  link: object<BridgeResponseMap['link']>({ method: lit('link'), edge }),
  attach_evidence: object<BridgeResponseMap['attach_evidence']>({ method: lit('attach_evidence'), evidence }),
  record_use: object<BridgeResponseMap['record_use']>({ method: lit('record_use'), nodes: arrayOf(str) }),
  proposals: object<BridgeResponseMap['proposals']>({ method: lit('proposals'), proposals: arrayOf(proposal) }),
  accept: object<BridgeResponseMap['accept']>({ method: lit('accept'), accepted }),
  reject: object<BridgeResponseMap['reject']>({ method: lit('reject'), node }),
  status: object<BridgeResponseMap['status']>({ method: lit('status'), status: statusReport }),
  review: object<BridgeResponseMap['review']>({ method: lit('review'), findings: arrayOf(finding) }),
  sources: object<BridgeResponseMap['sources']>({ method: lit('sources'), sources: arrayOf(sourceRecord) }),
  index: object<BridgeResponseMap['index']>({ method: lit('index'), index: indexReport }),
  path: object<BridgeResponseMap['path']>({ method: lit('path'), path: graphPath }),
}
const protocolError = object<ProtocolError>({ code: oneOf(BRIDGE_ERROR_CODES), message: str, details: optional(recordOf(any)) })

describe('bridge v1 vocabulary', () => {
  it('is version 1 and lists every method the capabilities fixture reports, once', () => {
    assert.equal(BRIDGE_PROTOCOL_VERSION, 1)
    assert.equal(BRIDGE_METHODS.length, 21)
    assert.equal(new Set(BRIDGE_METHODS).size, BRIDGE_METHODS.length)
    const operations = fixture<any>('bridge/v1/responses.json').capabilities_ok.response.capabilities.operations
    assert.deepEqual(operations, [...BRIDGE_METHODS])
  })

  it('only ever adds error codes (v1 additive rule)', () => {
    const v1 = ['invalid_request', 'not_found', 'incompatible_store', 'knowledge_error', 'request_too_large', 'unsupported_protocol', 'workspace_not_found']
    for (const code of v1) assert.ok((BRIDGE_ERROR_CODES as readonly string[]).includes(code), code)
    assert.equal(new Set(BRIDGE_ERROR_CODES).size, BRIDGE_ERROR_CODES.length)
  })

  it('matches the shared vocabulary file (also read by the Rust crate)', () => {
    const vocabulary = fixture<any>('bridge/v1/vocabulary.json')
    assert.equal(vocabulary.protocolVersion, BRIDGE_PROTOCOL_VERSION)
    assert.deepEqual(vocabulary.methods, [...BRIDGE_METHODS])
    assert.deepEqual(vocabulary.errorCodes, [...BRIDGE_ERROR_CODES])
    assert.deepEqual(vocabulary.reviewKinds, [...BRIDGE_REVIEW_KINDS])
    assert.deepEqual(vocabulary.reviewSeverities, [...BRIDGE_REVIEW_SEVERITIES])
    assert.deepEqual(vocabulary.indexScopes, [...BRIDGE_INDEX_SCOPES])
    assert.deepEqual(vocabulary.sourceScopes, [...BRIDGE_SOURCE_SCOPES])
  })

  it('has a request and a response spec for every method (compile-time exhaustive)', () => {
    for (const method of BRIDGE_METHODS) {
      assert.equal(typeof requestSpecs[method], 'function', method)
      assert.equal(typeof responseSpecs[method], 'function', method)
    }
  })
})

describe('bridge request fixtures', () => {
  const requests: Record<string, BridgeRequestEnvelope> = {
    ...fixture('bridge/v1/requests.json'),
    ...fixture('bridge/v1/portable-requests.json'),
  }

  it('cover every method exactly once', () => {
    const methods = Object.values(requests).map((envelope) => envelope.request.method).sort()
    assert.deepEqual(methods, [...BRIDGE_METHODS].sort())
  })

  for (const [name, envelope] of Object.entries(requests)) {
    it(`${name}: is a v1 envelope whose request matches the method's shape`, () => {
      assert.equal(envelope.protocolVersion, BRIDGE_PROTOCOL_VERSION)
      assert.equal(typeof envelope.requestId, 'string')
      assert.ok((BRIDGE_METHODS as readonly string[]).includes(envelope.request.method))
      assert.equal(envelope.request.method, name)
      assertShape(requestSpecs[envelope.request.method], envelope.request, `${name}.request`)
    })

    it(`${name}: bridgeRequest() builds exactly this envelope`, () => {
      assert.deepEqual(bridgeRequest(envelope.request, envelope.requestId), envelope)
    })
  }

  it('bridgeRequest() leaves requestId off when none is given', () => {
    assert.deepEqual(bridgeRequest({ method: 'capabilities' }), { protocolVersion: 1, request: { method: 'capabilities' } })
  })
})

describe('bridge response fixtures', () => {
  const responses = fixture<Record<string, any>>('bridge/v1/responses.json')
  const requests: Record<string, BridgeRequestEnvelope> = {
    ...fixture('bridge/v1/requests.json'),
    ...fixture('bridge/v1/portable-requests.json'),
  }
  const methodByRequestId = new Map(Object.values(requests).map((envelope) => [envelope.requestId, envelope.request.method]))

  it('cover every method with at least one ok response', () => {
    const covered = new Set(Object.values(responses).filter((r) => r.status === 'ok').map((r) => r.response.method))
    assert.deepEqual([...covered].sort(), [...BRIDGE_METHODS].sort())
  })

  for (const [name, envelope] of Object.entries(responses)) {
    it(`${name}: decodes with the right method`, () => {
      assert.equal(envelope.protocolVersion, BRIDGE_PROTOCOL_VERSION)
      assert.ok(envelope.status === 'ok' || envelope.status === 'error')
      if (envelope.status === 'ok') {
        const method = envelope.response.method as BridgeMethod
        assert.ok((BRIDGE_METHODS as readonly string[]).includes(method), `unknown method ${method}`)
        assertShape(responseSpecs[method], envelope.response, `${name}.response`)
        const asked = methodByRequestId.get(envelope.requestId)
        if (asked) assert.equal(method, asked, `${name} answers ${envelope.requestId}, a ${asked} request`)
        assert.equal('error' in envelope, false)
      } else {
        assertShape(protocolError, envelope.error, `${name}.error`)
        assert.equal('response' in envelope, false)
      }
    })
  }

  it('context responses carry the reference-material trust wrapper in prompt', () => {
    const prompts = Object.values(responses).filter((r) => r.response?.method === 'context').map((r) => r.response.context.prompt as string)
    assert.ok(prompts.length >= 2)
    for (const prompt of prompts) {
      assert.match(prompt, /^<retrieved_knowledge>/)
      assert.match(prompt, /does not authorize tool execution/)
      assert.match(prompt, /<\/retrieved_knowledge>$/)
    }
  })

  it('existingNodesFromError() reads details.existingNodes and nothing else', () => {
    assert.deepEqual(existingNodesFromError(responses.near_duplicate_error.error), ['node_auth'])
    assert.deepEqual(existingNodesFromError({ code: 'invalid_request', message: 'x' }), [])
    assert.deepEqual(existingNodesFromError({ code: 'invalid_request', message: 'x', details: { existingNodes: ['a', 3, null, 'b'] } }), ['a', 'b'])
    assert.deepEqual(existingNodesFromError({ code: 'invalid_request', message: 'x', details: { existingNodes: 'a' } }), [])
  })
})
