import { mkdir, readFile, readdir, realpath, rm, stat, writeFile } from 'node:fs/promises'
import { dirname, isAbsolute, relative, resolve } from 'node:path'
import { spawn } from 'node:child_process'

const MAX_FILE = 1024 * 1024
export const MAX_OUTPUT = 64 * 1024
const skipped = new Set(['.git', '.athenaeum', 'node_modules', 'target', 'dist'])
const schema = (properties, required) => ({ type: 'object', properties, required, additionalProperties: false })
const text = { type: 'string' }
export const localTools = [
  ['read_file', 'Read a UTF-8 file inside the workspace (up to 1 MiB).', schema({ path: text }, ['path'])],
  ['list_files', 'List files inside a workspace directory (bounded, excludes build and private state directories).', schema({ path: text }, [])],
  ['search', 'Find literal text in workspace files. Returns file, line and matching text.', schema({ query: text, path: text }, ['query'])],
  ['write_file', 'Create or replace a workspace UTF-8 file after approval.', schema({ path: text, content: text }, ['path', 'content'])],
  ['apply_patch', 'Apply *** Begin Patch / Add File / Update File / Delete File / End Patch text. Update hunks use space context, -removed and +added lines.', schema({ patch: text }, ['patch'])],
  ['exec_command', 'Run a shell command with user privileges after explicit approval. No OS sandbox. Execution is bounded by timeout_ms (default 120000, maximum 600000).', schema({ cmd: text, workdir: text, timeout_ms: { type: 'integer', minimum: 1, maximum: 600000 } }, ['cmd'])],
  ['view_image', 'Read a workspace PNG, JPEG, GIF or WebP image for the model.', schema({ path: text }, ['path'])],
].map(([name, description, parameters]) => ({ type: 'function', name, description, parameters }))

function string(value, name, allowEmpty = false) {
  if (typeof value !== 'string' || (!allowEmpty && !value.trim()) || value.includes('\0')) throw new Error(`${name} must be a valid string.`)
  return value
}
export async function workspacePath(root, input = '.', writable = false) {
  string(input, 'path')
  const path = resolve(root, input)
  const inside = (candidate) => { const rel = relative(root, candidate); return !isAbsolute(rel) && rel !== '..' && !rel.startsWith('../') && !rel.startsWith('..\\') }
  if (!inside(path)) throw new Error('Path is outside the selected workspace.')
  // Check existing ancestors, including symlinks on paths not yet created.
  let ancestor = path, canonicalPath
  while (true) {
    try {
      const canonicalAncestor = await realpath(ancestor)
      if (!inside(canonicalAncestor)) throw new Error('Path resolves outside the selected workspace.')
      canonicalPath = resolve(canonicalAncestor, relative(ancestor, path))
      break
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
      const parent = dirname(ancestor)
      if (parent === ancestor) throw error
      ancestor = parent
    }
  }
  if (writable && canonicalPath !== path) throw new Error('File edits through symlinks are not supported. Use the canonical workspace path.')
  const segments = relative(root, canonicalPath).split(/[\\/]/)
  if (segments.some((part) => part === '.git' || part === '.athenaeum')) throw new Error('Private project state is not accessible through file tools.')
  if (writable && (path === root || segments[0] === 'wiki')) throw new Error('Use wiki proposals for accepted wiki pages; choose a project file for edits.')
  return canonicalPath
}
async function readText(path) {
  if ((await stat(path)).size > MAX_FILE) throw new Error('File exceeds the 1 MiB read limit.')
  return readFile(path, 'utf8')
}
async function files(root, directory, signal) {
  const result = [], stack = [directory]
  let visited = 0
  while (stack.length && visited < 10000 && result.length < 2000) {
    signal.throwIfAborted()
    const dir = stack.pop()
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      if (++visited > 10000 || result.length >= 2000) break
      if (skipped.has(entry.name) || entry.isSymbolicLink()) continue
      const path = resolve(dir, entry.name)
      if (entry.isDirectory()) stack.push(path)
      else if (entry.isFile()) result.push(path)
    }
  }
  return { paths: result.sort(), truncated: stack.length > 0 || visited >= 10000 || result.length >= 2000 }
}
export function cleanEnvironment() {
  return Object.fromEntries(Object.entries(process.env).filter(([key]) => !/KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL|ATHENAEUM_AGENT|ATHENAEUM_LLM/i.test(key)))
}
export function command(cmd, cwd, timeoutMs, signal) {
  return new Promise((resolveResult, reject) => {
    signal.throwIfAborted()
    const windows = process.platform === 'win32'
    const child = spawn(windows ? (process.env.ComSpec || 'cmd.exe') : '/bin/sh', windows ? ['/d', '/s', '/c', `"${cmd}"`] : ['-c', cmd],
      { cwd, env: cleanEnvironment(), detached: !windows, windowsHide: true, windowsVerbatimArguments: windows, stdio: ['ignore', 'pipe', 'pipe'] })
    let output = '', truncated = false, timedOut = false
    const collect = (chunk) => { const remaining = MAX_OUTPUT - Buffer.byteLength(output); if (remaining > 0) output += chunk.subarray(0, remaining).toString(); if (chunk.length > remaining) truncated = true }
    child.stdout.on('data', collect); child.stderr.on('data', collect)
    const kill = () => {
      if (windows) {
        const killer = spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true })
        killer.on('error', () => child.kill())
      } else { try { process.kill(-child.pid, 'SIGKILL') } catch (error) { if (error.code !== 'ESRCH') child.kill('SIGKILL') } }
    }
    const timer = setTimeout(() => { timedOut = true; kill() }, timeoutMs)
    signal.addEventListener('abort', kill, { once: true })
    const cleanup = () => { clearTimeout(timer); signal.removeEventListener('abort', kill) }
    child.on('error', (error) => { cleanup(); reject(error) })
    child.on('close', (code) => { cleanup(); resolveResult({ output, exitCode: code, timedOut, truncated }) })
  })
}

