import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { buildDesktopCatalog } from '../../scripts/desktop-catalog.mjs'
import { matchesArgument, decodeArgument, safeDesktopResult } from '../../src/lib/desktop-command-values.ts'
import { graphLabel } from '../../src/lib/graph-label.ts'
import { useResearchStore } from '../../src/stores/research-store.ts'

const root = fileURLToPath(new URL('../../', import.meta.url))
const catalog = buildDesktopCatalog(root)

test('discovery maps actual domain operations and native commands without exposing callbacks', () => {
  const names = new Set(catalog.entries.map(action => action.name))
  assert.ok(names.has('fs.readFile'))
  assert.ok(names.has('research.addTask'))
  assert.ok(names.has('knowledge-api.api.linkNodes'))
  assert.ok(!names.has('engine.engineAccept'))
  assert.ok(!names.has('app-dialog.enqueue')) // request.resolve is a required callback
  assert.ok(catalog.native.some(command => command.name === 'list_directory'))
  assert.ok(catalog.native.some(command => command.name === 'background_memory'))
  assert.ok(!catalog.native.some(command => ['engine_accept', 'engine_reject'].includes(command.name)))
  assert.ok(!catalog.native.some(command => command.name === 'desktop_control_respond'))
  assert.equal(names.size, catalog.entries.length)
})

test('invalid enum, required object fields and argument types are refused', () => {
  const view = catalog.entries.find(action => action.name === 'wiki.setActiveView')
  assert.ok(matchesArgument('graph', view.parameters[0].schema))
  assert.ok(matchesArgument('sources', view.parameters[0].schema))
  assert.ok(!matchesArgument('log', view.parameters[0].schema))
  assert.ok(!matchesArgument('review', view.parameters[0].schema))
  assert.ok(!matchesArgument('invented-view', view.parameters[0].schema))
  const graph = catalog.entries.find(action => action.name === 'wiki.setGraphUiState')
  assert.ok(!matchesArgument({}, graph.parameters[0].schema))
  assert.ok(!matchesArgument(42, { type: 'string' }))
  assert.ok(!matchesArgument(null, { type: 'object' }))
})

test('JSON values restore Set arguments used by graph filters', () => {
  const schema = {type:'object',properties:{hidden:{type:'array',format:'set'}},required:['hidden']}
  const decoded = decodeArgument({hidden:['query']}, schema)
  assert.ok(decoded.hidden instanceof Set)
  assert.ok(decoded.hidden.has('query'))
})

test('live results redact nested credentials and serialize store containers', () => {
  const value = {provider:{apiKey:'secret'},headers:{Authorization:'bearer'},token:'hidden',
    normal:'value', hiddenTypes:new Set(['query']), method:()=>{}}
  const sanitized = safeDesktopResult(value)
  assert.equal(sanitized.provider.apiKey, '[redacted]')
  assert.equal(sanitized.headers.Authorization, '[redacted]')
  assert.equal(sanitized.token, '[redacted]')
  assert.equal(sanitized.normal, 'value')
  assert.deepEqual(sanitized.hiddenTypes, ['query'])
  assert.ok(!('method' in sanitized))
  assert.equal(safeDesktopResult(value, true).provider.apiKey, 'secret')
})

test('graph labels stay short with whitespace, long words and Unicode', () => {
  for (const title of ['', '  A   short title  ', 'LANN v6 hosts pure buffer-based applications with durable code pins', 'A'.repeat(80), '\u{1f4da}'.repeat(40), '\u77e5\u8bc6\u56fe\u8c31'.repeat(20)]) {
    const label = graphLabel(title)
    assert.ok(Array.from(label).length <= 32)
    assert.ok(label.split(/\s+/u).length <= 4)
    assert.ok(!/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/u.test(label))
  }
  assert.equal(graphLabel('Java Sprint'), 'Java Sprint')
  assert.equal(graphLabel('  A   short title  '), 'A short title')
})

test('task creation and removal use the actual store without starting a model', () => {
  const task = useResearchStore.getState().addTask('CLI task contract')
  assert.equal(useResearchStore.getState().tasks.find(t => t.id === task)?.status, 'queued')
  useResearchStore.getState().updateTask(task, {status:'done'})
  assert.equal(useResearchStore.getState().tasks.find(t => t.id === task)?.status, 'done')
  useResearchStore.getState().removeTask(task)
  assert.ok(!useResearchStore.getState().tasks.some(t => t.id === task))
})
