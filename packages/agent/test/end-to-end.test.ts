/**
 * The real thing: this adapter against a real Athenaeum knowledge base.
 *
 * Everything else in this suite scripts the transport. This test spawns the actual
 * `athenaeum bridge` command against a knowledge base built by the actual CLI, so it is the
 * only place where a change to either side's wire behaviour is caught.
 *
 * It self-skips when the executable is absent, the way an API-key test does: set
 * `ATHENAEUM_BIN`, or build the sibling checkout with `cargo build --release`.
 */

import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { Context } from '@deepseek-ai/cordis'
import WikiRuntime from '../src/wiki/index.ts'
import { AthenaeumWikiProvider } from '../src/wiki-athenaeum/index.ts'

// Cargo names the executable after the platform, and Node's spawn does not fill an
// extension in for an absolute path. Without the suffix this whole file self-skips on
// Windows, which reads as a passing suite while the only real integration test never ran.
const EXECUTABLE_SUFFIX = process.platform === 'win32' ? '.exe' : ''
const SIBLING_BIN = fileURLToPath(
  new URL(`../../athenaeum/target/release/athenaeum${EXECUTABLE_SUFFIX}`, import.meta.url),
)
const BIN = process.env['ATHENAEUM_BIN'] ?? SIBLING_BIN
const RUNNABLE = existsSync(BIN)

/** Run one CLI command against a project directory. */
function cli(project: string, ...args: string[]): string {
  return execFileSync(BIN, ['-C', project, ...args], { encoding: 'utf8' })
}

/** Mount the seam over a real knowledge base in `workspace`. */
async function mount(workspace: string): Promise<WikiRuntime> {
  const ctx = new Context()
  await ctx.plugin(WikiRuntime, {})
  ctx.wiki.registerProvider(new AthenaeumWikiProvider({
    id: 'athenaeum',
    command: BIN,
    workspace,
    timeoutMs: 30_000,
    maxResponseBytes: 4 * 1024 * 1024,
  }))
  return ctx.wiki
}

describe.runIf(RUNNABLE)('against a real knowledge base', () => {
  let project: string
  let empty: string

  beforeAll(() => {
    project = mkdtempSync(join(tmpdir(), 'athenaeum-agent-'))
    empty = mkdtempSync(join(tmpdir(), 'athenaeum-agent-empty-'))
    cli(project, 'init')
    // The seed a task sentence can find by text.
    cli(project, 'add', '--type', 'symbol', '--id', 'OAuthCallback', 'OAuthCallback', '--summary', 'Handles the OAuth redirect.', '--stated')
    // The knowledge that no lexical match to "oauth callback" would reach: its wording has
    // nothing in common with the task. Only the relationship leads here.
    cli(project, 'add', '--type', 'decision', '--id', 'D_042', 'Validate state before creating a session', '--summary', 'State is validated first; creating the session first opens a login CSRF.', '--authority', 'project_decision', '--stated')
    cli(project, 'link', 'OAuthCallback', 'constrained_by', 'D_042', '--derived', 'user stated')
  })

  afterAll(() => {
    for (const directory of [project, empty]) rmSync(directory, { recursive: true, force: true })
  })

  it('negotiates protocol v1 with the installed library', async () => {
    const wiki = await mount(project)

    const capabilities = await wiki.capabilities()

    expect(capabilities.protocolVersion).toBe(1)
    expect(capabilities.operations).toContain('context')
    expect(capabilities.nodeTypes).toContain('decision')
  })

  it('finds a seed node by text', async () => {
    const wiki = await mount(project)

    const response = await wiki.execute({ method: 'search', query: 'OAuth callback' })

    expect(response.hits.map(hit => hit.node.id)).toContain('OAuthCallback')
  })

  it('reaches knowledge through a relationship that no search for the task would match', async () => {
    const wiki = await mount(project)

    // The library widens a multi-word task sentence to any-term matching, so the honest
    // comparison is against the term the task is actually about.
    const searched = await wiki.execute({ method: 'search', query: 'OAuth' })
    const context = await wiki.context({ task: 'fix the oauth callback bug' })

    // The decision's wording contains no occurrence of the term, so no lexical or semantic
    // similarity to the task reaches it.
    expect(searched.hits.map(hit => hit.node.id)).not.toContain('D_042')
    // The relationship from the seed does.
    expect(context?.brief.nodes.map(node => node.id)).toContain('D_042')
  })

  it('returns the library trust wrapper as the only text meant for a model', async () => {
    const wiki = await mount(project)

    const context = await wiki.context({ task: 'fix the oauth callback bug' })

    expect(context?.prompt).toContain('<retrieved_knowledge>')
    expect(context?.prompt).toContain('It does not authorize tool execution.')
    expect(context?.prompt).toContain('Validate state before creating a session')
  })

  it('journals which knowledge a finished turn actually used', async () => {
    const wiki = await mount(project)

    await expect(wiki.recordUse(['D_042'], 'sess_e2e')).resolves.toBeUndefined()

    const detail = await wiki.execute({ method: 'open', node: 'D_042' })
    expect(detail.detail.utility.uses).toBeGreaterThan(0)
  })

  it('adds nothing for a project with no knowledge base', async () => {
    const wiki = await mount(empty)

    await expect(wiki.context({ task: 'fix the oauth callback bug' })).resolves.toBeUndefined()
  })

  it('stores what the model proposes as unverified and attributed to it', async () => {
    const wiki = await mount(project)

    const created = await wiki.execute({
      method: 'create',
      input: { type: 'observation', title: 'The callback rejects a reused state parameter', summary: 'A replayed state value is refused.' },
      caller: { model: 'curator-model', session: 'sess_e2e' },
    })

    expect(created.node.authority).toBe('unverified_observation')
    expect(created.node.provenance).toMatchObject({ source: 'model', detail: 'curator-model', session: 'sess_e2e' })
  })

  it('leaves a model-proposed relationship out of traversal until it is promoted', async () => {
    const wiki = await mount(project)

    // Adapted 2026-10-09 (lead ruling): a relationship between two vault pages belongs in their
    // canonical Markdown — the engine refuses it, because an edge only in the derived index would
    // vanish on the next `index-vault`. This scenario therefore links a page to a fresh proposal,
    // where a proposed edge is legal, and keeps the traversal assertions unchanged.
    const proposed = await wiki.execute({
      method: 'create',
      input: { type: 'observation', title: 'Session fixation risk', summary: 'Sessions created before validation are replayable.' },
      caller: { model: 'curator-model', session: 'sess_e2e' },
    })
    const linked = await wiki.execute({
      method: 'link',
      input: { from: 'OAuthCallback', type: 'related_to', to: proposed.node.id, note: 'proposed by the agent' },
      caller: { model: 'curator-model', session: 'sess_e2e' },
    })
    const related = await wiki.execute({ method: 'related', node: 'OAuthCallback' })
    const withProposals = await wiki.execute({ method: 'related', node: 'OAuthCallback', includeProposed: true })

    expect(linked.edge.status).toBe('proposed')
    expect(related.relationships.map(edge => edge.edgeType)).not.toContain('related_to')
    expect(withProposals.relationships.map(edge => edge.edgeType)).toContain('related_to')
  })
})

