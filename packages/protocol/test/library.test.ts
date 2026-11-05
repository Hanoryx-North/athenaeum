import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { describe, it } from 'node:test'
import {
  API_SERVICE_NAME, COLLABORATOR_ROLES, DEVICE_POLL_ERROR_CODES, LIBRARY_ERROR_CODES, LIBRARY_ERROR_STATUS,
  REPOSITORY_ROLES, REPOSITORY_SORTS, REPOSITORY_VISIBILITIES, SYNC_ERROR_CODES, TOKEN_SCOPES, TOKEN_SECRET_PREFIX,
  adminRoutes, canonicalManifest, canonicalRevision, isCompatibleServer, isRepositoryId, isRevisionId, isSyncPath,
  libraryRoutes, syncRoutes, validateSyncContent, withQuery,
} from '../src/index.js'
import type {
  AdminOperator, AdminRepository, AdminRepositoryPatch, AdminStats, AdminUser, ApiErrorEnvelope, ApiInfo, AuthResponse,
  CollaboratorRequest, CollaboratorView, Comparison, CreateTokenRequest, CreatedToken, CreateRepository, CurrentUser,
  DeleteAccountRequest, DeviceApproveRequest, DeviceAuthorizationRequest, DeviceCode, DeviceCodeLookup,
  DeviceDenyRequest, DeviceTokenGrant, DeviceTokenRequest, DiffHunk, DiffLine, FileContent, FileDiff, ForkOrigin,
  ItemList, LoginRequest, Page, PasswordChange, ProfilePatch, PublicStats, PublicUser, PushRequest, PushResponse,
  RegisterRequest, RepositoryPage, RepositoryView, RevisionList, RevisionView, SelfUser, TokenView, TreeEntry,
  TreeListing, UpdateRepository, UserProfile, ManifestEntry,
} from '../src/index.js'
import { fixture } from './fixtures.ts'
import {
  type Check, any, arrayOf, assertShape, bool, int, lit, nullable, num, object, oneOf, optional, pattern, recordOf, str,
} from './shape.ts'

const sha256 = (text: string) => createHash('sha256').update(text, 'utf8').digest('hex')
const examples = fixture<any>('library/v1/examples.json')

const publicUser = object<PublicUser>({ username: str, displayName: str, bio: str, createdAt: str })
const selfUser = object<SelfUser>({ username: str, displayName: str, bio: str, createdAt: str, id: str, email: nullable(str) })
const repoId = pattern(/^repo_[A-Za-z0-9_-]{1,60}$/)
const revId = pattern(/^rev_[0-9a-f]{64}$/)
const sha = pattern(/^[0-9a-f]{64}$/)
const iso = pattern(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z$/)
const visibility = oneOf(REPOSITORY_VISIBILITIES)
const scopes = arrayOf(oneOf(TOKEN_SCOPES))
const manifestEntry = object<ManifestEntry>({ path: str, sha256: sha, size: int })
const page = <T>(item: Check) => object<Page<T>>({ items: arrayOf(item), total: int, page: int, perPage: int })
const apiError = (code?: string) => object<ApiErrorEnvelope>({
  error: object<ApiErrorEnvelope['error']>({ code: code ? lit(code) : oneOf(LIBRARY_ERROR_CODES), message: str, details: optional(recordOf(any)) }),
  requestId: optional(str),
})
const repositoryView = object<RepositoryView>({
  id: repoId, owner: publicUser, name: str, fullName: str, description: str, visibility, head: nullable(revId),
  forkedFrom: nullable(object<ForkOrigin>({ id: repoId, fullName: str })), forks: int, fileCount: int, sizeBytes: int,
  createdAt: iso, updatedAt: iso, role: oneOf(REPOSITORY_ROLES),
})
const revisionView = object<RevisionView>({
  id: revId, parent: nullable(revId), treeHash: sha, author: str, message: str, createdAt: iso,
  mergedFrom: optional(revId), manifest: optional(arrayOf(manifestEntry)),
})
const tokenView = object<TokenView>({
  id: str, name: str, prefix: str, scopes, repositories: nullable(arrayOf(repoId)), createdAt: iso, lastUsedAt: nullable(iso), expiresAt: nullable(iso),
})
const adminRepository = object<AdminRepository>({
  id: repoId, owner: str, name: str, fullName: str, visibility, head: nullable(revId), fileCount: int, sizeBytes: int, createdAt: iso, updatedAt: iso,
})

