import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { arrayOf, assertShape, int, lit, nullable, object, oneOf, optional, str } from './shape.ts'

interface Sample { id: string; count: number; tag?: string; parent: string | null; kind: 'a' | 'b'; items: string[] }
const sample = object<Sample>({ id: str, count: int, tag: optional(str), parent: nullable(str), kind: oneOf(['a', 'b']), items: arrayOf(str) })
const good = { id: 'x', count: 1, parent: null, kind: 'a', items: ['y'] }

describe('the fixture shape checker rejects drift (so a green fixture test means something)', () => {
  it('accepts a conforming object, with or without optional fields', () => {
    assertShape(sample, good, 'good')
    assertShape(sample, { ...good, tag: 't' }, 'good+tag')
  })
  it('rejects a missing required field', () => {
    const { id: _id, ...rest } = good
    assert.throws(() => assertShape(sample, rest, 'rest'), /rest\.id: missing/)
  })
  it('rejects a field the contract does not have', () => {
    assert.throws(() => assertShape(sample, { ...good, extra: 1 }, 'x'), /x\.extra: not part of the contract/)
  })
  it('rejects wrong types, wrong literals and nested problems together', () => {
    assert.throws(() => assertShape(sample, { ...good, count: '1', kind: 'c', items: [1] }, 'x'), (error: Error) =>
      /x\.count: expected an integer/.test(error.message) && /x\.kind: expected one of a, b/.test(error.message) && /x\.items\[0\]: expected string/.test(error.message))
    assert.throws(() => assertShape(lit('v1'), 'v2', 'v'), /expected "v1"/)
  })
  it('rejects null where null is not allowed and undefined-ish optional values', () => {
    assert.throws(() => assertShape(sample, { ...good, id: null }, 'x'), /x\.id: expected string, got null/)
    assert.throws(() => assertShape(sample, { ...good, tag: null }, 'x'), /x\.tag: expected string, got null/)
  })
})
