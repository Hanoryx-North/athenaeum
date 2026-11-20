// Generate desktop command discovery from the actual TypeScript and Rust APIs.
// Service modules are intentional public operations, not every internal helper.
import ts from 'typescript'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve, relative } from 'node:path'

export const SERVICES = [
  'commands/fs', 'commands/file-sync', 'lib/knowledge-api', 'lib/knowledge-index',
  'lib/engine', 'lib/cli-tools', 'lib/project-store', 'lib/persist', 'lib/ingest',
  'lib/deep-research', 'lib/ingest-queue', 'lib/dedup-queue', 'lib/dedup-runner',
  'lib/lint', 'lib/lint-fixes', 'lib/sweep-reviews', 'lib/source-lifecycle',
  'lib/wiki-page-delete', 'lib/wiki-cleanup', 'lib/page-merge', 'lib/chat-save-to-wiki',
  'lib/url-source-import', 'lib/search', 'lib/scheduled-import', 'lib/connection-tests',
  'lib/self-update', 'lib/theme', 'lib/templates', 'lib/project-file-tree-refresh', 'lib/wiki-notes',
]

export function buildDesktopCatalog(root) {
  const configPath = resolve(root, 'tsconfig.app.json')
  const config = ts.readConfigFile(configPath, ts.sys.readFile)
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root)
  const program = ts.createProgram(parsed.fileNames, parsed.options)
  const checker = program.getTypeChecker()
  const entries = []
  const stores = []
  const location = symbol => symbol.valueDeclaration ?? symbol.declarations?.[0]
  const typeOf = symbol => checker.getTypeOfSymbolAtLocation(symbol, location(symbol))
  const schema = (type, depth = 0) => {
    if (type.getCallSignatures().length) return { type: 'never' }
    if (type.isUnion()) {
      const defined = type.types.filter(t => !(t.flags & ts.TypeFlags.Undefined))
      return { anyOf: defined.map(t => schema(t, depth + 1)) }
    }
    if (type.isStringLiteral() || type.isNumberLiteral()) return { enum: [type.value] }
    if (type.flags & ts.TypeFlags.StringLike) return { type: 'string' }
    if (type.flags & ts.TypeFlags.NumberLike) return { type: 'number' }
    if (type.flags & ts.TypeFlags.BooleanLike) return { type: 'boolean' }
    if (type.flags & ts.TypeFlags.Null) return { type: 'null' }
    if (type.symbol?.name === 'Set' || type.symbol?.name === 'ReadonlySet') return { type: 'array', format: 'set' }
    if (type.symbol?.name === 'Date') return { type: 'string', format: 'date' }
    if (checker.isArrayType(type) || checker.isTupleType(type)) return { type: 'array' }
    if (type.flags & ts.TypeFlags.Object) {
      if (depth >= 4) return { type: 'object' }
      const properties = type.getProperties().filter(p => !p.name.startsWith('__'))
      return { type: 'object', properties: Object.fromEntries(properties.map(p => [p.name, schema(typeOf(p), depth + 1)])),
        required: properties.filter(p => !(p.flags & ts.SymbolFlags.Optional)).map(p => p.name) }
    }
    return {}
  }
  const add = (name, kind, module, exportName, member, type, source) => {
    const signatures = type.getCallSignatures()
    if (!signatures.length) return
    // Callback-driven APIs are not callable with JSON; use their domain service instead.
    const signature = signatures[signatures.length - 1]
    const parameters = signature.parameters.map(parameter => {
      const parameterType = typeOf(parameter)
      const declaration = location(parameter)
      return {
        name: parameter.name,
        type: checker.typeToString(parameterType),
        optional: !!(parameter.flags & ts.SymbolFlags.Optional) || !!declaration?.questionToken || !!declaration?.initializer,
        callback: parameterType.getCallSignatures().length > 0,
        schema: schema(parameterType),
      }
    })
    const impossible = s => s.type === 'never' || s.required?.some(key => impossible(s.properties[key]))
    if (parameters.some(p => !p.optional && impossible(p.schema))) return
    entries.push({ name, kind, module, exportName, member, parameters,
      signature: checker.signatureToString(signature),
      description: ts.displayPartsToString(type.symbol?.getDocumentationComment(checker) ?? []), source })
  }
  const storeFiles = readdirSync(resolve(root, 'src/stores')).filter(f => f.endsWith('.ts'))
  const modules = [...SERVICES, ...storeFiles.map(f => `stores/${f.slice(0, -3)}`)]
  for (const module of modules) {
    const file = program.getSourceFile(resolve(root, `src/${module}.ts`))
    if (!file) continue
    const symbol = checker.getSymbolAtLocation(file)
    if (!symbol) continue
    for (const exported of checker.getExportsOfModule(symbol)) {
      if (!location(exported)) continue
      const type = typeOf(exported)
      const source = relative(root, file.fileName).replaceAll('\\', '/')
      if (module.startsWith('stores/')) {
        const getState = type.getProperty('getState')
        if (!getState) continue
        const signature = typeOf(getState).getCallSignatures()[0]
        if (!signature) continue
        const domain = module.slice(7).replace(/-store$/, '')
        stores.push({ name: domain, module, exportName: exported.name })
        for (const property of signature.getReturnType().getProperties()) {
          add(`${domain}.${property.name}`, 'store', module, exported.name, property.name, typeOf(property), source)
        }
      } else {
        const domain = module.split('/').at(-1)
        add(`${domain}.${exported.name}`, 'service', module, exported.name, null, type, source)
        // Public API objects such as knowledge-api.api share their real methods.
        if (!type.getCallSignatures().length && !exported.name.startsWith('use')) {
          for (const property of type.getProperties()) {
            add(`${domain}.${exported.name}.${property.name}`, 'service', module, exported.name, property.name, typeOf(property), source)
          }
        }
      }
    }
  }
  const rust = readFileSync(resolve(root, 'src-tauri/src/lib.rs'), 'utf8')
  const registrations = rust.match(/invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/)?.[1] ?? ''
  const native = registrations.split(',').map(s => s.trim()).filter(Boolean).map(path => {
    const name = path.split('::').at(-1)
    const file = path.includes('::') ? `src-tauri/src/${path.split('::').slice(0, -1).join('/')}.rs` : 'src-tauri/src/lib.rs'
    const contents = readFileSync(resolve(root, file), 'utf8')
    const signature = contents.match(new RegExp(`(?:pub\\s+)?(?:async\\s+)?fn\\s+${name}(?:<[^>]*>)?\\s*\\(([\\s\\S]*?)\\)\\s*(?:->|\\{)`))?.[1] ?? ''
    return { name, source: file, signature: signature.trim(), transport: 'named JSON arguments (camelCase)' }
  }).filter(entry => !entry.name.startsWith('desktop_control_'))
  return { entries, stores, native }
}