describe('Library API vocabulary', () => {
  it('is additive over sync: every sync error code is a library error code with an HTTP status', () => {
    for (const code of SYNC_ERROR_CODES) assert.ok((LIBRARY_ERROR_CODES as readonly string[]).includes(code), code)
    for (const code of DEVICE_POLL_ERROR_CODES) assert.ok((LIBRARY_ERROR_CODES as readonly string[]).includes(code), code)
    assert.deepEqual(Object.keys(LIBRARY_ERROR_STATUS).sort(), [...LIBRARY_ERROR_CODES].sort())
    assert.equal(new Set(LIBRARY_ERROR_CODES).size, LIBRARY_ERROR_CODES.length)
  })
  it('maps the codes the contract names to the statuses the contract names', () => {
    const expected: Record<string, number> = {
      stale_base: 409, invalid_path: 422, invalid_content: 422, request_too_large: 413, unauthorized: 401, forbidden: 403, not_found: 404,
      authorization_pending: 400, slow_down: 400, expired_token: 400, access_denied: 400, rate_limited: 429, internal_error: 500,
    }
    for (const [code, status] of Object.entries(expected)) assert.equal(LIBRARY_ERROR_STATUS[code as keyof typeof LIBRARY_ERROR_STATUS], status, code)
  })
  it('names the service, the scopes, roles, sorts', () => {
    assert.equal(API_SERVICE_NAME, 'athenaeum-api')
    assert.deepEqual([...TOKEN_SCOPES], ['repo:read', 'repo:write', 'repo:admin'])
    assert.deepEqual([...REPOSITORY_ROLES], ['none', 'read', 'write', 'admin', 'owner'])
    assert.deepEqual([...COLLABORATOR_ROLES], ['read', 'write', 'admin'])
    assert.deepEqual([...REPOSITORY_VISIBILITIES], ['private', 'public'])
    assert.deepEqual([...REPOSITORY_SORTS], ['updated', 'created', 'name', 'forks'])
    assert.equal(TOKEN_SECRET_PREFIX, 'atn_')
  })
})

describe('shared vocabulary (fixtures/library/v1/vocabulary.json, also read by the Rust crate)', () => {
  const vocabulary = fixture<any>('library/v1/vocabulary.json')
  it('matches the exported constants exactly, in order', () => {
    assert.deepEqual(vocabulary.tokenScopes, [...TOKEN_SCOPES])
    assert.equal(vocabulary.tokenSecretPrefix, TOKEN_SECRET_PREFIX)
    assert.deepEqual(vocabulary.repositoryVisibilities, [...REPOSITORY_VISIBILITIES])
    assert.deepEqual(vocabulary.repositoryRoles, [...REPOSITORY_ROLES])
    assert.deepEqual(vocabulary.collaboratorRoles, [...COLLABORATOR_ROLES])
    assert.deepEqual(vocabulary.repositorySorts, [...REPOSITORY_SORTS])
    assert.deepEqual(vocabulary.devicePollErrorCodes, [...DEVICE_POLL_ERROR_CODES])
    assert.deepEqual(vocabulary.syncErrorCodes, [...SYNC_ERROR_CODES])
    assert.deepEqual(vocabulary.errorStatus, { ...LIBRARY_ERROR_STATUS })
  })
})

