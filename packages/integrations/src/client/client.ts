import { randomUUID } from 'node:crypto';
import { bridgeRequest, type BridgeRequest } from '@athenaeum/protocol/bridge';
import { AthenaeumError, KnowledgeStateError, OFFLINE_PATTERN, knowledgeStateError, redact } from './errors.ts';
import { resolveCommand, type RemoteConfig } from './config.ts';
import { run } from './process.ts';
import {
  bounds as boundsSchema,
  callerIdentity,
  evidenceCreate,
  indexRequest as indexRequestSchema,
  input,
  linkCreate,
  nodeCreate,
  nodeRef,
  nodeUpdate,
  parseAnswer,
  pathRequest as pathRequestSchema,
  queryText,
  reasonText,
  responses,
  reviewQuery as reviewQuerySchema,
  sourceQuery as sourceQuerySchema,
  taskText,
} from './validate.ts';
import {
  PROTOCOL_VERSION,
  type Accepted,
  type Bounds,
  type Caller,
  type Capabilities,
  type Context,
  type Edge,
  type EngineStatus,
  type Evidence,
  type IndexReport,
  type IndexRequest,
  type IndexScope,
  type KnowledgeNode,
  type KnowledgePath,
  type NavigatedNode,
  type NewNode,
  type NodeDetail,
  type NodeVersion,
  type Page,
  type Proposal,
  type RelatedNode,
  type RepoInfo,
  type ReviewPage,
  type ReviewQuery,
  type SearchHit,
  type SourcePage,
  type SourceQuery,
  type SyncAction,
  type SyncResult,
  type Keep,
} from './types.ts';

export interface ClientOptions {
  /** The `athenaeum` executable. A bare name resolves through PATH. */
  command?: string;
  timeoutMs?: number;
  maxResponseBytes?: number;
  env?: NodeJS.ProcessEnv;
  remote?: RemoteConfig;
}

const DEFAULTS = { timeoutMs: 30_000, maxResponseBytes: 8 * 1024 * 1024 };

/** Every list read is bounded: a page is at most this many items, and a scan at most this many rows. */
export const PAGE_DEFAULTS = { size: 20, maxSize: 100, scan: 200, maxScan: 1_000 } as const;

interface CallOptions {
  /**
   * True for the additive v1 methods (`status`, `review`, `sources`, `index`, `path`).
   *
   * An engine older than the client rejects those methods as `invalid_request` with serde's
   * "unknown variant" message, because that is the only code protocol v1 has for a request it
   * cannot parse. The client reads that message as the honest signal that the ENGINE IS OLDER THAN
   * THE CLIENT and reports `protocol_mismatch` with the update action — while leaving a genuine
   * `invalid_request` (a malformed argument) exactly as it came. It is a client-side reading of an
   * existing code, not a new wire code: no error code is invented here.
   */
  additive?: boolean;
}

/**
 * The one way this package reaches a knowledge base: the `athenaeum` command, speaking protocol v1
 * over `athenaeum bridge`. It never reads `knowledge.db` or parses the vault. The knowledge
 * processing — search, graph, ranking, briefs — is Athenaeum's, running on this machine.
 */
export class AthenaeumClient {
  private readonly command: string;
  private readonly timeoutMs: number;
  private readonly maxBytes: number;
  private readonly env: NodeJS.ProcessEnv | undefined;
  private readonly remote: ClientOptions['remote'];

  constructor(
    /** The project directory whose knowledge base is addressed. */
    readonly workspace: string,
    options: ClientOptions = {},
  ) {
    this.command = options.command ?? resolveCommand(options.env);
    this.timeoutMs = options.timeoutMs ?? DEFAULTS.timeoutMs;
    this.maxBytes = options.maxResponseBytes ?? DEFAULTS.maxResponseBytes;
    this.env = options.env;
    this.remote = options.remote;
  }

  // -- protocol v1 -----------------------------------------------------------------------------

