import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { projectSelection } from './project-context.mjs'
import { recallContext } from './mcp.mjs'

test('empty projects, actual manifests and explicit overrides select different guidance', async t => {
  const root = await mkdtemp(join(tmpdir(), 'athenaeum-selection-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  assert.equal((await projectSelection(root, 'Build the app')).stage, 'initialize')
  await mkdir(join(root, 'src'))
  await writeFile(join(root, 'package.json'), JSON.stringify({ dependencies: { react: '19', next: '16' } }))
  await writeFile(join(root, 'tsconfig.json'), '{}')
  const selected = await projectSelection(root, 'Fix the crash')
  assert.equal(selected.project, root.replaceAll('\\', '/'))
  assert.equal(selected.stage, 'debugging')
  assert.deepEqual(selected.technologies, ['typescript', 'react', 'next'])
  assert.equal((await projectSelection(root, 'Write a feature')).stage, 'coding')
  assert.equal((await projectSelection(root, 'Test the CLI')).stage, 'verification')
  assert.equal((await projectSelection(root, 'Fix the crash', { project: 'stable-id', stage: 'design' })).stage, 'design')
  await assert.rejects(projectSelection(root, 'Build', { stage: 'invalid' }), /valid memory task stage/)
})

test('selector support is negotiated and changing stage invalidates negative recall', async () => {
  const requests = []
  const connection = { tools: [{ name: 'wiki_context', parameters: { properties: { selection: {} } } }],
    handlers: new Map([['wiki_context', { tool: 'wiki_context', approval: false, client: { request: async (_, params) => {
      requests.push(params.arguments)
      return { structuredContent: { brief: { nodes: [], warnings: [] } } }
    } } }]]) }
  const signal = new AbortController().signal
  const selection = { project: 'a', technologies: ['react'], stage: 'initialize' }
  const first = await recallContext(connection, 'Build the app', signal, { selection })
  assert.deepEqual(requests[0].selection, selection)
  await recallContext(connection, 'Build the app', signal, { selection: { ...selection, stage: 'coding' }, previous: first.diagnostic })
  assert.equal(requests.length, 2)
  connection.tools = []
  await recallContext(connection, 'Build the app', signal, { selection })
  assert.equal(requests[2].selection, undefined)
})
