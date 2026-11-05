import fs from 'node:fs';
import path from 'node:path';
import { REPOSITORY_ID_PATTERN } from '@athenaeum/protocol/sync';
import { findWorkspace } from './workspace.ts';
import { z } from 'zod';

/**
 * What the person who launched an integration allows it to do. Reads are always allowed; every
 * write is an explicit grant, because the integration is driven by a model and a model is not the
 * person who owns the knowledge.
 *
 * `propose` may queue knowledge for a person to review; `sync` may talk to the Library; `review`
 * may accept or reject a proposal, which is the one path that turns a model's words into permanent
 * Markdown. Nothing here is granted by a model: the grants come from the command line or a
 * configuration file the person writes.
 */
export const PERMISSIONS = ['propose', 'review', 'sync'] as const;
export type Permission = (typeof PERMISSIONS)[number];

/**
 * The hosted Library a repository may be linked to, as the person who launched the integration
 * configured it. It is never read from model input. `repository` is what `athenaeum sync link
 * --remote` takes (`owner/name`, or the `repo_...` ID); `create` links by creating a new PRIVATE
 * hosted repository named after the configured repository. No credential lives here: the token
 * comes from `ATHENAEUM_LIBRARY_TOKEN` or the CLI's own credential store.
 */
export interface RemoteConfig {
  server: string;
  repository?: string;
}

export interface RepositoryConfig {
  /** The name a tool call uses to pick this repository. */
  name: string;
  /** Absolute path of the project directory. */
  path: string;
  /** An explicitly linked hosted repository. Never inferred from model input. */
  remote?: RemoteConfig;
}

export interface IntegrationConfig {
  /** The `athenaeum` executable (a bare name resolves through PATH). */
  command: string;
  repositories: RepositoryConfig[];
  allow: ReadonlySet<Permission>;
  /** The model name that proposals are attributed to when the client does not identify itself. */
  caller: string;
  timeoutMs: number;
  /** Identifies this server run in the provenance of what it proposes. */
  session?: string;
  capture?: 'off' | 'instruct' | 'nudge';
}

/** The canonical executable. `athenaeum-core` is a deprecated alias and nothing here uses it. */
export const DEFAULT_COMMAND = 'athenaeum';

/** `ATHENAEUM_BIN`, then the older names the engine still honours. */
export function resolveCommand(env: NodeJS.ProcessEnv = process.env): string {
  return env.ATHENAEUM_BIN || env.ATHENAEUM_CORE_BIN || env.ATHENAEUM_CLI_BIN || DEFAULT_COMMAND;
}

const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]', '::1']);

/** Tokens travel only over https, or plain http to this machine; the CLI enforces it too, this fails earlier. */
const serverSchema = z
  .string()
  .url()
  .refine((value) => {
    const url = new URL(value);
    return !url.username && !url.password;
  }, 'put no credentials in the server URL')
  .refine((value) => {
    const url = new URL(value);
    return url.protocol === 'https:' || (url.protocol === 'http:' && LOOPBACK.has(url.hostname));
  }, 'the server must be https (plain http is allowed only for localhost)');

const hostedRepository = z
  .string()
  .regex(
    /^(repo_[A-Za-z0-9_-]{1,60}|[A-Za-z0-9][A-Za-z0-9-]{0,38}\/[A-Za-z0-9][A-Za-z0-9._-]{0,99})$/,
    'use owner/name, or a repository ID (repo_ followed by up to 60 letters, digits, - or _)',
  );

/** `repositoryId` is the older spelling of `repository` for a repo_ ID; one shared rule, so a typo fails at load, not mid-sync. */
const remoteSchema = z
  .object({
    server: serverSchema,
    repository: hostedRepository.optional(),
    repositoryId: z.string().regex(REPOSITORY_ID_PATTERN, 'a repository ID is repo_ followed by up to 60 letters, digits, - or _').optional(),
  })
  .strict()
  .transform(({ server, repository, repositoryId }): RemoteConfig => ({ server, ...((repository ?? repositoryId) ? { repository: (repository ?? repositoryId)! } : {}) }));

const fileSchema = z
  .object({
    athenaeum: z.string().optional(),
    repositories: z.array(z.union([z.string(), z.object({ name: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/), path: z.string(), remote: remoteSchema.optional() })])).optional(),
    allow: z.array(z.enum([...PERMISSIONS, 'all', 'read'])).optional(),
    caller: z.string().min(1).max(80).optional(),
    capture: z.enum(['off', 'instruct', 'nudge']).optional(),
    timeoutMs: z.number().int().positive().max(600_000).optional(),
  })
  .strict();

function permissions(values: readonly string[] | undefined): Set<Permission> {
  const allowed = new Set<Permission>();
  for (const value of values ?? []) {
    if (value === 'all') PERMISSIONS.forEach((p) => allowed.add(p));
    else if (value === 'read' || value === '') continue;
    else if ((PERMISSIONS as readonly string[]).includes(value)) allowed.add(value as Permission);
    else throw new Error(`unknown permission ${JSON.stringify(value)}; use ${[...PERMISSIONS, 'all'].join(', ')}`);
  }
  return allowed;
}