  private async call<T>(request: BridgeRequest, options: CallOptions = {}): Promise<T> {
    const requestId = randomUUID();
    const { stdout, stderr, exitCode } = await run(this.command, ['-C', this.workspace, 'bridge'], {
      stdin: JSON.stringify(bridgeRequest(request, requestId)),
      timeoutMs: this.timeoutMs,
      maxBytes: this.maxBytes,
      env: this.env,
    });
    let parsed: unknown;
    try {
      parsed = JSON.parse(stdout);
    } catch {
      throw new AthenaeumError(
        'bad_response',
        `the athenaeum bridge did not answer with JSON (exit ${exitCode})${stderr ? `: ${redact(stderr.trim().slice(0, 300), [this.env?.ATHENAEUM_LIBRARY_TOKEN])}` : ''}`,
      );
    }
    if (typeof parsed !== 'object' || parsed === null) {
      throw new AthenaeumError('bad_response', `the athenaeum bridge answered with ${parsed === null ? 'null' : typeof parsed} instead of an envelope`);
    }
    // The envelope is checked here; the payload is checked by the method that reads it, so a
    // malformed answer becomes a typed failure at the boundary rather than a shape the caller
    // discovers halfway through using it.
    const envelope = parsed as {
      protocolVersion?: unknown;
      requestId?: unknown;
      status?: unknown;
      response?: { method?: unknown };
      error?: { code?: unknown; message?: unknown; details?: unknown };
    };
    if (envelope.protocolVersion !== PROTOCOL_VERSION) {
      throw new KnowledgeStateError(
        'protocol_mismatch',
        'unsupported_protocol',
        `the bridge speaks protocol v${String(envelope.protocolVersion)}; this client speaks v${PROTOCOL_VERSION}`,
        'Update or rebuild Athenaeum so the engine and this integration speak the same bridge version.',
      );
    }
    if (typeof envelope.requestId === 'string' && envelope.requestId !== requestId) {
      throw new AthenaeumError('bad_response', 'the bridge answered a different request');
    }
    if (envelope.status !== 'ok') {
      const error = envelope.error ?? {};
      const code = typeof error.code === 'string' ? error.code : 'error';
      const message = redact(typeof error.message === 'string' ? error.message : 'the bridge reported an error', [this.env?.ATHENAEUM_LIBRARY_TOKEN]);
      if (options.additive && code === 'invalid_request' && /unknown variant|unknown method|unrecognized|expected one of|invalid method/i.test(message)) {
        throw new KnowledgeStateError(
          'protocol_mismatch',
          'unsupported_method',
          `the installed engine does not implement the \`${request.method}\` bridge method: ${message}`,
          'Update or rebuild Athenaeum so its bridge implements protocol v1 (status, review, sources, index, path), then retry.',
          error.details,
        );
      }
      throw knowledgeStateError(new AthenaeumError(code, message, error.details), this.workspace) ?? new AthenaeumError(code, message, error.details);
    }
    if (envelope.response?.method !== request.method) {
      throw new AthenaeumError('bad_response', `the bridge answered ${String(envelope.response?.method)} instead of ${request.method}`);
    }
    return envelope.response as T;
  }

  async capabilities(): Promise<Capabilities> {
    return (await this.call<{ capabilities: Capabilities }>({ method: 'capabilities' })).capabilities;
  }

  // -- state and indexing (additive v1) --------------------------------------------------------

  /**
   * The workspace as it stands: initialized, vault, counts, index freshness, versions. Answers even
   * when there is no knowledge base (`initialized: false`), because that is a state to report, not
   * a failure to hide. It never refreshes anything: it reports staleness rather than papering over
   * it.
   */
  async status(): Promise<EngineStatus> {
    const answer = await this.call<unknown>({ method: 'status' }, { additive: true });
    return parseAnswer(responses.status, answer, 'status').status;
  }