describe('Library API example payloads (fixtures/library/v1/examples.json)', () => {
  it('info: a compatible athenaeum-api with the full limits set', () => {
    assertShape(object<ApiInfo>({
      service: lit(API_SERVICE_NAME), apiVersion: lit(1), syncProtocol: lit(1),
      limits: object<ApiInfo['limits']>({ maxFileBytes: int, maxRepositoryBytes: int, maxPushBytes: int, maxChangesPerPush: int }),
      registration: oneOf(['open', 'closed']),
    }), examples.info, 'info')
    assert.equal(isCompatibleServer(examples.info), true)
  })
  it('stats, users, profile', () => {
    assertShape(object<PublicStats>({ users: int, publicRepositories: int, revisions: int }), examples.stats, 'stats')
    assertShape(publicUser, examples.users.publicUser, 'publicUser')
    assertShape(selfUser, examples.users.selfUser, 'selfUser')
    assertShape(object<CurrentUser>({ username: str, displayName: str, bio: str, createdAt: str, id: str, email: nullable(str), via: oneOf(['session', 'token']) }), examples.users.currentUser, 'currentUser')
    assertShape(object<UserProfile>({ username: str, displayName: str, bio: str, createdAt: str, publicRepositories: int }), examples.users.profile, 'profile')
    assertShape(object<AuthResponse>({ user: selfUser }), examples.users.authResponse, 'authResponse')
    assertShape(object<RegisterRequest>({ username: str, email: optional(str), password: str, displayName: optional(str) }), examples.users.registerRequest, 'registerRequest')
    assertShape(object<LoginRequest>({ username: str, password: str }), examples.users.loginRequest, 'loginRequest')
    // present in the type vocabulary; exercised so the interfaces cannot rot unnoticed
    assertShape(object<ProfilePatch>({ displayName: optional(str), bio: optional(str), email: optional(nullable(str)) }), { bio: 'x', email: null }, 'profilePatch')
    assertShape(object<PasswordChange>({ current: str, next: str }), { current: 'a', next: 'b' }, 'passwordChange')
    assertShape(object<DeleteAccountRequest>({ password: str }), { password: 'a' }, 'deleteAccount')
  })
  it('repository views: public, fork of a public one, anonymous', () => {
    assertShape(repositoryView, examples.repository, 'repository')
    assertShape(repositoryView, examples.privateRepositoryForkOfPublic, 'privateRepositoryForkOfPublic')
    assertShape(repositoryView, examples.anonymousView, 'anonymousView')
    assert.equal(examples.repository.fullName, `${examples.repository.owner.username}/${examples.repository.name}`)
    assert.equal(examples.anonymousView.role, 'read')
    assert.ok(isRepositoryId(examples.repository.id))
  })
  it('listings: Page<T> and ItemList<T>', () => {
    const repositoryPage: RepositoryPage = examples.repositoryPage
    assertShape(page<RepositoryView>(repositoryView), repositoryPage, 'repositoryPage')
    assertShape(object<ItemList<RepositoryView>>({ items: arrayOf(repositoryView) }), examples.itemList, 'itemList')
    assertShape(object<ItemList<CollaboratorView>>({
      items: arrayOf(object<CollaboratorView>({ user: publicUser, role: oneOf(COLLABORATOR_ROLES) })),
    }), examples.collaborators, 'collaborators')
    assertShape(object<CollaboratorRequest>({ role: oneOf(COLLABORATOR_ROLES) }), examples.collaboratorRequest, 'collaboratorRequest')
    assertShape(object<UpdateRepository>({ name: optional(str), description: optional(str), visibility: optional(visibility) }), examples.updateRepository, 'updateRepository')
    assertShape(object<CreateRepository>({ id: optional(repoId), name: str, visibility, description: optional(str) }), examples.createRepository, 'createRepository')
  })
  it('revisions, trees, files, comparisons', () => {
    assertShape(revisionView, examples.revision, 'revision')
    assertShape(object<RevisionList>({ revisions: arrayOf(revisionView), next: nullable(str) }), examples.revisionList, 'revisionList')
    const treeEntry = object<TreeEntry>({ name: str, path: str, type: oneOf(['file', 'dir']), size: optional(int), sha256: optional(sha) })
    const tree = object<TreeListing>({ revision: nullable(revId), path: str, entries: arrayOf(treeEntry) })
    assertShape(tree, examples.tree.root, 'tree.root')
    assertShape(tree, examples.tree.empty, 'tree.empty')
    assertShape(object<FileContent>({ revision: revId, path: str, sha256: sha, size: int, content: str }), examples.file, 'file')
    const line = object<DiffLine>({ kind: oneOf(['context', 'add', 'remove']), text: str, oldLine: nullable(int), newLine: nullable(int) })
    const hunk = object<DiffHunk>({ oldStart: int, oldLines: int, newStart: int, newLines: int, lines: arrayOf(line) })
    const fileDiff = object<FileDiff>({ path: str, status: oneOf(['added', 'modified', 'deleted']), additions: int, deletions: int, hunks: arrayOf(hunk), coarse: bool })
    const comparison: Comparison = examples.comparison
    assertShape(object<Comparison>({ from: nullable(revId), to: revId, files: arrayOf(fileDiff), additions: int, deletions: int, truncated: bool }), comparison, 'comparison')
    assert.equal(comparison.additions, comparison.files.reduce((sum, file) => sum + file.additions, 0))
    assert.equal(comparison.deletions, comparison.files.reduce((sum, file) => sum + file.deletions, 0))
  })

  it('the revision, tree and file examples agree with each other and with the hashing rules', () => {
    const { revision, file } = examples
    assert.equal(sha256(canonicalManifest(revision.manifest)), revision.treeHash)
    assert.equal(sha256(file.content), file.sha256)
    assert.equal(Buffer.byteLength(file.content), file.size)
    const entry = revision.manifest.find((e: ManifestEntry) => e.path === file.path)
    assert.deepEqual(entry, { path: file.path, sha256: file.sha256, size: file.size })
    assert.equal(`rev_${sha256(canonicalRevision({ ...revision, mergedFrom: revision.mergedFrom }))}`, revision.id)
    assert.equal(examples.repository.head, revision.id)
    assert.equal(examples.repository.fileCount, revision.manifest.length)
    assert.equal(examples.repository.sizeBytes, revision.manifest.reduce((sum: number, e: ManifestEntry) => sum + e.size, 0))
    const [first, second] = examples.revisionList.revisions
    assert.equal(first.parent, second.id)
    assert.equal(`rev_${sha256(canonicalRevision(second))}`, second.id)
  })

  it('push: request shapes, storable paths, and responses that name the new revision', () => {
    const change = (value: unknown, at: string) => (value as { delete?: boolean }).delete
      ? object<{ path: string; delete: true }>({ path: str, delete: lit(true) })(value, at)
      : object<{ path: string; content: string }>({ path: str, content: str })(value, at)
    const pushRequest = object<PushRequest>({ base: nullable(revId), message: str, changes: arrayOf(change), mergedFrom: optional(revId) })
    for (const key of ['request', 'requestWithDelete', 'mergeRequest']) {
      assertShape(pushRequest, examples.push[key], `push.${key}`)
      for (const c of examples.push[key].changes) {
        assert.equal(isSyncPath(c.path), true, c.path)
        if (!c.delete) assert.equal(validateSyncContent(new TextEncoder().encode(c.content)), null)
      }
    }
    assert.equal(examples.push.request.base, null)
    assert.equal(examples.push.mergeRequest.mergedFrom !== undefined, true)
    const pushResponse = object<PushResponse>({
      id: revId, parent: nullable(revId), treeHash: sha, author: str, message: str, createdAt: iso, mergedFrom: optional(revId), manifest: arrayOf(manifestEntry),
    })
    assertShape(pushResponse, examples.push.response, 'push.response')
    assertShape(pushResponse, examples.push.responseNoChange, 'push.responseNoChange')
    assert.equal(sha256(canonicalManifest(examples.push.response.manifest)), examples.push.response.treeHash)
    assert.equal(examples.push.response.parent, examples.push.request.base)
  })

  it('errors: every code is a Library code and the details contract holds', () => {
    for (const [name, envelope] of Object.entries<any>(examples.errors)) {
      assertShape(apiError(), envelope, `errors.${name}`)
      assert.ok(typeof envelope.requestId === 'string', `${name} carries requestId`)
    }
    assert.equal(examples.errors.staleBase.error.code, 'stale_base')
    assert.ok(isRevisionId(examples.errors.staleBase.error.details.head))
    assert.equal(LIBRARY_ERROR_STATUS[examples.errors.staleBase.error.code as 'stale_base'], 409)
    assert.equal(LIBRARY_ERROR_STATUS[examples.errors.invalidPath.error.code as 'invalid_path'], 422)
    assert.equal(examples.errors.invalidPath.error.details.path.length > 0, true)
    assert.equal(examples.errors.rateLimited.error.details.retryAfter, 60)
  })

  it('tokens: view, request, and the one-time secret that is never a real one', () => {
    assertShape(tokenView, examples.tokens.view, 'tokens.view')
    assertShape(tokenView, examples.tokens.unrestricted, 'tokens.unrestricted')
    assertShape(object<CreateTokenRequest>({ name: str, scopes, repositories: optional(nullable(arrayOf(repoId))), expiresInDays: optional(nullable(int)) }), examples.tokens.createRequest, 'tokens.createRequest')
    const created: CreatedToken = examples.tokens.created
    assertShape(object<CreatedToken>({
      token: pattern(/^atn_/), id: str, name: str, prefix: str, scopes, repositories: nullable(arrayOf(repoId)),
      createdAt: iso, lastUsedAt: nullable(iso), expiresAt: nullable(iso),
    }), created, 'tokens.created')
    assert.equal(created.token.slice(0, 8), created.prefix.slice(0, 8))
    assert.match(created.token, /not_a_real_secret/)
  })

  it('device flow: start, lookup, approve/deny, poll errors, grant', () => {
    const { device } = examples
    assertShape(object<DeviceAuthorizationRequest>({ client: optional(str) }), device.startRequest, 'device.startRequest')
    const start: DeviceCode = device.start
    assertShape(object<DeviceCode>({ deviceCode: str, userCode: pattern(/^[A-Z0-9]{4}-[A-Z0-9]{4}$/), verificationUri: str, verificationUriComplete: optional(str), interval: optional(int), expiresIn: optional(int) }), start, 'device.start')
    assert.ok(start.deviceCode.length >= 10)
    assert.ok(start.verificationUriComplete!.includes(start.userCode))
    assertShape(object<DeviceCodeLookup>({ userCode: str, client: str, expiresAt: iso }), device.lookup, 'device.lookup')
    assertShape(object<DeviceApproveRequest>({ userCode: str, scopes: optional(scopes), repositories: optional(nullable(arrayOf(repoId))) }), device.approveRequest, 'device.approveRequest')
    assertShape(object<DeviceDenyRequest>({ userCode: str }), device.denyRequest, 'device.denyRequest')
    assertShape(object<DeviceTokenRequest>({ deviceCode: str }), device.tokenRequest, 'device.tokenRequest')
    assertShape(object<DeviceTokenGrant>({ accessToken: pattern(/^atn_/), username: str, tokenType: lit('bearer'), scopes }), device.grant, 'device.grant')
    assert.deepEqual(Object.keys(device.pollErrors).sort(), [...DEVICE_POLL_ERROR_CODES].sort())
    for (const [code, envelope] of Object.entries<any>(device.pollErrors)) {
      assertShape(apiError(code), envelope, `device.pollErrors.${code}`)
      assert.equal(LIBRARY_ERROR_STATUS[code as keyof typeof LIBRARY_ERROR_STATUS], 400)
    }
  })

  it('admin: metadata only, takedown only', () => {
    const { admin } = examples
    assertShape(object<AdminOperator>({ operator: lit(true), username: str }), admin.me, 'admin.me')
    assertShape(object<AdminStats>({ users: int, repositories: int, publicRepositories: int, privateRepositories: int, revisions: int, blobs: int, storedBytes: int }), admin.stats, 'admin.stats')
    assert.equal(admin.stats.repositories, admin.stats.publicRepositories + admin.stats.privateRepositories)
    assertShape(page<AdminUser>(object<AdminUser>({ id: str, username: str, email: nullable(str), displayName: str, createdAt: iso, repositoryCount: int })), admin.users, 'admin.users')
    assertShape(page<AdminRepository>(adminRepository), admin.repositories, 'admin.repositories')
    assertShape(object<AdminRepositoryPatch>({ visibility: lit('private') }), admin.takedownRequest, 'admin.takedownRequest')
    assertShape(adminRepository, admin.takedownResponse, 'admin.takedownResponse')
    assert.equal(admin.takedownResponse.visibility, 'private')
    assertShape(apiError('forbidden'), admin.publishRefused, 'admin.publishRefused')
    assertShape(apiError('unauthorized'), admin.anonymous, 'admin.anonymous')
    assertShape(apiError('forbidden'), admin.notOperator, 'admin.notOperator')
    // metadata only: no operator shape may carry file content or a token
    const serialised = JSON.stringify([admin.users, admin.repositories, admin.takedownResponse])
    assert.doesNotMatch(serialised, /"content"|"token"|"password"/)
  })

  it('never contains anything that looks like a real secret', () => {
    const text = JSON.stringify(examples)
    for (const match of text.matchAll(/atn_[A-Za-z0-9_-]+/g)) assert.match(match[0], /^atn_(exam|example_not_a_real_secret)/, match[0])
  })
})

