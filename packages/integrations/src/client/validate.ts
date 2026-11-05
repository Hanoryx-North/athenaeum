/**
 * Boundary validation. Everything a model can put into a tool call, and everything the engine
 * answers with, passes one explicit schema here: nonsense is rejected before a process is started,
 * and a malformed answer becomes a typed `bad_response` instead of an `undefined` that travels
 * further than it should. Nothing in this file decides what knowledge means.
 */

import { BRIDGE_SOURCE_SCOPES } from '@athenaeum/protocol/bridge';
import { z } from 'zod';
import { AthenaeumError } from './errors.ts';
import {
  INDEX_SCOPES,
  REVIEW_KINDS,
  REVIEW_SEVERITIES,
  type Bounds,
  type Edge,
  type EngineStatus,
  type Evidence,
  type IndexReport,
  type IndexRequest,
  type KnowledgeNode,
  type KnowledgePath,
  type NewNode,
  type ReviewFinding,
  type ReviewQuery,
  type SourceQuery,
  type SourceRecord,
} from './types.ts';

/** A node reference: an id, or an exact title. Bounded so a payload cannot be a novel. */
export const nodeRef = z.string().trim().min(1, 'a node id or title is required').max(300);
export const queryText = z.string().trim().min(1, 'a query is required').max(2_000);
export const taskText = z.string().trim().min(1, 'a task is required').max(4_000);
export const pathText = z.string().trim().min(1, 'a path is required').max(1_024);
export const reasonText = z.string().trim().min(1).max(2_000);
export const noteText = z.string().max(2_000);
export const summaryText = z.string().max(4_000);
export const contentText = z.string().max(100_000);
export const typeText = z.string().trim().min(1, 'a type is required').max(80);

/**
 * Who is deciding. Accepting a proposal writes a page into the vault, so the identity is recorded
 * there for good: it is a plain name, bounded, with no control characters and no markup.
 */