  /**
   * Fail with the state that explains why a knowledge answer cannot be trusted yet: no knowledge
   * base, no vault, or a derived index that is behind the files. Callers that promise a current
   * view (a review, a source list) use this instead of returning last week's answer as today's.
   */
  async requireFreshIndex(scope: IndexScope = 'vault'): Promise<EngineStatus> {
    const status = await this.status();
    if (!status.initialized) {
      throw new KnowledgeStateError(
        'absent',
        'workspace_not_found',
        `there is no knowledge base at ${status.workspacePath}`,
        'Create one with `athenaeum init` there, or point this integration at a directory that holds .athenaeum/.',
      );
    }
    if (!status.hasVault) {
      throw new KnowledgeStateError(
        'absent',
        'no_vault',
        `the knowledge base at ${status.workspacePath} has no vault yet`,
        'Create the vault (in Athenaeum, or with `athenaeum init`), then retry.',
      );
    }
    if (status.indexedAt === null || status.indexStale) {
      const detail =
        status.indexedAt === null
          ? `nothing has been indexed yet at ${status.workspacePath}, so there is no current picture to answer from`
          : `the derived index is behind the files on disk (last indexed: ${status.indexedAt})`;
      const remedy = scope === 'all' ? 'call wiki_index with scope "all"' : `call wiki_index with scope "all" (or "${scope}" if that is all that moved)`;
      throw new KnowledgeStateError('stale_index', 'stale_index', detail, `Rebuild the index before trusting this answer: ${remedy}, then retry.`);
    }
    return status;
  }

  /**
   * Rescan the vault, the repository, or both. `paths` narrows a repository scan; it is refused
   * with a vault scan because indexing part of a vault would silently leave the rest behind.
   */
  async index(request: IndexRequest): Promise<IndexReport> {
    const parsed = input(indexRequestSchema, request, 'index request');
    if (parsed.paths?.length && parsed.scope !== 'repository') {
      throw new AthenaeumError('invalid_request', 'index request: `paths` is only meaningful with scope "repository"; a vault scan reconciles the whole vault');
    }
    const answer = await this.call<unknown>(
      { method: 'index', scope: parsed.scope, ...(parsed.paths?.length ? { paths: parsed.paths } : {}) },
      { additive: true },
    );
    return parseAnswer(responses.index, answer, 'index').index;
  }

  // -- reading knowledge -----------------------------------------------------------------------

  async search(query: string, limit?: number): Promise<SearchHit[]> {
    const text = input(queryText, query, 'search query');
    const bounded = limit === undefined ? undefined : input(boundsSchema, { limit }, 'search limit').limit;
    return (await this.call<{ hits: SearchHit[] }>({ method: 'search', query: text, ...(bounded ? { limit: bounded } : {}) })).hits;
  }

  /** The same search, as a window with a real `hasMore` derived from one extra result. */
  async searchPage(query: string, window: Bounds = {}): Promise<Page<SearchHit>> {
    const text = input(queryText, query, 'search query');
    const asked = this.window(window, PAGE_DEFAULTS.maxSize);
    const hits = (await this.call<{ hits: SearchHit[] }>({ method: 'search', query: text, limit: asked.requested })).hits;
    return this.page(hits, asked);
  }

  async open(node: string): Promise<NodeDetail> {
    return (await this.call<{ detail: NodeDetail }>({ method: 'open', node: input(nodeRef, node, 'node') })).detail;
  }

  async navigate(node: string, depth?: number, maxNodes?: number): Promise<NavigatedNode[]> {
    const request = { method: 'navigate' as const, node: input(nodeRef, node, 'node'), ...boundedWalk({ depth, maxNodes }) };
    return (await this.call<{ nodes: NavigatedNode[] }>(request)).nodes;
  }

  /**
   * How one node connects to another. `found: false` is an ordinary answer: no path exists inside
   * the walk's budget, and the caller is told that rather than handed an empty trail as if it were.
   */
  async path(request: { from: string; to: string; depth?: number; maxNodes?: number }): Promise<KnowledgePath> {
    const parsed = input(pathRequestSchema, request, 'path request');
    const answer = await this.call<unknown>(
      {
        method: 'path',
        from: parsed.from,
        to: parsed.to,
        ...(parsed.depth !== undefined ? { depth: parsed.depth } : {}),
        ...(parsed.maxNodes !== undefined ? { maxNodes: parsed.maxNodes } : {}),
      },
      { additive: true },
    );
    return parseAnswer(responses.path, answer, 'path').path;
  }

  async evidence(node: string): Promise<Evidence[]> {
    return (await this.call<{ evidence: Evidence[] }>({ method: 'evidence', node: input(nodeRef, node, 'node') })).evidence;
  }