describe('route builders (fixtures/library/v1/routes.json)', () => {
  const routes = fixture<{ prefix: string; sync: any[]; library: any[]; admin: any[] }>('library/v1/routes.json')
  const groups = { sync: syncRoutes, library: libraryRoutes, admin: adminRoutes } as unknown as Record<string, Record<string, unknown>>

  for (const group of ['sync', 'library', 'admin'] as const) {
    for (const c of routes[group]) {
      it(`${group}.${c.route}(${JSON.stringify(c.args).slice(1, -1)}) -> ${c.path}`, () => {
        const route = groups[group]![c.route]
        const actual = typeof route === 'function' ? (route as (...a: unknown[]) => string)(...c.args) : route
        assert.equal(actual, c.path)
        assert.ok(String(actual).startsWith(routes.prefix))
      })
    }
  }

  it('no route builder carries a credential or a host', () => {
    for (const c of [...routes.sync, ...routes.library, ...routes.admin]) assert.doesNotMatch(c.path, /^[a-z]+:\/\//i)
  })
  it('withQuery drops undefined, null and empty values and encodes the rest', () => {
    assert.equal(withQuery('/x'), '/x')
    assert.equal(withQuery('/x', { a: undefined, b: null, c: '' }), '/x')
    assert.equal(withQuery('/x', { a: 'b c', n: 0, t: false }), '/x?a=b+c&n=0&t=false')
  })
})
