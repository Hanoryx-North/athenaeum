/**
 * A failure reported by the Athenaeum bridge or CLI. `code` is the contract (snake_case, stable);
 * `message` is for people.
 */
export class AthenaeumError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly details?: unknown,
  ) {
    super(message);
    this.name = 'AthenaeumError';
  }

  /** There is no knowledge base here. Not a fault: an ordinary agent works without one. */
  get isNoKnowledgeBase(): boolean {
    return this.code === 'workspace_not_found' || (this instanceof KnowledgeStateError && this.state === 'absent');
  }
}

/** The `athenaeum` command could not be run at all. */
export class AthenaeumUnavailable extends AthenaeumError {
  constructor(command: string, cause: unknown) {
    super(
      'unavailable',
      `could not run \`${command}\` (${cause instanceof Error ? cause.message : String(cause)}). ` +
        'Install Athenaeum, or set ATHENAEUM_BIN to the path of its command-line tool.',
    );
    this.name = 'AthenaeumUnavailable';
  }
}

/**
 * A state of the world a caller has to act on, told apart from an ordinary failure. An agent that
 * cannot tell "there is no vault here" from "the index is behind the files" from "the Library is
 * unreachable" will guess, and guessing about knowledge is how a wrong answer gets written down.
 *
 * Every instance carries `state` (what is wrong) and `action` (what a person or agent can do next),
 * and `code` stays the bridge's own code so existing callers keep working.
 */
export type KnowledgeState = 'absent' | 'stale_index' | 'offline' | 'protocol_mismatch';

export class KnowledgeStateError extends AthenaeumError {
  /** The failure without the remedy sentence; `action` carries that, separately. */
  readonly detail: string;

  constructor(
    readonly state: KnowledgeState,
    code: string,
    detail: string,
    /** One sentence a caller can act on. Never empty. */
    readonly action: string,
    details?: unknown,
  ) {
    super(code, `${detail.replace(/\s+$/, '')} ${action}`.trim(), details);
    this.name = 'KnowledgeStateError';
    this.detail = detail;
  }

  /** True when there is nothing to read here yet, which is an ordinary state, not a fault. */
  get isAbsent(): boolean {
    return this.state === 'absent';
  }
}

const CODES: Record<KnowledgeState, readonly string[]> = {
  absent: ['workspace_not_found', 'no_vault', 'vault_not_found', 'not_initialized', 'knowledge_base_not_found'],
  stale_index: ['stale_index', 'index_stale'],
  offline: ['offline', 'library_offline', 'unreachable', 'network_error', 'connection_failed'],
  protocol_mismatch: ['unsupported_protocol', 'protocol_mismatch', 'unsupported_method'],
};

/** The state a bridge or CLI error code names, or `null` when it names an ordinary failure. */
export function knowledgeStateFor(code: string): KnowledgeState | null {
  for (const state of Object.keys(CODES) as KnowledgeState[]) {
    if (CODES[state].includes(code)) return state;
  }
  return null;
}

/** What the core writes to stderr when a sync command could not reach the server at all. */
export const OFFLINE_PATTERN = /error sending request|connection refused|connection reset|dns error|failed to lookup|timed out|network is unreachable|could not connect/i;

/**
 * Translate an engine failure into the state it names, with the action that resolves it. Anything
 * that is not one of the four product states stays exactly as it was: this adds meaning, it does
 * not rewrite errors.
 */
export function knowledgeStateError(error: AthenaeumError, workspace: string): KnowledgeStateError | null {
  const state = knowledgeStateFor(error.code);
  if (!state) return null;
  const detail = error.message;
  switch (state) {
    case 'absent':
      return new KnowledgeStateError(
        'absent',
        error.code,
        detail,
        `Nothing to read: create a knowledge base with \`athenaeum init\` in ${workspace}, or point this integration at a directory that holds .athenaeum/.`,
        error.details,
      );
    case 'stale_index':
      return new KnowledgeStateError(
        'stale_index',
        error.code,
        detail,
        'Rebuild the derived index with the wiki_index tool (scope vault, repository or all), then retry.',
        error.details,
      );
    case 'offline':
      return new KnowledgeStateError(
        'offline',
        error.code,
        detail,
        'The Library is unreachable; local knowledge still works. Check the configured server and the network, then retry.',
        error.details,
      );
    case 'protocol_mismatch':
      return new KnowledgeStateError(
        'protocol_mismatch',
        error.code,
        detail,
        'This client and the engine disagree on the bridge contract; update or rebuild Athenaeum so both speak protocol v1.',
        error.details,
      );
  }
}

const TOKEN_SHAPES = [/\batn_[A-Za-z0-9_-]{4,}/g, /\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/gi];

/**
 * Text that came back from a child process is shown to people and models; it must never carry a
 * credential. The CLI does not print tokens, so this is a second wall, not the first.
 */
export function redact(text: string, secrets: Iterable<string | undefined> = []): string {
  let out = text;
  for (const secret of secrets) {
    if (secret && secret.length >= 8) out = out.split(secret).join('[redacted]');
  }
  for (const shape of TOKEN_SHAPES) out = out.replace(shape, '[redacted]');
  return out;
}