  async evidencePage(node: string, window: Bounds = {}): Promise<Page<Evidence>> {
    const parsed = input(nodeRef, node, 'node');
    const asked = this.window(window, PAGE_DEFAULTS.maxScan);
    const evidence = (await this.call<{ evidence: Evidence[] }>({ method: 'evidence', node: parsed })).evidence;
    return this.page(evidence, { ...asked, requested: evidence.length + 1 });
  }

  async history(node: string): Promise<NodeVersion[]> {
    return (await this.call<{ versions: NodeVersion[] }>({ method: 'history', node: input(nodeRef, node, 'node') })).versions;
  }

  async historyPage(node: string, window: Bounds = {}): Promise<Page<NodeVersion>> {
    const parsed = input(nodeRef, node, 'node');
    const asked = this.window(window, PAGE_DEFAULTS.maxScan);
    const versions = (await this.call<{ versions: NodeVersion[] }>({ method: 'history', node: parsed })).versions;
    return this.page(versions, { ...asked, requested: versions.length + 1 });
  }

  async related(node: string, includeProposed = false): Promise<RelatedNode[]> {
    return (await this.call<{ relationships: RelatedNode[] }>({ method: 'related', node: input(nodeRef, node, 'node'), includeProposed })).relationships;
  }

  async relatedPage(node: string, includeProposed = false, window: Bounds = {}): Promise<Page<RelatedNode>> {
    const parsed = input(nodeRef, node, 'node');
    const asked = this.window(window, PAGE_DEFAULTS.maxScan);
    const relationships = (await this.call<{ relationships: RelatedNode[] }>({ method: 'related', node: parsed, includeProposed })).relationships;
    return this.page(relationships, { ...asked, requested: relationships.length + 1 });
  }

  async context(request: { task: string; seeds?: string[]; depth?: number; maxNodes?: number }): Promise<Context> {
    const task = input(taskText, request.task, 'task');
    const seeds = request.seeds?.map((seed, index) => inputNode(seed, `seeds.${index}`));
    const walk = boundedWalk({ depth: request.depth, maxNodes: request.maxNodes, maxDepth: 10, maxNodesCap: 1_000 });
    return (
      await this.call<{ context: Context }>({
        method: 'context',
        task,
        ...(seeds?.length ? { seeds } : {}),
        ...walk,
      })
    ).context;
  }

  // -- the review queue (additive v1) ----------------------------------------------------------

  /**
   * What needs a person's decision: contradictions, unsupported rules, stale anchors, proposals
   * nobody has decided, scans that did not finish, and so on. The engine's list has no offset and
   * no filter on the wire, so a filtered read fetches up to `scan` findings and filters those;
   * `scanComplete` says whether the whole list was read, and `hasMore` stays conservative when it
   * was not. Staleness is data here, not an error: `requireFreshIndex` is the guard when a caller
   * needs a current index.
   */
  async review(query: ReviewQuery = {}): Promise<ReviewPage> {
    const parsed = input(reviewQuerySchema, query, 'review query');
    const size = parsed.limit ?? PAGE_DEFAULTS.size;
    const offset = parsed.offset ?? 0;
    const filtered = parsed.kind !== undefined || parsed.blocking !== undefined;
    const requested = filtered ? Math.min(parsed.scan ?? PAGE_DEFAULTS.scan, PAGE_DEFAULTS.maxScan) : Math.min(offset + size + 1, PAGE_DEFAULTS.maxScan);
    const { findings } = parseAnswer(responses.review, await this.call<unknown>({ method: 'review', limit: requested }, { additive: true }), 'review');
    return this.filteredPage(findings, offset, size, requested, {
      kind: parsed.kind,
      blocking: parsed.blocking,
    }, (finding) => (parsed.kind === undefined || finding.kind === parsed.kind) && (parsed.blocking === undefined || finding.blocking === parsed.blocking));
  }

