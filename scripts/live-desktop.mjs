// Managed live source session. Vite handles frontend edits; Tauri watches Rust.
import { spawn } from 'node:child_process'
import { mkdirSync, openSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'node:http'
import { randomUUID } from 'node:crypto'

const script = fileURLToPath(import.meta.url)
const root = resolve(dirname(script), '..')
const directory = resolve(root, '.athenaeum-dev')
const descriptor = resolve(directory, 'live.json')
const logPath = resolve(directory, 'live.log')
mkdirSync(directory, { recursive: true })

const token = randomUUID()
const server = createServer((request, response) => {
  if (request.url !== '/status' || request.headers.authorization !== `Bearer ${token}` || request.headers.origin) {
    response.writeHead(403).end(); return
  }
  response.setHeader('Content-Type', 'application/json')
  response.end(JSON.stringify({ pid: process.pid, root, running: true }))
})
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
writeFileSync(descriptor, JSON.stringify({ pid: process.pid, port: server.address().port, token }), { mode: 0o600 })
const log = openSync(logPath, 'a')
const child = spawn(process.execPath, [resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'dev'], {
  cwd: root, windowsHide: true, stdio: ['ignore', log, log],
})
child.on('error', error => { console.error(error); server.close(); process.exitCode = 1 })
child.on('exit', code => {
  try {
    if (JSON.parse(readFileSync(descriptor, 'utf8')).token === token) rmSync(descriptor)
  } catch { /* A newer source session owns the descriptor. */ }
  server.close()
  process.exitCode = code ?? 1
})