export const callerIdentity = z
  .string()
  .trim()
  .min(1, 'a caller identity is required')
  .max(80, 'a caller identity is at most 80 characters')
  .regex(/^[A-Za-z0-9][A-Za-z0-9 ._@'’-]*$/, "use letters, digits, spaces, . _ @ ' - only");

export const limit = z.number().int().min(1).max(100);
export const offset = z.number().int().min(0).max(1_000);
export const scan = z.number().int().min(1).max(1_000);

export const bounds: z.ZodType<Bounds> = z.object({ limit: limit.optional(), offset: offset.optional() }).strict();

export const reviewQuery: z.ZodType<ReviewQuery> = z
  .object({
    limit: limit.optional(),
    offset: offset.optional(),
    scan: scan.optional(),
    kind: z.enum(REVIEW_KINDS).optional(),
    blocking: z.boolean().optional(),
  })
  .strict();

export const sourceQuery: z.ZodType<SourceQuery> = z
  .object({
    limit: limit.optional(),
    offset: offset.optional(),
    scan: scan.optional(),
    prefix: pathText.optional(),
    staleOnly: z.boolean().optional(),
  })
  .strict();

export const indexRequest: z.ZodType<IndexRequest> = z
  .object({
    scope: z.enum(INDEX_SCOPES),
    paths: z.array(pathText).min(1).max(100).optional(),
  })
  .strict();

export const pathRequest = z
  .object({
    from: nodeRef,
    to: nodeRef,
    depth: z.number().int().min(1).max(10).optional(),
    maxNodes: z.number().int().min(1).max(5_000).optional(),
  })
  .strict();

/** Proposal inputs. Each is strict: an unknown field is a caller that believes something false. */
export const nodeCreate: z.ZodType<NewNode> = z
  .object({
    type: typeText,
    title: z.string().trim().min(1, 'a title is required').max(300),
    summary: summaryText.optional(),
    content: contentText.optional(),
    scope: typeText.optional(),
  })
  .strict();

export const nodeUpdate = z
  .object({
    node: nodeRef,
    title: z.string().trim().min(1).max(300).nullable().optional(),
    summary: summaryText.nullable().optional(),
    content: contentText.nullable().optional(),
    reason: reasonText.nullable().optional(),
  })
  .strict();

export const linkCreate = z
  .object({
    from: nodeRef,
    type: typeText,
    to: nodeRef,
    note: noteText.optional(),
  })
  .strict();

export const evidenceCreate = z
  .object({
    node: nodeRef,
    kind: typeText.optional(),
    anchor: z.object({ file: pathText, symbol: typeText.optional() }).strict().optional(),
    excerpt: z.string().max(20_000).optional(),
    note: noteText.optional(),
  })
  .strict();

// -- engine answers --------------------------------------------------------------------------

const provenance = z.object({ source: z.string() }).passthrough();

const knowledgeNode = z
  .object({
    id: z.string(),
    type: z.string(),
    title: z.string(),
    summary: z.string(),
    content: z.string(),
    scope: z.string(),
    authority: z.string(),
    status: z.string(),
    provenance,
    createdAt: z.string(),
    updatedAt: z.string(),
    version: z.number().int(),
  })
  .passthrough();

/**
 * The engine sends whole nodes, and the protocol says so: a path's nodes are `BridgeNode`s, not a
 * projection. Validating the whole shape keeps a lighter answer from being read as a full one.
 */
const pathNode = knowledgeNode;

/**
 * Each schema validates a whole bridge response object (`{ method?, ...payload }`), which is what
 * `call` returns, so a tool reads one field out of a parsed answer and never out of an unvalidated
 * envelope.
 */
export const responses: {
  status: z.ZodType<{ status: EngineStatus }>;
  review: z.ZodType<{ findings: ReviewFinding[] }>;
  sources: z.ZodType<{ sources: SourceRecord[] }>;
  index: z.ZodType<{ index: IndexReport }>;
  path: z.ZodType<{ path: KnowledgePath }>;
  node: z.ZodType<{ node: KnowledgeNode }>;
  edge: z.ZodType<{ edge: Edge }>;
  evidence: z.ZodType<{ evidence: Evidence }>;
} = {
  status: z.object({
    status: z
      .object({
        workspacePath: z.string(),
        initialized: z.boolean(),
        hasVault: z.boolean(),
        vaultPages: z.number().int().nonnegative(),
        nodes: z.number().int().nonnegative(),
        edges: z.number().int().nonnegative(),
        proposals: z.number().int().nonnegative(),
        findings: z.number().int().nonnegative(),
        indexedAt: z.string().nullable(),
        indexStale: z.boolean(),
        protocolVersion: z.number().int(),
        engineVersion: z.string(),
      })
      .passthrough(),
  }),
  review: z.object({
    findings: z.array(
      z
        .object({
          id: z.string(),
          kind: z.enum(REVIEW_KINDS),
          severity: z.enum(REVIEW_SEVERITIES),
          title: z.string(),
          detail: z.string(),
          nodes: z.array(z.string()),
          evidence: z.array(z.string()),
          sourcePath: z.string().nullable(),
          confidence: z.number().min(0).max(1),
          blocking: z.boolean(),
        })
        .passthrough(),
    ),
  }),
  sources: z.object({
    sources: z.array(
      z
        .object({
          path: z.string(),
          /**
           * Which scan produced this record. One record per (scope, path): the same path read by
           * both scans appears twice, and the scope is why. Never merged or de-duplicated here.
           * Required by the published contract; the vocabulary comes from the protocol package.
           */
          scope: z.enum(BRIDGE_SOURCE_SCOPES),
          bytes: z.number().int().nonnegative(),
          sha256: z.string(),
          scanned: z.boolean(),
          nodes: z.number().int().nonnegative(),
          anchors: z.number().int().nonnegative(),
          indexedAt: z.string().nullable(),
          stale: z.boolean(),
        })
        .passthrough(),
    ),
  }),
  index: z.object({
    index: z
      .object({
        vaultPages: z.number().int().nonnegative(),
        repositoryFiles: z.number().int().nonnegative(),
        symbols: z.number().int().nonnegative(),
        edges: z.number().int().nonnegative(),
        durationMs: z.number().nonnegative(),
        incomplete: z.boolean(),
        skipped: z.array(z.string()),
      })
      .passthrough(),
  }),
  path: z.object({
    // A path answer may gain optional detail (the reason a step exists, its confidence); passing
    // unknown fields through keeps a validated answer from silently losing what the wire carries.
    path: z
      .object({
        from: z.string(),
        to: z.string(),
        found: z.boolean(),
        steps: z.array(z.object({ from: z.string(), edgeType: z.string() }).passthrough()),
        nodes: z.array(pathNode),
      })
      .passthrough(),
  }),
  node: z.object({ node: knowledgeNode }),
  edge: z
    .object({
      edge: z
        .object({
          id: z.string(),
          from: z.string(),
          to: z.string(),
          type: z.string(),
          provenance,
          status: z.string(),
          scope: z.string(),
          createdAt: z.string(),
        })
        .passthrough(),
    }),
  evidence: z
    .object({
      evidence: z
        .object({
          id: z.string(),
          node: z.string(),
          kind: z.string(),
          createdAt: z.string(),
        })
        .passthrough(),
    }),
};

/** Parse an engine payload, turning any violation into a typed failure that names the field. */
export function parseAnswer<T>(schema: z.ZodType<T>, value: unknown, method: string): T {
  const parsed = schema.safeParse(value);
  if (parsed.success) return parsed.data;
  const issue = parsed.error.issues[0];
  const where = issue?.path.length ? issue.path.join('.') : 'the answer';
  throw new AthenaeumError('bad_response', `the engine's \`${method}\` answer is not the protocol's shape: ${where}: ${issue?.message ?? 'invalid'}`);
}

/** Validate one input at the boundary and report the first problem as a typed request error. */
export function input<T>(schema: z.ZodType<T>, value: unknown, what: string): T {
  const parsed = schema.safeParse(value);
  if (parsed.success) return parsed.data;
  const issue = parsed.error.issues[0];
  const where = issue?.path.length ? `\`${issue.path.join('.')}\`` : what;
  throw new AthenaeumError('invalid_request', `${what}: ${where}: ${issue?.message ?? 'invalid'}`);
}

export { z };