  /**
   * Every indexed source file and vault page, with the coverage the index has of it. `stale: true`
   * is a fact about that one source, reported as such.
   */
  async sources(query: SourceQuery = {}): Promise<SourcePage> {
    const parsed = input(sourceQuerySchema, query, 'sources query');
    const size = parsed.limit ?? PAGE_DEFAULTS.size;
    const offset = parsed.offset ?? 0;
    const filtered = parsed.prefix !== undefined || parsed.staleOnly === true;
    const requested = filtered ? Math.min(parsed.scan ?? PAGE_DEFAULTS.scan, PAGE_DEFAULTS.maxScan) : Math.min(offset + size + 1, PAGE_DEFAULTS.maxScan);
    const { sources } = parseAnswer(responses.sources, await this.call<unknown>({ method: 'sources', limit: requested }, { additive: true }), 'sources');
    return this.filteredPage(sources, offset, size, requested, {
      prefix: parsed.prefix,
      staleOnly: parsed.staleOnly,
    }, (source) => (parsed.prefix === undefined || source.path.startsWith(parsed.prefix)) && (parsed.staleOnly !== true || source.stale));
  }

  // -- proposals and the human decision --------------------------------------------------------

  async proposals(limit?: number): Promise<Proposal[]> {
    const bounded = limit === undefined ? undefined : input(boundsSchema, { limit }, 'proposals limit').limit;
    return (await this.call<{ proposals: Proposal[] }>({ method: 'proposals', ...(bounded ? { limit: bounded } : {}) })).proposals;
  }

  async proposalsPage(window: Bounds = {}): Promise<Page<Proposal>> {
    const asked = this.window(window, PAGE_DEFAULTS.maxSize);
    const proposals = (await this.call<{ proposals: Proposal[] }>({ method: 'proposals', limit: asked.requested })).proposals;
    return this.page(proposals, asked);
  }

  /**
   * Make a proposal permanent by writing it to the vault as a page. The caller is the identity
   * that decided; it is recorded in the page, so it must be a real one and is required.
   */
  async accept(node: string, caller: Caller): Promise<Accepted> {
    const reference = input(nodeRef, node, 'proposal');
    return (await this.call<{ accepted: Accepted }>({ method: 'accept', node: reference, caller: decisionCaller(caller) })).accepted;
  }

  /** Decline a proposal so it is not offered again. Writes no file. */
  async reject(node: string, caller: Caller, reason?: string): Promise<KnowledgeNode> {
    const reference = input(nodeRef, node, 'proposal');
    const why = reason === undefined ? undefined : input(reasonText, reason, 'rejection reason');
    return (await this.call<{ node: KnowledgeNode }>({ method: 'reject', node: reference, caller: decisionCaller(caller), ...(why ? { reason: why } : {}) })).node;
  }

  // -- proposing knowledge ---------------------------------------------------------------------

  async create(draft: NewNode, caller: Caller): Promise<KnowledgeNode> {
    const parsed = input(nodeCreate, draft, 'new node');
    const answer = await this.call<unknown>({ method: 'create', input: parsed, caller: proposeCaller(caller) });
    return parseAnswer(responses.node, answer, 'create').node;
  }

  async update(change: { node: string; title?: string; summary?: string; content?: string; reason?: string }, caller: Caller): Promise<KnowledgeNode> {
    const parsed = input(nodeUpdate, change, 'node update');
    const answer = await this.call<unknown>({ method: 'update', input: parsed, caller: proposeCaller(caller) });
    return parseAnswer(responses.node, answer, 'update').node;
  }

  async link(edge: { from: string; type: string; to: string; note?: string }, caller: Caller): Promise<Edge> {
    const parsed = input(linkCreate, edge, 'new relationship');
    const answer = await this.call<unknown>({ method: 'link', input: parsed, caller: proposeCaller(caller) });
    return parseAnswer(responses.edge, answer, 'link').edge;
  }

  async attachEvidence(
    reference: { node: string; kind?: string; anchor?: { file: string; symbol?: string }; excerpt?: string; note?: string },
    caller: Caller,
  ): Promise<Evidence> {
    const parsed = input(evidenceCreate, reference, 'new evidence');
    const answer = await this.call<unknown>({ method: 'attach_evidence', input: parsed, caller: proposeCaller(caller) });
    return parseAnswer(responses.evidence, answer, 'attach_evidence').evidence;
  }

