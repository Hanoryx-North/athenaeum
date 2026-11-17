import { readdirSync, readlinkSync, realpathSync, rmSync, symlinkSync } from 'node:fs'
import { isAbsolute, join, sep } from 'node:path'

// Tauri copies resource symlinks as regular files. npm resolves its modules
// relative to its real launcher path, so restore the pinned runtime's links.
export function restoreResourceSymlinks(source, destination) {
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    const from = join(source, entry.name)
    const to = join(destination, entry.name)
    if (entry.isSymbolicLink()) {
      rmSync(to, { recursive: true, force: true })
      symlinkSync(readlinkSync(from), to)
    } else if (entry.isDirectory()) restoreResourceSymlinks(from, to)
  }
}

export function verifyRelativeSymlinks(output) {
  const boundary = realpathSync(output)
  const walk = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name)
      if (entry.isSymbolicLink()) {
        const destination = realpathSync(path) // Also rejects dangling links.
        if (isAbsolute(readlinkSync(path)) || (destination !== boundary && !destination.startsWith(boundary + sep))) {
          throw new Error(`Bundle symlink cannot be relocated: ${path}`)
        }
      } else if (entry.isDirectory()) walk(path)
    }
  }
  walk(output)
}
