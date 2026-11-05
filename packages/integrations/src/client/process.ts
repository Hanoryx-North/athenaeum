import { spawn } from 'node:child_process';
import { AthenaeumError, AthenaeumUnavailable } from './errors.ts';

export interface RunOptions {
  stdin?: string;
  timeoutMs: number;
  maxBytes: number;
  env?: NodeJS.ProcessEnv;
}

export interface RunResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

/**
 * Run a command with no shell, so nothing a model or a page says can become shell syntax, and with
 * bounds on both time and output so a misbehaving child cannot stall or exhaust the caller.
 */
export function run(command: string, args: string[], options: RunOptions): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: options.env ?? process.env,
      shell: false,
      windowsHide: true,
    });
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    let size = 0;
    let settled = false;
    const finish = (action: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      action();
    };

    const timer = setTimeout(() => {
      child.kill('SIGKILL');
      finish(() =>
        reject(new AthenaeumError('timeout', `\`${command}\` did not answer within ${options.timeoutMs} ms`)),
      );
    }, options.timeoutMs);

    child.stdout.on('data', (chunk: Buffer) => {
      size += chunk.length;
      if (size > options.maxBytes) {
        child.kill('SIGKILL');
        finish(() =>
          reject(new AthenaeumError('response_too_large', `\`${command}\` answered with more than ${options.maxBytes} bytes`)),
        );
        return;
      }
      out.push(chunk);
    });
    child.stderr.on('data', (chunk: Buffer) => {
      if (err.reduce((n, c) => n + c.length, 0) < 64 * 1024) err.push(chunk);
    });
    child.on('error', (error) => finish(() => reject(new AthenaeumUnavailable(command, error))));
    child.on('close', (code) =>
      finish(() =>
        resolve({
          stdout: Buffer.concat(out).toString('utf8'),
          stderr: Buffer.concat(err).toString('utf8'),
          exitCode: code ?? 1,
        }),
      ),
    );
    child.stdin.on('error', () => {
      /* the child exited before reading its input; its exit status explains why */
    });
    child.stdin.end(options.stdin ?? '');
  });
}
