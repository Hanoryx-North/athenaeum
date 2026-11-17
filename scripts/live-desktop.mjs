// One persistent source session for npm start and wiki start.
import { spawn } from 'node:child_process'
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer, get } from 'node:http'
import { randomUUID } from 'node:crypto'
import { setTimeout as delay } from 'node:timers/promises'

const script = fileURLToPath(import.meta.url)
const root = resolve(dirname(script), '..')
const directory = resolve(root, '.athenaeum-dev')
const descriptor = resolve(directory, 'live.json')
const logPath = resolve(directory, 'live.log')
mkdirSync(directory, { recursive: true })

async function activeSession() {
  try {
    const saved = JSON.parse(readFileSync(descriptor, 'utf8'))
    if (!Number.isInteger(saved.port) || saved.port < 1 || saved.port > 65535 || typeof saved.token !== 'string') return
    return await new Promise(resolve => {
      const request = get({ hostname: '127.0.0.1', port: saved.port, path: '/status',
        headers: { Authorization: `Bearer ${saved.token}` } }, response => {
        let body = ''
        response.on('data', chunk => { body += chunk; if (body.length > 8192) request.destroy() })
        response.on('end', () => {
          try {
            const status = JSON.parse(body)
            resolve(response.statusCode === 200 && status.pid === saved.pid && status.root === root ? status : undefined)
          } catch { resolve() }
        })
      })
      request.setTimeout(1000, () => request.destroy())
      request.on('error', () => resolve())
    })
  } catch { return undefined }
}

async function supervise(foreground) {
  const token = randomUUID()
  const server = createServer((request, response) => {
    if (request.url !== '/status' || request.headers.authorization !== `Bearer ${token}` || request.headers.origin) {
      response.writeHead(403).end(); return
    }
    response.setHeader('Content-Type', 'application/json')
    response.end(JSON.stringify({ pid: process.pid, root, running: true }))
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const log = openSync(logPath, 'a')
  writeFileSync(log, `\n[desktop] Live session started ${new Date().toISOString()}\n`)
  const child = spawn(process.execPath, [resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'dev'], {
    cwd: root, windowsHide: true, stdio: ['ignore', foreground ? 'inherit' : log, foreground ? 'inherit' : log],
  })
  closeSync(log)
  const cleanup = () => {
    try {
      if (JSON.parse(readFileSync(descriptor, 'utf8')).token === token) rmSync(descriptor)
    } catch { /* A newer source session owns the descriptor. */ }
    server.close()
  }
  child.on('spawn', () => writeFileSync(descriptor,
    JSON.stringify({ pid: process.pid, port: server.address().port, token }), { mode: 0o600 }))
  child.on('error', error => { console.error(error.message); cleanup(); process.exitCode = 1 })
  child.on('exit', code => { cleanup(); process.exitCode = code ?? 1 })
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal))
}

try {
  const args = process.argv.slice(2)
  if (args.some(arg => !['--supervise', '--foreground'].includes(arg))) throw new Error('Usage: npm start [-- --foreground]')
  const active = await activeSession()
  if (active) {
    console.log(JSON.stringify({ live: true, reused: true, pid: active.pid, logPath }))
  } else if (args.includes('--supervise') || args.includes('--foreground')) {
    await supervise(args.includes('--foreground'))
  } else {
    const child = spawn(process.execPath, [script, '--supervise'], {
      cwd: root, detached: true, windowsHide: true, stdio: 'ignore',
    })
    let failed
    child.on('error', error => { failed = error })
    child.on('exit', code => { failed = new Error(`Live desktop launcher exited (${code}). Read ${logPath}`) })
    child.unref()
    let status
    for (let attempt = 0; attempt < 100; attempt++) {
      status = await activeSession()
      if (status || failed) break
      await delay(100)
    }
    if (!status) throw failed ?? new Error(`Live desktop did not start. Read ${logPath}`)
    console.log(JSON.stringify({ live: true, started: true, pid: status.pid, logPath }))
  }
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
}
