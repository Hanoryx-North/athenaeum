import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'vitest'
import { boundaryError, importSpecifiers, unusedFrontendFiles } from './check-boundaries.mjs'

test('source layers communicate through public protocol instead of importing implementations', () => {
  assert.match(boundaryError('src/lib/feature.ts', '../../packages/integrations/src/client/client.ts'), /versioned protocol/)
  assert.match(boundaryError('packages/agent/runtime/src/agent.mjs', '../../../integrations/src/client/client.ts'), /versioned protocol/)
  assert.match(boundaryError('packages/agent/runtime/src/agent.mjs', 'some-sdk'), /dependency-free/)
  assert.match(boundaryError('packages/agent/runtime/src/agent.mjs', '@athenaeum/protocol'), /dependency-free/)
  assert.match(boundaryError('src/lib/feature.ts', 'athenaeum-agent'), /versioned protocol/)
  for (const [file, specifier] of [['src/lib/core-bridge.ts', '@athenaeum/protocol/bridge'], ['packages/agent/runtime/src/agent.mjs', './tools.mjs'], ['packages/integrations/src/client/client.ts', './validate.ts'], ['src/components/example.tsx', '@/lib/engine']]) assert.equal(boundaryError(file, specifier), null)
})

test('static imports, reexports and dynamic imports are checked without matching prose', () => {
  const imports = importSpecifiers('src/example.ts', `import x from './one'; export { y } from './two'; const z = import('./three'); const q = require('./four'); const prose = "import './fake'";`)
  assert.deepEqual(imports.map(item => item.specifier), ['./one', './two', './three', './four'])
})

test('unused modules fail even when tests or an orphan cycle import them; services and workers remain reachable', async () => {
  const root = await mkdtemp(join(tmpdir(), 'athenaeum-reachability-'))
  try {
    await mkdir(join(root, 'src/lib'), { recursive: true })
    await mkdir(join(root, 'src/stores'))
    const files = {
      'tsconfig.app.json': JSON.stringify({ compilerOptions: { moduleResolution: 'bundler', module: 'esnext', baseUrl: '.', paths: { '@/*': ['./src/*'] } }, include: ['src'] }),
      'src/main.tsx': `import '@/lib/active'; new Worker(new URL('./worker.ts', import.meta.url)); import('./lazy')`,
      'src/lib/active.ts': 'export const active = true',
      'src/worker.ts': 'export const worker = true',
      'src/lazy.ts': 'export const lazy = true',
      'src/service.ts': 'export const service = true',
      'src/stores/public-store.ts': 'export const store = true',
      'src/unused.ts': `import './cycle'; export const unused = true`,
      'src/cycle.ts': `import './unused'`,
      'src/unused.test.ts': `import './unused'`,
      'src/env.d.ts': 'declare const environment: string',
    }
    await Promise.all(Object.entries(files).map(([file, source]) => writeFile(join(root, file), source)))
    assert.deepEqual(await unusedFrontendFiles(root, ['service']), ['src/cycle.ts', 'src/unused.ts'])
  } finally { await rm(root, { recursive: true, force: true }) }
})
