import fs from 'node:fs';
import path from 'node:path';

/**
 * The project a directory belongs to: the nearest ancestor (or the directory itself) that holds
 * `.athenaeum/`, which is the same rule the `athenaeum` command applies when it is not told where
 * to look. `undefined` means no knowledge base is initialised above `start`, which is an ordinary
 * state, not a failure: callers degrade to an agent without a wiki.
 */
export function findWorkspace(start: string): string | undefined {
  let dir = path.resolve(start);
  for (;;) {
    try {
      if (fs.statSync(path.join(dir, '.athenaeum')).isDirectory()) return dir;
    } catch {
      /* not here; keep climbing */
    }
    const parent = path.dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}
