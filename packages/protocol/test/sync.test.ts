import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { describe, it } from 'node:test'
import {
  API_PREFIX, API_VERSION, DEFAULT_SYNC_LIMITS, MANIFEST_FILE, PORTABLE_FORMAT_VERSION, REPOSITORY_ID_PATTERN,
  REPOSITORY_ID_SOURCE, SYNC_ERROR_CODES, SYNC_EXTENSIONS, SYNC_PROTOCOL_VERSION, canonicalManifest, canonicalRevision,
  isCompatibleServer, isRepositoryId, isRevisionId, isSyncPath, validateSyncContent, validateSyncPath,
} from '../src/index.js'
import type {
  ApiErrorEnvelope, Change, CreateRepository, ManifestEntry, PushRequest, RepositoryManifest, RevisionFields, ServerInfo,
} from '../src/index.js'
import { fixture } from './fixtures.ts'
import { any, arrayOf, assertShape, int, lit, nullable, object, oneOf, optional, recordOf, str } from './shape.ts'

const sha256 = (text: string) => createHash('sha256').update(text, 'utf8').digest('hex')
const bytes = (text: string) => new TextEncoder().encode(text)

describe('sync v1 constants', () => {
  it('every protocol version is 1 and the wire names are stable', () => {
    assert.equal(API_VERSION, 1)
    assert.equal(SYNC_PROTOCOL_VERSION, 1)
    assert.equal(PORTABLE_FORMAT_VERSION, 1)
    assert.equal(API_PREFIX, '/api/v1')
    assert.equal(MANIFEST_FILE, 'athenaeum.json')
    assert.deepEqual([...SYNC_EXTENSIONS], ['md'])
    assert.deepEqual({ ...DEFAULT_SYNC_LIMITS }, { maxFileBytes: 1048576, maxRepositoryBytes: 52428800 })
  })

  it('only ever adds sync error codes (v1 additive rule)', () => {
    const v1 = ['invalid_request', 'not_found', 'request_too_large', 'unsupported_protocol', 'unauthorized', 'forbidden', 'stale_base', 'conflict', 'invalid_path', 'invalid_content', 'limit_exceeded', 'rate_limited']
    for (const code of v1) assert.ok((SYNC_ERROR_CODES as readonly string[]).includes(code), code)
    assert.equal(new Set(SYNC_ERROR_CODES).size, SYNC_ERROR_CODES.length)
  })
})

describe('path safety vectors (fixtures/sync/v1/paths.json)', () => {
  const { valid, invalid } = fixture<{ valid: string[]; invalid: { path: string; why: string }[] }>('sync/v1/paths.json')

  for (const path of valid) {
    it(`accepts ${JSON.stringify(path).slice(0, 60)}`, () => {
      assert.equal(validateSyncPath(path), null)
      assert.equal(isSyncPath(path), true)
    })
  }
  for (const { path, why } of invalid) {
    it(`refuses ${JSON.stringify(path).slice(0, 60)} (${why})`, () => {
      assert.equal(validateSyncPath(path), 'invalid_path')
      assert.equal(isSyncPath(path), false)
    })
  }

  it('lone surrogates are refused', () => {
    assert.equal(validateSyncPath('wiki/\ud800.md'), 'invalid_path')
    assert.equal(validateSyncPath('wiki/\udc00.md'), 'invalid_path')
  })
  it('1024 bytes pass, 1025 do not; 255-byte segments pass, 256 do not', () => {
    const longPath = (last: number) => [...Array(5).fill('x'.repeat(200)), 'y'.repeat(last)].join('/')
    assert.equal(longPath(19).length, 1024)
    assert.equal(validateSyncPath(longPath(19)), null)
    assert.equal(validateSyncPath(longPath(20)), 'invalid_path')
    assert.equal(validateSyncPath('a'.repeat(252) + '.md'), null)
    assert.equal(validateSyncPath('a'.repeat(253) + '.md'), 'invalid_path')
    assert.equal(validateSyncPath('é'.repeat(128)), 'invalid_path')
  })
})

