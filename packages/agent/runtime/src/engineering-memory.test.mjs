import { test } from 'node:test'
import assert from 'node:assert/strict'
import { textChunks, lessonContent, commandObservations, frictionSignals, MAX_TEXT_BYTES } from './engineering-memory.mjs'

test('long engineering input covers every character including Unicode, code and the final lesson', () => {
  const talk = `# High trust engineering\nArchitecture and types enforce boundaries that prose cannot guarantee.\n\nExample:\n\`\`\`rust\nstruct ReviewedRule(String);\n\`\`\`\n\n# Tool friction\nPreserve failing exit codes; a pipeline can hide the actual failure.\n`.repeat(180) + '\nFinal lesson: verify the running application. 🧪日本語'
  const chunks = textChunks(talk)
  assert.ok(chunks.length > 3)
  assert.equal(chunks.map((chunk) => chunk.text).join(''), talk)
  assert.ok(chunks.every((chunk) => Buffer.byteLength(chunk.text) <= 12 * 1024))
  assert.match(chunks.at(-1).text, /Final lesson/)
  assert.throws(() => textChunks('x'.repeat(MAX_TEXT_BYTES + 1)), /64 KiB/)
  for (const source of ['漢'.repeat(18000), 'x'.repeat(MAX_TEXT_BYTES), '\n'.repeat(13000) + 'lesson']) {
    const parts = textChunks(source)
    assert.equal(parts.map((part) => part.text).join(''), source)
    assert.ok(parts.every((part) => Buffer.byteLength(part.text) <= 12 * 1024))
  }
})

test('shallow memories and unverifiable improvement suggestions fail the structure gate', () => {
  assert.equal(lessonContent({ content: 'The registry contains a retry file.' }), null)
  const lesson = { recommendation: 'Check the exit status of every pipeline stage.', rationale: 'A successful tail process may conceal a failing compiler.',
    whenToUse: 'When automating command pipelines during validation.', validation: 'Add a fixture with a failing first stage and successful tail.', limits: 'Pipelines used for display alone may not need failure propagation.' }
  assert.match(lessonContent({ lesson }), /## When to use/)
  assert.equal(lessonContent({ lesson: { ...lesson, rationale: 'use it' } }), null)
  assert.equal(lessonContent({ lesson: { ...lesson, improvement: { kind: 'lint', proposal: 'Add a shell rule.' } } }), null)
  assert.equal(lessonContent({ lesson: { ...lesson, example: 'api_key = sk-testcredentialvalue123456789' } }), null)
})

test('failure and retry observations are leads; ordinary repeated reads are not flagged as defects', () => {
  const commands = commandObservations([
    { command: 'npm test -- retry', exitCode: 1, output: 'Assertion failed: expected three attempts.' },
    { command: 'npm test -- retry --verbose', exitCode: 1, output: 'Assertion failed: expected three attempts.' },
    { command: 'npm test -- retry --run', exitCode: 0, output: 'All retry tests passed.' },
    { command: 'cat .env', exitCode: 0, output: 'private content' },
    { command: 'cat app.js', exitCode: 0, output: 'password = abcdefghijklmnop' },
  ])
  assert.equal(commands.length, 3)
  const signals = frictionSignals(commands)
  assert.equal(signals.length, 1)
  assert.match(signals[0].signal, /investigate whether retries were necessary/)
  assert.equal(frictionSignals(commandObservations(Array.from({ length: 4 }, () => ({ command: 'cat app.js', exitCode: 0, output: 'Legitimate inspection.' })))).length, 0)
})