  async recordUse(nodes: string[], session?: string): Promise<string[]> {
    if (!Array.isArray(nodes) || nodes.length === 0 || nodes.length > 100) {
      throw new AthenaeumError('invalid_request', 'record_use needs between 1 and 100 node references');
    }
    return (await this.call<{ nodes: string[] }>({ method: 'record_use', nodes: nodes.map((node, index) => inputNode(node, `nodes.${index}`)), ...(session ? { session } : {}) })).nodes;
  }

  // -- pages -----------------------------------------------------------------------------------

  /**
   * Turn a caller's window into what the engine is actually asked for: one row more than the
   * window, so `hasMore` is read rather than guessed, clamped to the engine's own cap for the list.
   */
  private window(bounds: Bounds | undefined, cap: number): { limit: number; offset: number; requested: number } {
    const parsed = input(boundsSchema, bounds ?? {}, 'page window');
    const limit = parsed.limit ?? PAGE_DEFAULTS.size;
    const offset = parsed.offset ?? 0;
    return { limit, offset, requested: Math.min(offset + limit + 1, cap) };
  }

  /**
   * Cut a window out of what the engine answered. `total` is present only when the answer was the
   * whole list; `hasMore` is true when a further row was actually seen, or when the engine's own
   * cap cut the answer short — never when it is merely assumed.
   */
  private page<T>(items: T[], asked: { limit: number; offset: number; requested: number }): Page<T> {
    const cutByCap = items.length === asked.requested && asked.requested < asked.offset + asked.limit + 1;
    return {
      items: items.slice(asked.offset, asked.offset + asked.limit),
      offset: asked.offset,
      limit: asked.limit,
      hasMore: items.length > asked.offset + asked.limit || cutByCap,
      ...(items.length < asked.requested ? { total: items.length } : {}),
    };
  }

  /** A window over an engine list read up to `requested` rows, optionally filtered before slicing. */
  private filteredPage<T, F extends Record<string, unknown>>(
    rows: T[],
    offset: number,
    limit: number,
    requested: number,
    filters: F,
    keep: (row: T) => boolean,
  ): Page<T> & { scanned: number; scanComplete: boolean; filters: F } {
    const scanned = rows.length;
    const scanComplete = scanned < requested;
    const matching = rows.filter(keep);
    const hasMore = matching.length > offset + limit || !scanComplete;
    return {
      items: matching.slice(offset, offset + limit),
      offset,
      limit,
      hasMore,
      scanned,
      scanComplete,
      filters,
      ...(scanComplete ? { total: matching.length } : {}),
    };
  }

  // -- the command line, for what is not part of the knowledge protocol ------------------------

  private async cli(args: string[]): Promise<{ json: any; exitCode: number; stderr: string }> {
    const { stdout, stderr, exitCode } = await run(this.command, ['-C', this.workspace, '--json', ...args], {
      timeoutMs: Math.max(this.timeoutMs, 120_000),
      maxBytes: this.maxBytes,
      env: this.env,
    });
    let json: any = null;
    try {
      json = stdout.trim() ? JSON.parse(stdout) : null;
    } catch {
      /* not JSON; the exit status and stderr carry the failure */
    }
    return { json, exitCode, stderr: redact(stderr, [this.env?.ATHENAEUM_LIBRARY_TOKEN]) };
  }

  /** Identity, index and Library link of the repository. Works before `athenaeum init`. */
  async repositoryInfo(): Promise<RepoInfo> {
    const { json, exitCode, stderr } = await this.cli(['repo', 'info']);
    if (exitCode !== 0 || !json || json.error) {
      const failure = new AthenaeumError(json?.error?.code ?? 'error', redact(json?.error?.message ?? (stderr.trim() || 'could not read the repository'), [this.env?.ATHENAEUM_LIBRARY_TOKEN]));
      throw knowledgeStateError(failure, this.workspace) ?? failure;
    }
    return json as RepoInfo;
  }

