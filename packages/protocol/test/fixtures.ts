import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

export const root = join(dirname(fileURLToPath(import.meta.url)), '..')

/** Reads a fixture under `fixtures/`, e.g. `fixture('sync/v1/paths.json')`. */
export function fixture<T = any>(relative: string): T {
  return JSON.parse(readFileSync(join(root, 'fixtures', relative), 'utf8')) as T
}