describe('extension vectors (fixtures/sync/v1/extensions.json)', () => {
  const { allowed, refused } = fixture<{ allowed: string[]; refused: { path: string; why: string }[] }>('sync/v1/extensions.json')
  for (const path of allowed) it(`stores ${path}`, () => assert.equal(isSyncPath(path), true))
  for (const { path, why } of refused) it(`does not store ${path} (${why})`, () => assert.equal(isSyncPath(path), false))
  it('nothing under a dot-directory is ever storable, which keeps .athenaeum/** out', () => {
    for (const path of ['.athenaeum/knowledge.db', '.athenaeum/cache/a.json', 'wiki/.athenaeum/x.md', '.git/config.toml']) {
      assert.equal(isSyncPath(path), false, path)
    }
  })
})

describe('content vectors', () => {
  it('accepts UTF-8 text up to the limit', () => {
    assert.equal(validateSyncContent(bytes('# héllo 日本語\n')), null)
    assert.equal(validateSyncContent(new Uint8Array(0)), null)
    assert.equal(validateSyncContent(new Uint8Array(DEFAULT_SYNC_LIMITS.maxFileBytes).fill(97)), null)
  })
  it('refuses NUL, invalid UTF-8 and oversize', () => {
    assert.equal(validateSyncContent(Uint8Array.of(0x61, 0, 0x62)), 'invalid_content')
    assert.equal(validateSyncContent(Uint8Array.of(0xff, 0xfe)), 'invalid_content')
    assert.equal(validateSyncContent(Uint8Array.of(0xc3)), 'invalid_content')
    assert.equal(validateSyncContent(new Uint8Array(DEFAULT_SYNC_LIMITS.maxFileBytes + 1).fill(97)), 'invalid_content')
  })
  it('honours caller-supplied limits', () => {
    assert.equal(validateSyncContent(bytes('abcd'), { maxFileBytes: 3, maxRepositoryBytes: 10 }), 'invalid_content')
    assert.equal(validateSyncContent(bytes('abc'), { maxFileBytes: 3, maxRepositoryBytes: 10 }), null)
  })
})

describe('canonicalManifest and treeHash vectors (fixtures/sync/v1/manifests.json)', () => {
  const { cases } = fixture<{ cases: { name: string; entries: ManifestEntry[]; canonical: string; treeHash: string }[] }>('sync/v1/manifests.json')
  assert.ok(cases.length >= 5)
  for (const c of cases) {
    it(`${c.name}: canonical form and tree hash`, () => {
      assert.equal(canonicalManifest(c.entries), c.canonical)
      assert.equal(sha256(canonicalManifest(c.entries)), c.treeHash)
    })
  }
  it('sorting is by UTF-8 bytes, not UTF-16 code units', () => {
    const entries = [{ path: '￿.md', sha256: 'a', size: 1 }, { path: '\u{10000}.md', sha256: 'b', size: 1 }]
    // U+FFFF (EF BF BF) sorts before U+10000 (F0 90 80 80) in UTF-8; UTF-16 would put the surrogate pair first.
    assert.deepEqual(JSON.parse(canonicalManifest(entries)).map((e: ManifestEntry) => e.path), ['￿.md', '\u{10000}.md'])
  })
  it('keys are emitted path, sha256, size and extras are dropped', () => {
    assert.equal(canonicalManifest([{ size: 1, sha256: 'x', path: 'a.md', extra: true } as ManifestEntry]), '[{"path":"a.md","sha256":"x","size":1}]')
  })
})

describe('revision id vectors (fixtures/sync/v1/revisions.json)', () => {
  const { cases } = fixture<{ cases: { name: string; fields: RevisionFields; canonical: string; revisionId: string }[] }>('sync/v1/revisions.json')
  for (const c of cases) {
    it(`${c.name}`, () => {
      assert.equal(canonicalRevision(c.fields), c.canonical)
      assert.equal(`rev_${sha256(c.canonical)}`, c.revisionId)
      assert.ok(isRevisionId(c.revisionId))
    })
  }
  it('the id pattern is rev_ plus 64 lowercase hex digits', () => {
    assert.equal(isRevisionId(`rev_${'a'.repeat(64)}`), true)
    for (const bad of ['', 'rev_', `rev_${'A'.repeat(64)}`, `rev_${'a'.repeat(63)}`, `rev_${'a'.repeat(65)}`, `abc_${'a'.repeat(64)}`]) assert.equal(isRevisionId(bad), false, bad)
  })
})

