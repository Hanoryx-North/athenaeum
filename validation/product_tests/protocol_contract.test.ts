/** Shared wire fixtures and path/hash rules consumed by the local core and API. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { bridgeRequest, BRIDGE_METHODS, BRIDGE_PROTOCOL_VERSION } from '../../packages/protocol/src/bridge.js';
import {
  API_PREFIX, API_VERSION, MANIFEST_FILE, REPOSITORY_ID_PATTERN, SYNC_PROTOCOL_VERSION,
  canonicalManifest, isRepositoryId, isSyncPath, validateSyncContent, validateSyncPath,
} from '../../packages/protocol/src/sync.js';

const fixture = (name: string) => JSON.parse(readFileSync(
  fileURLToPath(new URL(`../../packages/protocol/fixtures/${name}`, import.meta.url)), 'utf8',
)) as Record<string, any>;

test('all pinned bridge request methods have a v1 envelope', () => {
  const requests = fixture('bridge/v1/requests.json');
  const portable = fixture('bridge/v1/portable-requests.json');
  for (const method of BRIDGE_METHODS) {
    const wire = requests[method] ?? portable[method];
    assert.ok(wire, `missing fixture for ${method}`);
    assert.equal(wire.protocolVersion, BRIDGE_PROTOCOL_VERSION);
    assert.equal(wire.request.method, method);
    assert.deepEqual(bridgeRequest(wire.request, wire.requestId), wire);
  }
});

test('sync v1 agrees on path safety and canonical tree hashing', () => {
  const wire = fixture('sync/v1/wire.json');
  assert.equal(API_PREFIX, '/api/v1');
  assert.equal(API_VERSION, 1);
  assert.equal(SYNC_PROTOCOL_VERSION, 1);
  // Safe syntax is wider than the sync policy: a raw source is a legal relative path but is
  // deliberately not shared. Root Markdown documentation, wiki pages and the manifest sync.
  for (const path of ['wiki/decision.md', 'raw/sources/source.txt', MANIFEST_FILE]) {
    assert.equal(validateSyncPath(path), null);
  }
  for (const path of ['wiki/decision.md', 'index.md', 'README.md', MANIFEST_FILE]) assert.equal(isSyncPath(path), true, path);
  for (const path of ['raw/sources/source.txt', 'docs/private.md', 'wiki/notes.txt']) {
    assert.equal(isSyncPath(path), false, path);
  }
  for (const path of ['../secret.md', '.athenaeum/knowledge.db.json', '/absolute.md', 'wiki\\x.md']) {
    assert.equal(validateSyncPath(path), 'invalid_path');
    assert.equal(isSyncPath(path), false);
  }
  assert.equal(validateSyncContent(new TextEncoder().encode('# Knowledge\n')), null);
  assert.equal(validateSyncContent(new Uint8Array([0xff])), 'invalid_content');
  const content = wire.push.changes[0].content as string;
  const entry = { path: wire.push.changes[0].path as string,
    sha256: createHash('sha256').update(content).digest('hex'), size: Buffer.byteLength(content) };
  assert.equal(canonicalManifest([entry]), JSON.stringify([entry]));
  assert.equal(createHash('sha256').update(canonicalManifest([entry])).digest('hex').length, 64);
});

test('a repository id is one value for the manifest, the CLI and the hosted API', () => {
  const wire = fixture('sync/v1/wire.json');
  for (const id of wire.repositoryIds.valid as string[]) assert.equal(isRepositoryId(id), true, id);
  for (const id of wire.repositoryIds.invalid as string[]) assert.equal(isRepositoryId(id), false, id);
  assert.equal(REPOSITORY_ID_PATTERN.test(wire.manifest.repositoryId), true);
  assert.equal(REPOSITORY_ID_PATTERN.test(wire.createRepository.id), true);
  // A repository created without a suggestion omits the key: the API rejects `null`, it does not read it as absent.
  assert.equal('id' in wire.createRepositoryWithoutId, false);
});
