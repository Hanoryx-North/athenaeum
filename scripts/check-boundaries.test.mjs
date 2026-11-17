import assert from 'node:assert/strict'
import { test } from 'vitest'
import { boundaryError, importSpecifiers } from './check-boundaries.mjs'

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