function repository(spec: string | { name: string; path: string; remote?: RemoteConfig }, baseDir: string): RepositoryConfig {
  let name: string | undefined;
  let target: string;
  if (typeof spec === 'string') {
    const eq = spec.indexOf('=');
    // `name=path`, or just a path. A Windows drive letter ("C:\\x") has no `=`, so this is unambiguous.
    if (eq > 0 && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(spec.slice(0, eq))) {
      name = spec.slice(0, eq);
      target = spec.slice(eq + 1);
    } else {
      target = spec;
    }
  } else {
    name = spec.name;
    target = spec.path;
  }
  const resolved = path.resolve(baseDir, target);
  let stat: fs.Stats;
  try {
    stat = fs.statSync(resolved);
  } catch {
    throw new Error(`repository ${JSON.stringify(target)} does not exist (${resolved})`);
  }
  if (!stat.isDirectory()) throw new Error(`repository ${JSON.stringify(target)} is not a directory`);
  return { name: name ?? (path.basename(resolved).replace(/[^A-Za-z0-9._-]/g, '-') || 'repository'), path: resolved, ...(typeof spec === 'string' || !spec.remote ? {} : { remote: spec.remote }) };
}

export function loadConfig(input: {
  file?: string;
  repositories?: string[];
  allow?: string[];
  command?: string;
  caller?: string;
  session?: string;
  capture?: 'off' | 'instruct' | 'nudge';
  timeoutMs?: number;
  cwd?: string;
  env?: NodeJS.ProcessEnv;
}): IntegrationConfig {
  const cwd = input.cwd ?? process.cwd();
  const env = input.env ?? process.env;
  let fromFile: z.infer<typeof fileSchema> = {};
  let baseDir = cwd;
  if (input.file) {
    const file = path.resolve(cwd, input.file);
    let raw: unknown;
    try {
      raw = JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch (error) {
      throw new Error(`could not read the configuration file ${file}: ${error instanceof Error ? error.message : String(error)}`);
    }
    const parsed = fileSchema.safeParse(raw);
    if (!parsed.success) {
      throw new Error(`${file}: ${parsed.error.issues[0]?.path.join('.') || 'configuration'}: ${parsed.error.issues[0]?.message}`);
    }
    fromFile = parsed.data;
    baseDir = path.dirname(file);
  }

  const specs = input.repositories?.length ? input.repositories : (fromFile.repositories ?? []);
  // With nothing configured the server serves the project it was started in: the nearest ancestor
  // holding `.athenaeum/`, so one user-level registration works from any subdirectory of any project.
  const repositories = (specs.length ? specs : [findWorkspace(cwd) ?? cwd]).map((spec) => repository(spec, specs === fromFile.repositories ? baseDir : cwd));
  const seen = new Set<string>();
  for (const repo of repositories) {
    if (seen.has(repo.name)) throw new Error(`two repositories are named ${JSON.stringify(repo.name)}; name them with name=path`);
    seen.add(repo.name);
  }

  const allowSpec = input.allow ?? (env.ATHENAEUM_MCP_ALLOW ? env.ATHENAEUM_MCP_ALLOW.split(',').map((s) => s.trim()) : undefined) ?? fromFile.allow;
  const timeoutMs = input.timeoutMs ?? fromFile.timeoutMs ?? 30_000;
  if (!Number.isInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 600_000) throw new Error('--timeout-ms must be a whole number of milliseconds between 1 and 600000');
  return {
    command: input.command ?? fromFile.athenaeum ?? resolveCommand(env),
    repositories,
    allow: permissions(allowSpec),
    caller: input.caller ?? fromFile.caller ?? 'mcp-client',
    capture: input.capture ?? fromFile.capture ?? 'instruct',
    timeoutMs,
    ...(input.session ? { session: input.session } : {}),
  };
}

/** Parse `--repo a --repo name=b --allow propose,sync --config file --athenaeum /path --caller NAME --session ID`. */
export function parseArguments(argv: string[]): Parameters<typeof loadConfig>[0] & { help?: boolean; version?: boolean } {
  const out: ReturnType<typeof parseArguments> = {};
  const take = (i: number, flag: string): string => {
    const value = argv[i + 1];
    if (value === undefined || value.startsWith('--')) throw new Error(`${flag} needs a value`);
    return value;
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]!;
    const [flag, inline] = arg.startsWith('--') && arg.includes('=') ? [arg.slice(0, arg.indexOf('=')), arg.slice(arg.indexOf('=') + 1)] : [arg, undefined];
    const value = () => (inline !== undefined ? inline : (i += 1, argv[i] ?? take(i - 1, flag)));
    switch (flag) {
      case '--repo':
      case '--repository':
        (out.repositories ??= []).push(value());
        break;
      case '--allow':
        out.allow = [...(out.allow ?? []), ...value().split(',').map((s) => s.trim()).filter(Boolean)];
        break;
      case '--config':
        out.file = value();
        break;
      case '--athenaeum':
        out.command = value();
        break;
      case '--caller':
        out.caller = value();
        break;
      case '--session':
        out.session = value();
        break;
      case '--capture':
        out.capture = z.enum(['off', 'instruct', 'nudge']).parse(value());
        break;
      case '--timeout-ms':
        out.timeoutMs = Number(value());
        break;
      case '--help':
      case '-h':
        out.help = true;
        break;
      case '--version':
        out.version = true;
        break;
      default:
        throw new Error(`unknown argument ${JSON.stringify(arg)}`);
    }
  }
  return out;
}
