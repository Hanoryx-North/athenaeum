import path from 'node:path';
import { findWorkspace } from '../client/workspace.ts';
import type { ConnectContext } from './types.ts';

/** An explicit connection is to the wiki the person selected, not the host's cwd. */
export function connectionWorkspace(value: string): string {
  const workspace = findWorkspace(path.resolve(value));
  if (!workspace) throw new Error(`No Athenaeum wiki at ${value}. Run \`wiki setup -C <project>\` first.`);
  return workspace;
}

/** Keep the selected wiki when reconnecting without an explicit replacement. */
export function savedWorkspace(args: unknown): string | undefined {
  if (!Array.isArray(args)) return undefined;
  const at = args.indexOf('--repo');
  const value: unknown = at < 0 ? undefined : args[at + 1];
  return typeof value === 'string' ? value : undefined;
}

export function workspaceArgs(ctx: Pick<ConnectContext, 'workspace'>): string[] {
  return ctx.workspace ? ['--repo', ctx.workspace] : [];
}