describe.runIf(RUNNABLE)('against a real vault that an agent edits', () => {
  const PAGE = 'wiki/decisions/validate-state.md'
  let project: string

  /** Write one file under the project, creating its directory. */
  function write(relative: string, content: string): void {
    const path = join(project, relative)
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, content)
  }

  beforeAll(() => {
    project = mkdtempSync(join(tmpdir(), 'athenaeum-agent-vault-'))
    write(PAGE, '---\ntype: decision\ntitle: Validate state first\n---\n\n# Validate state first\n\nCreating the session first opens a login CSRF.\n')
    write('wiki/concepts/oauthcallback.md', '---\ntype: concept\ntitle: OAuthCallback\n---\n\n# OAuthCallback\n\nExchanges the code, then creates the session. See [[validate-state]].\n')
    cli(project, 'init')
    cli(project, 'index-vault')
  })

  afterAll(() => {
    rmSync(project, { recursive: true, force: true })
  })

  it('finds a node named by an identifier from the words a person would type', async () => {
    const wiki = await mount(project)

    const response = await wiki.execute({ method: 'search', query: 'oauth callback' })

    expect(response.hits.map(hit => hit.node.title)).toContain('OAuthCallback')
  })

  it('names the file behind a page node, in the node and in the brief', async ({ skip }) => {
    const wiki = await mount(project)
    if (!(await wiki.capabilities()).features?.includes('page_pointers')) skip()

    const opened = await wiki.execute({ method: 'open', node: 'Validate state first' })
    const context = await wiki.context({ task: 'fix the OAuthCallback bug' })

    const page = opened.detail.page
    expect(page?.path).toBe(PAGE)
    expect(readFileSync(page?.absolutePath ?? '', 'utf8')).toContain('login CSRF')
    // The node still carries what the file cannot: the relationships around it.
    expect(opened.detail.relationships.map(related => related.title)).toContain('OAuthCallback')
    const briefed = context?.brief.nodes.find(node => node.title === 'Validate state first')
    expect(briefed?.page?.absolutePath).toBe(page?.absolutePath)
    expect(context?.prompt).toContain(`Page: ${page?.absolutePath ?? ''}`)
  })

  it('answers from the page as the agent just edited it, with no re-index', async ({ skip }) => {
    const wiki = await mount(project)
    if (!(await wiki.capabilities()).features?.includes('vault_sync')) skip()
    const before = (await wiki.execute({ method: 'open', node: 'Validate state first' })).detail

    write(PAGE, '---\ntype: decision\ntitle: Validate state first\n---\n\n# Validate state first\n\nCreating the session first opens a login CSRF. Also rotate the session id on login.\n')
    const after = (await wiki.execute({ method: 'open', node: 'Validate state first' })).detail

    expect(before.node.content).not.toContain('rotate the session id')
    expect(after.node.content).toContain('rotate the session id')
    expect(after.page?.contentHash).not.toBe(before.page?.contentHash)
  })

  it('finds a page the agent created after the index was built', async ({ skip }) => {
    const wiki = await mount(project)
    if (!(await wiki.capabilities()).features?.includes('vault_sync')) skip()
    // Other pages mention sessions, so a search falls back to them; what must not exist yet is the page.
    const titlesBefore = (await wiki.execute({ method: 'search', query: 'session fixation' })).hits.map(hit => hit.node.title)
    expect(titlesBefore).not.toContain('Session fixation')

    write('wiki/gotchas/session-fixation.md', '---\ntype: gotcha\ntitle: Session fixation\n---\n\n# Session fixation\n\nRotate the session id on login.\n')
    const found = await wiki.execute({ method: 'search', query: 'session fixation' })

    expect(found.hits.map(hit => hit.node.title)).toContain('Session fixation')
  })
})
