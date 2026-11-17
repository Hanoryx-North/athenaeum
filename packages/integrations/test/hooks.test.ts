import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { AthenaeumClient } from '../src/client/client.ts'
import { runHook } from '../src/hooks/index.ts'

test('automatic hook recall caches empty tasks, isolates workspaces and preserves complete framing', async (t) => {
  const dir = await mkdtemp(path.join(tmpdir(), 'athenaeum-hooks-'))
  const original = AthenaeumClient.prototype.context
  t.after(async () => { AthenaeumClient.prototype.context = original; await rm(dir, { recursive: true, force: true }) })
  const calls: unknown[] = []
  let nodes: unknown[] = [], prompt = '<retrieved_knowledge>Complete reference and warnings</retrieved_knowledge>'
  AthenaeumClient.prototype.context = async (args) => {
    calls.push(args)
    return { brief: { nodes, warnings: [] }, prompt } as any
  }
  const options = { event: 'user-prompt' as const, workspace: path.join(dir, 'project-a'), stateDir: dir,
    stdin: JSON.stringify({ session_id: 'session', prompt: 'Debug OAuth callback state validation' }) }
  for (let index = 0; index < 5; index++) assert.equal((await runHook(options)).stdout, '')
  assert.equal(calls.length, 1)
  assert.deepEqual(calls[0], { task: 'Debug OAuth callback state validation', depth: 1, maxNodes: 5 })
  await runHook({ ...options, workspace: path.join(dir, 'project-b') })
  assert.equal(calls.length, 2)
  nodes = [{ id: 'oauth' }]
  const relevant = await runHook({ ...options, stdin: JSON.stringify({ prompt: 'Fix OAuth callback regression tests' }) })
  assert.equal(JSON.parse(relevant.stdout).hookSpecificOutput.additionalContext, prompt)
  prompt = '<retrieved_knowledge>' + 'x'.repeat(9000) + 'Important warning</retrieved_knowledge>'
  assert.equal((await runHook({ ...options, stdin: JSON.stringify({ prompt: 'Inspect OAuth callback architecture' }) })).stdout, '')
})