  /** Run one explicit CLI sync action. Only the CLI reads the vault or contacts the Library. */
  async sync(action: SyncAction = 'sync', message?: string, resolution?: { path: string; keep: Keep }): Promise<SyncResult> {
    const remote = this.remote;
    let args: string[];
    switch (action) {
      case 'create': {
        if (!remote) throw new AthenaeumError('not_configured', 'create needs a configured Library server');
        const info = await this.repositoryInfo();
        args = ['sync', 'link', '--server', remote.server, '--create', info.name ?? this.workspace.split(/[\\/]/).filter(Boolean).at(-1) ?? 'wiki'];
        break;
      }
      case 'link':
        if (!remote?.repository) throw new AthenaeumError('not_configured', 'link needs a configured hosted repository');
        args = ['sync', 'link', '--server', remote.server, '--remote', remote.repository];
        break;
      case 'resolve':
        if (!resolution) throw new AthenaeumError('invalid_request', 'resolve needs a path and a keep choice');
        args = ['sync', 'resolve', resolution.path, '--keep', resolution.keep];
        break;
      case 'conflicts':
      case 'status':
        args = ['sync', 'status'];
        break;
      case 'sync':
        args = ['sync'];
        break;
      default:
        args = ['sync', action];
    }
    if (message && (action === 'sync' || action === 'push')) args.push('--message', message);
    const { json, exitCode, stderr } = await this.cli(args);
    if (json === null) throw new AthenaeumError('bad_response', stderr.trim() || 'the sync command did not answer with JSON');
    if (json.error) {
      const code = json.error.code ?? 'sync_error';
      const detail = redact(json.error.message ?? stderr.trim() ?? 'sync failed', [this.env?.ATHENAEUM_LIBRARY_TOKEN]);
      if (code === 'offline' || OFFLINE_PATTERN.test(detail)) {
        const offline = new KnowledgeStateError('offline', 'offline', detail, 'The Library is unreachable; local knowledge still works. Check the configured server and the network, then retry.');
        return { ok: false, exitCode, state: 'offline', result: { message: offline.detail, action: offline.action } };
      }
      throw new AthenaeumError(code, detail);
    }
    if (json.status === 'offline' || json.position === 'offline') return { ok: false, exitCode, state: 'offline', result: json };
    const conflicts = action === 'pull' || action === 'sync' ? json.pull?.conflicts : json.conflicts;
    const conflicted = json.position === 'conflicted' || (Array.isArray(conflicts) && conflicts.length > 0);
    if (conflicted) return { ok: false, exitCode, state: 'conflicted', result: json };
    if (exitCode !== 0) throw new AthenaeumError('sync_error', stderr.trim() || 'sync failed');
    return { ok: true, exitCode, state: 'done', result: json };
  }
}

function inputNode(value: unknown, what: string): string {
  return input(nodeRef, value, what);
}

/** A walk's optional bounds, validated together so a nonsense pair fails before a process starts. */
function boundedWalk(bounds: { depth?: number; maxNodes?: number; maxDepth?: number; maxNodesCap?: number }): { depth?: number; maxNodes?: number } {
  const maxDepth = bounds.maxDepth ?? 10;
  const maxNodesCap = bounds.maxNodesCap ?? 5_000;
  const walk: { depth?: number; maxNodes?: number } = {};
  if (bounds.depth !== undefined) {
    if (!Number.isInteger(bounds.depth) || bounds.depth < 0 || bounds.depth > maxDepth) throw new AthenaeumError('invalid_request', `depth must be a whole number between 0 and ${maxDepth}`);
    walk.depth = bounds.depth;
  }
  if (bounds.maxNodes !== undefined) {
    if (!Number.isInteger(bounds.maxNodes) || bounds.maxNodes < 1 || bounds.maxNodes > maxNodesCap) throw new AthenaeumError('invalid_request', `maxNodes must be a whole number between 1 and ${maxNodesCap}`);
    walk.maxNodes = bounds.maxNodes;
  }
  return walk;
}

/** A proposal is attributed to a model; the identity is bounded because it is written into pages. */
function proposeCaller(caller: Caller): Caller {
  return { model: input(callerIdentity, caller?.model, 'caller'), ...(caller?.session ? { session: caller.session } : {}) };
}

/** A decision is attributed to whoever made it; an accept or reject without one is refused. */
function decisionCaller(caller: Caller): Caller {
  return { model: input(callerIdentity, caller?.model, 'caller identity'), ...(caller?.session ? { session: caller.session } : {}) };
}
