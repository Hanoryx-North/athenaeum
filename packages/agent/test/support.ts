/** Test support: fixture loading and a transport that answers from memory. */

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { BridgeExchange, BridgeTransport } from '../src/protocol/index.ts'

/** Absolute path to a vendored protocol fixture file. */
export function fixturePath(name: string): string {
  return fileURLToPath(new URL(`../../../fixtures/wiki-protocol/v1/${name}`, import.meta.url))
}

/** Parse one vendored fixture file. */
export function loadFixtures(name: string): Record<string, Record<string, unknown>> {
  return JSON.parse(readFileSync(fixturePath(name), 'utf8')) as Record<string, Record<string, unknown>>
}

/** The request envelopes a transport was asked to send, in order. */
export interface RecordedTransport extends BridgeTransport {
  readonly payloads: string[]
  /** The parsed `request` field of the last envelope sent. */
  lastRequest(): unknown
}

/**
 * A transport that records what it was asked to send and replays a scripted answer.
 * `answer` receives the parsed request envelope so a script can echo the request id.
 */
export function recordingTransport(
  answer: (envelope: Record<string, unknown>) => Partial<BridgeExchange> | string,
): RecordedTransport {
  const payloads: string[] = []
  return {
    payloads,
    lastRequest(): unknown {
      const last = payloads.at(-1)
      if (last === undefined) throw new Error('no request was sent')
      return (JSON.parse(last) as Record<string, unknown>)['request']
    },
    async send(payload: string): Promise<BridgeExchange> {
      payloads.push(payload)
      const scripted = answer(JSON.parse(payload) as Record<string, unknown>)
      const exchange = typeof scripted === 'string' ? { stdout: scripted } : scripted
      return { stdout: '', stderr: '', exitCode: 0, ...exchange }
    },
  }
}

/** Wrap a response payload in a successful v1 envelope correlated to the request. */
export function okEnvelope(envelope: Record<string, unknown>, response: unknown): string {
  return JSON.stringify({
    protocolVersion: 1,
    requestId: envelope['requestId'],
    status: 'ok',
    response,
  })
}