export default function desktopCatalogPlugin() {
  const id = 'virtual:desktop-catalog'
  const resolved = `\0${id}`
  let root
  return {
    name: 'athenaeum-desktop-catalog',
    configResolved(config) { root = config.root },
    resolveId(source) { if (source === id) return resolved },
    load(source) {
      if (source !== resolved) return
      const catalog = buildDesktopCatalog(root)
      this.addWatchFile(`${root}/src-tauri/src/lib.rs`)
      for (const source of new Set(catalog.native.map(command => command.source))) this.addWatchFile(`${root}/${source}`)
      const modules = [...new Set([...catalog.entries, ...catalog.stores].map(row => row.module))]
      return `export const catalog = ${JSON.stringify(catalog)};\nexport const modules = {${modules.map(m => `${JSON.stringify(m)}: () => import(${JSON.stringify(`/src/${m}.ts`)})`).join(',')}};`
    },
    handleHotUpdate(context) {
      const file = context.file.replaceAll('\\', '/')
      if (!file.includes('/src/stores/') && !file.includes('/src-tauri/src/') && !SERVICES.some(m => file.endsWith(`/src/${m}.ts`))) return
      const module = context.server.moduleGraph.getModuleById(resolved)
      if (module) { context.server.moduleGraph.invalidateModule(module); return [...context.modules, module] }
    },
  }
}
