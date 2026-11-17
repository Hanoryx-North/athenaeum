import { readFile, readdir } from 'node:fs/promises'
import { dirname, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { SERVICES } from './desktop-catalog.mjs'

const root = fileURLToPath(new URL('../', import.meta.url))
const layers = ['src', 'packages/protocol/src', 'packages/integrations/src', 'packages/agent/src', 'packages/agent/runtime/src']
const inside = (path, directory) => path === directory || path.startsWith(`${directory}/`)
const normalized = path => path.replaceAll('\\', '/')

export function boundaryError(file, specifier) {
  file = normalized(file)
  const layer = layers.find(directory => inside(file, directory))
  if (!layer || /\.test\.[cm]?[jt]sx?$/.test(file)) return null
  if (layer === 'packages/agent/runtime/src' && !specifier.startsWith('.') && !specifier.startsWith('node:')) return 'Keep the native runtime dependency-free; use Node APIs or an owned runtime module.'
  if (specifier.startsWith('@athenaeum/protocol')) return null
  const packages = { 'athenaeum-agent': 'packages/agent/src', 'athenaeum-integrations': 'packages/integrations/src' }
  const packageTarget = Object.entries(packages).find(([name]) => specifier === name || specifier.startsWith(`${name}/`))?.[1]
  const target = packageTarget || (specifier.startsWith('.') ? normalized(relative(root, resolve(root, dirname(file), specifier)))
    : specifier.startsWith('@/') ? `src/${specifier.slice(2)}` : null)
  if (target && layers.some(directory => inside(target, directory)) && !inside(target, layer)) return 'Use the versioned protocol and bridge across source boundaries.'
  return null
}

export function importSpecifiers(file, source) {
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true)
  const imports = []
  const visit = node => {
    const specifier = (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) ? node.moduleSpecifier
      : ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword || node.expression.getText(tree) === 'require') ? node.arguments[0]
      : ts.isNewExpression(node) && node.expression.getText(tree) === 'URL' && node.arguments?.[1]?.getText(tree) === 'import.meta.url' ? node.arguments[0] : null
    if (specifier && ts.isStringLiteralLike(specifier)) imports.push({ specifier: specifier.text, line: tree.getLineAndCharacterOfPosition(specifier.getStart(tree)).line + 1 })
    ts.forEachChild(node, visit)
  }
  visit(tree)
  return imports
}

async function files(directory) {
  const entries = await readdir(resolve(root, directory), { withFileTypes: true })
  const nested = await Promise.all(entries.map(entry => entry.isDirectory() ? files(`${directory}/${entry.name}`) : /\.[cm]?[jt]sx?$/.test(entry.name) ? [`${directory}/${entry.name}`] : []))
  return nested.flat()
}

export async function unusedFrontendFiles(directory = root, services = SERVICES) {
  const config = ts.readConfigFile(resolve(directory, 'tsconfig.app.json'), ts.sys.readFile)
  if (config.error) throw new Error(ts.flattenDiagnosticMessageText(config.error.messageText, '\n'))
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, directory)
  if (parsed.errors.length) throw new Error(parsed.errors.map(error => ts.flattenDiagnosticMessageText(error.messageText, '\n')).join('\n'))
  const production = parsed.fileNames.filter(file => !/\.d\.ts$|\.test\.[jt]sx?$|[/\\](?:test-helpers|__tests__)[/\\]/.test(file))
  const graph = new Map(await Promise.all(production.map(async file => [file,
    importSpecifiers(file, await readFile(file, 'utf8')).flatMap(({ specifier }) => {
      const target = ts.resolveModuleName(specifier, file, parsed.options, ts.sys).resolvedModule
      return target ? [target.resolvedFileName] : []
    }),
  ])))
  const visited = new Set()
  const visit = file => {
    if (visited.has(file)) return
    visited.add(file)
    for (const dependency of graph.get(file) ?? []) visit(dependency)
  }
  // The live command catalog exposes services independently of mounted UI.
  visit(resolve(directory, 'src/main.tsx'))
  for (const service of services) visit(resolve(directory, `src/${service}.ts`))
  for (const file of production) if (inside(normalized(relative(directory, file)), 'src/stores')) visit(file)
  return production.filter(file => !visited.has(file)).map(file => normalized(relative(directory, file))).sort()
}

export async function checkBoundaries() {
  const problems = []
  for (const file of await unusedFrontendFiles()) problems.push(`${file}: No application, worker or desktop-service entry point reaches this module. Remove it or connect it to its intended flow.`)
  for (const file of (await Promise.all(layers.map(files))).flat()) {
    for (const { specifier, line } of importSpecifiers(file, await readFile(resolve(root, file), 'utf8'))) {
      const error = boundaryError(file, specifier)
      if (error) problems.push(`${file}:${line}: ${specifier}: ${error}`)
    }
  }
  const engine = await readFile(resolve(root, 'crates/athenaeum-core/Cargo.toml'), 'utf8')
  for (const name of ['tauri', 'wry', 'egui', 'iced', 'reqwest', 'hyper', 'ureq']) {
    if (new RegExp(`^${name}(?:[-\\w]*)\\s*=`, 'm').test(engine)) problems.push(`crates/athenaeum-core/Cargo.toml: Keep UI and network dependencies in consumers, outside the knowledge engine (${name}).`)
  }
  return problems
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const problems = await checkBoundaries()
  if (problems.length) { console.error(problems.join('\n')); process.exitCode = 1 }
  else console.log('Source boundaries passed.')
}