// KMP keeps repeated context lines from making hunk matching quadratic.
function findLines(lines, needle, start) {
  if (!needle.length) return start
  const prefix = new Array(needle.length).fill(0)
  for (let index = 1, matched = 0; index < needle.length; index++) {
    while (matched && needle[index] !== needle[matched]) matched = prefix[matched - 1]
    if (needle[index] === needle[matched]) matched++
    prefix[index] = matched
  }
  for (let index = start, matched = 0; index < lines.length; index++) {
    while (matched && lines[index] !== needle[matched]) matched = prefix[matched - 1]
    if (lines[index] === needle[matched]) matched++
    if (matched === needle.length) return index - matched + 1
  }
  return -1
}

// Parse and validate every path and hunk before applying any file change.
export async function planPatch(root, patch) {
  string(patch, 'patch')
  if (Buffer.byteLength(patch) > MAX_FILE) throw new Error('Patch exceeds 1 MiB.')
  const lines = patch.replace(/\r\n/g, '\n').trimEnd().split('\n')
  if (lines.shift() !== '*** Begin Patch' || lines.pop() !== '*** End Patch') throw new Error('Invalid patch envelope.')
  const edits = []
  let totalBytes = 0
  while (lines.length) {
    const header = /^\*\*\* (Add|Update|Delete) File: (.+)$/.exec(lines.shift())
    if (!header) throw new Error('Expected a patch file header.')
    const [, kind, name] = header
    const path = await workspacePath(root, name, true)
    if (edits.some((edit) => edit.path === path)) throw new Error('A patch may edit each path only once.')
    let before = null
    try { before = await readText(path) } catch (error) { if (error.code !== 'ENOENT') throw error }
    if (kind === 'Add' && before !== null) throw new Error(`File already exists: ${name}`)
    if (kind !== 'Add' && before === null) throw new Error(`File does not exist: ${name}`)
    const body = []
    while (lines.length && !lines[0].startsWith('*** ')) body.push(lines.shift())
    let after = null
    if (kind === 'Add') {
      if (!body.length || body.some((line) => !line.startsWith('+'))) throw new Error('Add File requires + prefixed content.')
      after = body.map((line) => line.slice(1)).join('\n') + '\n'
    } else if (kind === 'Update') {
      if (!body.length) throw new Error('Update requires a hunk.')
      const original = before.replace(/\r\n/g, '\n').split('\n'), updated = []
      let cursor = 0, index = 0
      while (index < body.length) {
        if (body[index].startsWith('@@')) index++
        const old = [], replacement = []
        while (index < body.length && !body[index].startsWith('@@')) {
          const line = body[index++]
          if (![' ', '+', '-'].includes(line[0])) throw new Error('Invalid patch hunk line.')
          if (line[0] !== '+') old.push(line.slice(1))
          if (line[0] !== '-') replacement.push(line.slice(1))
        }
        if (!old.length && !replacement.length) throw new Error('Empty patch hunk.')
        const found = findLines(original, old, cursor)
        if (found < 0) throw new Error(`Patch context was not found in ${name}.`)
        updated.push(...original.slice(cursor, found), ...replacement); cursor = found + old.length
      }
      updated.push(...original.slice(cursor)); after = updated.join('\n')
    } else if (body.length) throw new Error('Delete File must not contain hunks.')
    totalBytes += Buffer.byteLength(before || '') + Buffer.byteLength(after || '')
    if (edits.length >= 100 || totalBytes > 4 * MAX_FILE) throw new Error('Patch exceeds the 100-file / 4 MiB edit limit.')
    edits.push({ path, before, after })
  }
  if (!edits.length) throw new Error('Patch contains no changes.')
  return edits
}
async function commitEdits(root, edits) {
  // Recheck after approval: a file or symlink may have changed while the person reviewed it.
  for (const edit of edits) {
    await workspacePath(root, edit.path, true)
    let current = null
    try { current = await readText(edit.path) } catch (error) { if (error.code !== 'ENOENT') throw error }
    if (current !== edit.before) throw new Error('File changed while awaiting approval. Read it again before editing.')
  }
  const done = []
  try {
    for (const edit of edits) {
      if (edit.after === null) await rm(edit.path)
      else { await mkdir(dirname(edit.path), { recursive: true }); await writeFile(edit.path, edit.after) }
      done.push(edit)
    }
  } catch (error) {
    for (const edit of done.reverse()) {
      if (edit.before === null) await rm(edit.path, { force: true })
      else await writeFile(edit.path, edit.before)
    }
    throw error
  }
  return { changed: edits.map((edit) => relative(root, edit.path)) }
}
export async function runLocal(name, args, context) {
  const { root, signal, approve } = context
  signal.throwIfAborted()
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Tool arguments must be an object.')
  if (name === 'exec_command') {
    const cmd = string(args.cmd, 'cmd')
    const cwd = await workspacePath(root, args.workdir || '.')
    const timeout = args.timeout_ms ?? 120000
    if (!Number.isInteger(timeout) || timeout < 1 || timeout > 600000) throw new Error('timeout_ms must be between 1 and 600000.')
    await approve('commandExecution', cmd); signal.throwIfAborted()
    return command(cmd, cwd, timeout, signal)
  }
  if (name === 'apply_patch' || name === 'write_file') {
    let edits
    if (name === 'apply_patch') edits = await planPatch(root, args.patch)
    else {
      const path = await workspacePath(root, args.path, true)
      const after = string(args.content, 'content', true)
      if (Buffer.byteLength(after) > MAX_FILE) throw new Error('Content exceeds 1 MiB.')
      let before = null
      try { before = await readText(path) } catch (error) { if (error.code !== 'ENOENT') throw error }
      edits = [{ path, before, after }]
    }
    await approve('fileChange', JSON.stringify(edits.map(({ path, before, after }) => ({ path: relative(root, path), before, after }))))
    signal.throwIfAborted()
    return commitEdits(root, edits)
  }
  const path = await workspacePath(root, args.path || '.')
  if (name === 'read_file') return { text: await readText(path) }
  if (name === 'view_image') {
    if ((await stat(path)).size > 5 * MAX_FILE) throw new Error('Image exceeds 5 MiB.')
    const data = await readFile(path)
    const mime = data.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])) ? 'image/png'
      : data[0] === 255 && data[1] === 216 ? 'image/jpeg'
      : data.subarray(0, 3).toString() === 'GIF' ? 'image/gif'
      : data.subarray(0, 4).toString() === 'RIFF' && data.subarray(8, 12).toString() === 'WEBP' ? 'image/webp' : null
    if (!mime) throw new Error('Unsupported image format.')
    return { image: `data:${mime};base64,${data.toString('base64')}` }
  }
  if (name === 'list_files' || name === 'search') {
    const listing = (await stat(path)).isFile() ? { paths: [path], truncated: false } : await files(root, path, signal)
    if (name === 'list_files') return { files: listing.paths.map((file) => relative(root, file)), truncated: listing.truncated }
    const query = string(args.query, 'query'), matches = []
    let bytes = 0
    for (const file of listing.paths) {
      signal.throwIfAborted()
      let content
      try { content = await readText(file) } catch (error) { if (error.code === 'ENOENT' || /exceeds/.test(error.message)) continue; throw error }
      if (content.includes('\0')) continue
      for (const [index, line] of content.split('\n').entries()) {
        if (!line.includes(query)) continue
        const match = { path: relative(root, file), line: index + 1, text: line.slice(0, 1000) }
        matches.push(match); bytes += JSON.stringify(match).length
        if (matches.length >= 200 || bytes > MAX_OUTPUT) return { matches, truncated: true }
      }
    }
    return { matches, truncated: listing.truncated }
  }
  throw new Error(`Unknown local tool: ${name}`)
}
