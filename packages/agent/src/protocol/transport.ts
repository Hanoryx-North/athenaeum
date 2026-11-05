/**
 * The one-shot process transport for Wiki protocol v1.
 *
 * Version 1 of the protocol is one request per process: the adapter writes one envelope to
 * the bridge's stdin and reads one envelope from its stdout. That is what the library
 * publishes, and it is why this module spawns per call rather than holding a process open.
 * A later long-lived JSONL transport implements the same {@link BridgeTransport} interface
 * without changing anything above it.
 *
 * @module athenaeum-agent/protocol/transport
 */

import { spawn } from 'node:child_process'
import { BRIDGE_CANCELLED, BRIDGE_RESPONSE_TOO_LARGE, BRIDGE_SPAWN_FAILED, BRIDGE_TIMEOUT, WikiTransportError } from './errors.ts'

/** What one bridge invocation produced. A non-zero `exitCode` still carries a valid error envelope on stdout. */
export interface BridgeExchange {
  readonly stdout: string
  readonly stderr: string
  readonly exitCode: number | null
}

/**
 * One request/response exchange with a Wiki library.
 *
 * Implementations own process or connection lifetime and must reject with a
 * {@link WikiTransportError} when no response can be obtained. Returning a non-zero
 * `exitCode` is not a failure: the bridge exits non-zero for protocol and knowledge errors
 * while still writing a well-formed error envelope.
 */
export interface BridgeTransport {
  /**
   * Send one serialized request envelope and read the response.
   * @param payload - the JSON request envelope.
   * @param signal - optional cancellation; an abort kills the exchange.
   * @returns what the bridge wrote, plus its exit status.
   */
  send(payload: string, signal?: AbortSignal): Promise<BridgeExchange>
}

/** How to invoke the `athenaeum bridge` command. */
export interface SpawnBridgeOptions {
  /** The Athenaeum executable. A bare name resolves through `PATH`. */
  readonly command: string
  /** The project directory whose knowledge base is addressed. */
  readonly workspace: string
  /** Wall-clock budget for one exchange, in milliseconds. */
  readonly timeoutMs: number
  /**
   * Cap on bytes read from one exchange, in each direction this side keeps: stdout beyond it
   * abandons the exchange, and stderr beyond it is truncated. One budget bounds the whole
   * process, so a bridge that fails loudly cannot cost more memory than one that answers.
   */
  readonly maxResponseBytes: number
  /** Environment for the child process. Defaults to this process's environment. */
  readonly env?: NodeJS.ProcessEnv
}

/**
 * Append a chunk to a buffer without letting a chatty child grow it without bound.
 *
 * @param buffer - the bytes kept so far.
 * @param chunk - the next decoded chunk.
 * @param cap - the maximum length to keep.
 * @returns the buffer, and whether anything was dropped.
 */
function appendCapped(buffer: string, chunk: string, cap: number): { text: string; truncated: boolean } {
  const room = cap - buffer.length
  if (room <= 0) return { text: buffer, truncated: true }
  if (chunk.length <= room) return { text: buffer + chunk, truncated: false }
  return { text: buffer + chunk.slice(0, room), truncated: true }
}

/**
 * Build the bridge argument vector.
 *
 * `-C` is a global option and precedes the subcommand, which is what makes one installed
 * executable serve every workspace without a per-project install.
 *
 * @param workspace - the project directory whose knowledge base is addressed.
 * @returns the arguments following the executable name.
 */
export function bridgeArgs(workspace: string, _command = 'athenaeum'): readonly string[] {
  // One engine, one spelling. The retired `athenaeum-core` shim understands it too.
  return ['-C', workspace, 'bridge']
}

/**
 * A transport that spawns `athenaeum bridge` once per request.
 *
 * The child is killed when the caller aborts, when the time budget expires, or when stdout
 * exceeds the byte cap; each of those rejects with a distinct {@link WikiTransportError}
 * code, because "the knowledge base is enormous" and "the command is not installed" need
 * different operator responses.
 *
 * @param options - executable, workspace, and the budgets one exchange may spend.
 * @returns a transport bound to that command and workspace.
 */
export function spawnBridgeTransport(options: SpawnBridgeOptions): BridgeTransport {
  return {
    send(payload: string, signal?: AbortSignal): Promise<BridgeExchange> {
      return new Promise<BridgeExchange>((resolve, reject) => {
        if (signal?.aborted === true) {
          reject(new WikiTransportError('the wiki request was cancelled before it started', BRIDGE_CANCELLED))
          return
        }
        const child = spawn(options.command, [...bridgeArgs(options.workspace, options.command)], {
          stdio: ['pipe', 'pipe', 'pipe'],
          ...options.env ? { env: options.env } : {},
        })
        let stdout = ''
        let stderr = ''
        let stderrTruncated = false
        let settled = false
        const finish = (act: () => void): void => {
          if (settled) return
          settled = true
          clearTimeout(timer)
          signal?.removeEventListener('abort', onAbort)
          act()
        }
        const abandon = (message: string, code: string): void => {
          finish(() => {
            child.kill('SIGKILL')
            reject(new WikiTransportError(message, code))
          })
        }
        const timer = setTimeout(
          () => abandon(`the wiki bridge did not answer within ${options.timeoutMs}ms`, BRIDGE_TIMEOUT),
          options.timeoutMs,
        )
        const onAbort = (): void => abandon('the wiki request was cancelled', BRIDGE_CANCELLED)
        signal?.addEventListener('abort', onAbort, { once: true })

        child.stdout.setEncoding('utf8')
        child.stderr.setEncoding('utf8')
        child.stdout.on('data', (chunk: string) => {
          stdout += chunk
          if (stdout.length > options.maxResponseBytes) {
            abandon(
              `the wiki bridge wrote more than ${options.maxResponseBytes} bytes`,
              BRIDGE_RESPONSE_TOO_LARGE,
            )
          }
        })
        child.stderr.on('data', (chunk: string) => {
          // Diagnostics are kept for the failure message, so they are bounded by the same
          // budget stdout is: a bridge that answers with megabytes of noise must not cost
          // this process more memory than one that answers with a brief.
          const kept = appendCapped(stderr, chunk, options.maxResponseBytes)
          stderr = kept.text
          stderrTruncated = stderrTruncated || kept.truncated
        })
        child.on('error', (error: Error) => {
          finish(() => reject(new WikiTransportError(
            `could not run "${options.command}": ${error.message}`,
            BRIDGE_SPAWN_FAILED,
            { cause: error },
          )))
        })
        child.on('close', (exitCode: number | null) => {
          finish(() => resolve({
            stdout,
            stderr: stderrTruncated
              ? `${stderr}\n[stderr truncated at ${options.maxResponseBytes} bytes]`
              : stderr,
            exitCode,
          }))
        })

        // A bridge that exits before reading stdin makes this write fail with EPIPE. The
        // close handler above already reports that exit, so the write error adds nothing.
        child.stdin.on('error', () => {})
        child.stdin.end(payload)
      })
    },
  }
}
