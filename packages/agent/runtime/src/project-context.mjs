import { readdir, readFile, stat } from 'node:fs/promises'
import { join } from 'node:path'

export const stages = ['initialize', 'design', 'coding', 'debugging', 'verification', 'tools']

export async function projectSelection(root, task, configured = {}) {
  const entries = await readdir(root)
  const technologies = new Set(configured.technologies || [])
  if (entries.includes('Cargo.toml')) technologies.add('rust')
  if (entries.includes('tsconfig.json')) technologies.add('typescript')
  if (entries.includes('pyproject.toml')) technologies.add('python')
  if (entries.includes('package.json')) {
    const file = join(root, 'package.json')
    if ((await stat(file)).size <= 64 * 1024) {
      try {
        const manifest = JSON.parse(await readFile(file, 'utf8'))
        const dependencies = { ...manifest.dependencies, ...manifest.devDependencies }
        for (const name of ['react', 'next', 'vue', 'svelte', 'wrangler']) if (dependencies[name]) technologies.add(name)
      } catch (error) { if (!(error instanceof SyntaxError)) throw error }
    }
  }
  const empty = entries.every(name => name.startsWith('.') || /^(?:AGENTS|README|LICENSE|PRODUCT|DESIGN)(?:\.|$)/i.test(name))
  const stage = configured.stage || (
    empty || /\b(?:initialize|scaffold|empty project|new project|from scratch)\b/i.test(task) ? 'initialize'
      : /\b(?:debug|bug|broken|failure|crash|regression)\b/i.test(task) ? 'debugging'
      : /\b(?:verify|verification|test|benchmark|validate)\b/i.test(task) ? 'verification'
      : /\b(?:cli|tool|command|mcp|help text)\b/i.test(task) ? 'tools'
      : /\b(?:architecture|design|plan)\b/i.test(task) ? 'design' : 'coding')
  if (!stages.includes(stage)) throw new Error('Choose a valid memory task stage.')
  const project = configured.project || root.replaceAll('\\', '/')
  const values = [project, ...technologies]
  if (technologies.size > 32 || values.some(value => typeof value !== 'string' || !value.trim() || Buffer.byteLength(value) > 300 || value.includes('\0'))) throw new Error('Invalid memory project or technology selectors.')
  return { project, technologies: [...technologies], stage }
}
