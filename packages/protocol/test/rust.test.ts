import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { describe, it } from 'node:test'
import { root } from './fixtures.ts'

/**
 * The Rust crate loads the same fixtures and round-trips them through serde; that is how Rust/TS
 * agreement is proven. It runs here when `cargo` is available (set `PROTOCOL_SKIP_RUST=1` to opt out).
 */
const cargo = process.env.CARGO || 'cargo'
const available = spawnSync(cargo, ['--version'], { encoding: 'utf8' }).status === 0

describe('rust crate (athenaeum-protocol)', () => {
  it('cargo test passes against the shared fixtures', { timeout: 900_000, skip: process.env.PROTOCOL_SKIP_RUST ? 'PROTOCOL_SKIP_RUST is set' : available ? false : `cargo not found (set CARGO or put cargo on PATH); the Rust half was NOT verified` }, () => {
    const result = spawnSync(cargo, ['test', '--locked', '-p', 'athenaeum-protocol'], {
      cwd: root, encoding: 'utf8', env: { ...process.env, CARGO_TERM_COLOR: 'never' },
    })
    process.stdout.write(`${result.stdout ?? ''}${result.stderr ?? ''}`.split('\n').filter((line) => /^test result|^running|FAILED|panicked/.test(line)).map((line) => `# ${line}\n`).join(''))
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`)
  })
})