describe('wire fixtures (fixtures/sync/v1/wire.json)', () => {
  const wire = fixture<any>('sync/v1/wire.json')

  it('serverInfo: a compatible v1 server that names itself athenaeum-api', () => {
    assertShape(object<ServerInfo>({
      service: lit('athenaeum-api'), apiVersion: int, syncProtocol: int,
      limits: optional(object({ maxFileBytes: int, maxRepositoryBytes: int })),
    }), wire.serverInfo, 'serverInfo')
    assert.equal(isCompatibleServer(wire.serverInfo), true)
    assert.equal(isCompatibleServer({ ...wire.serverInfo, apiVersion: 2 }), false)
    assert.equal(isCompatibleServer({ ...wire.serverInfo, syncProtocol: 2 }), false)
    assert.deepEqual(wire.serverInfo.limits, { ...DEFAULT_SYNC_LIMITS })
  })

  it('manifest: has the required keys and a supported format', () => {
    // RepositoryManifest carries an index signature (unknown keys must survive a rewrite), so check by hand.
    const manifest: RepositoryManifest = wire.manifest
    assert.equal(manifest.formatVersion <= PORTABLE_FORMAT_VERSION, true)
    assert.ok(isRepositoryId(manifest.repositoryId))
    assert.equal(typeof manifest.name, 'string')
    assert.match(manifest.createdAt, /^\d{4}-\d\d-\d\dT/)
  })

  it('repository ids: valid and invalid vectors, and the HTML-pattern source', () => {
    for (const id of wire.repositoryIds.valid) assert.equal(isRepositoryId(id), true, id)
    for (const id of wire.repositoryIds.invalid) assert.equal(isRepositoryId(id), false, id)
    assert.equal(REPOSITORY_ID_PATTERN.source, `^${REPOSITORY_ID_SOURCE}$`)
    assert.equal(REPOSITORY_ID_SOURCE, 'repo_[A-Za-z0-9_-]{1,60}')
  })

  it('createRepository: an exact requested id is omitted rather than null when absent', () => {
    const create: CreateRepository = wire.createRepository
    assertShape(object<CreateRepository>({ id: optional(str), name: str, visibility: oneOf(['private', 'public']), description: optional(str) }), create, 'createRepository')
    assert.ok(isRepositoryId(create.id!))
    const withoutId: CreateRepository = wire.createRepositoryWithoutId
    assert.equal('id' in withoutId, false)
    assertShape(object<CreateRepository>({ id: optional(str), name: str, visibility: oneOf(['private', 'public']), description: optional(str) }), withoutId, 'createRepositoryWithoutId')
  })

  it('push: every path is storable, deletes carry no content, writes carry valid content', () => {
    const push: PushRequest = wire.push
    assertShape(object<PushRequest>({
      base: nullable(str), message: str, mergedFrom: optional(str),
      changes: arrayOf((value, at) => {
        const change = value as Change
        return change.delete
          ? object<{ path: string; delete: true }>({ path: str, delete: lit(true) })(value, at)
          : object<{ path: string; content: string }>({ path: str, content: str })(value, at)
      }),
    }), push, 'push')
    for (const change of push.changes) {
      assert.equal(isSyncPath(change.path), true, change.path)
      if (change.delete) assert.equal('content' in change, false)
      else assert.equal(validateSyncContent(bytes(change.content)), null)
    }
  })

  it('staleBase: a 409 envelope in the shared error shape with the current head', () => {
    const stale: ApiErrorEnvelope = wire.staleBase
    assertShape(object<ApiErrorEnvelope>({
      error: object<ApiErrorEnvelope['error']>({ code: lit('stale_base'), message: str, details: optional(recordOf(any)) }),
      requestId: optional(str),
    }), stale, 'staleBase')
    assert.ok((SYNC_ERROR_CODES as readonly string[]).includes(stale.error.code))
    assert.equal(typeof stale.error.details?.head, 'string')
  })

})
