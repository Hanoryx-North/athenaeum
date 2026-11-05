/**
 * The Library API v1: the JSON the hosted text backend (`athenaeum_api`) speaks to its browser
 * frontends (`athenaeum_library`, `athenaeum_admin`) and, for the sync routes, to the `athenaeum`
 * CLI. The sync half lives in `sync.ts`; this module is everything else: accounts, tokens, the
 * repository browsing views, the device flow and the operator surface.
 *
 * Additive only. Nothing here changes the meaning of an export in `sync.ts` or `bridge.ts`, and the
 * API stays at `API_VERSION` 1 until a field is removed or changes meaning.
 */
import type { DeviceToken, ManifestEntry, SyncLimits } from './sync.js'
import { API_PREFIX, SYNC_ERROR_CODES } from './sync.js'

/** What `GET /api/v1` reports as `service`. The retired name `athenaeum-library` is not accepted. */
export const API_SERVICE_NAME = 'athenaeum-api' as const

// --- vocabularies ------------------------------------------------------------------------------

export const TOKEN_SCOPES = ['repo:read', 'repo:write', 'repo:admin'] as const
export type TokenScope = typeof TOKEN_SCOPES[number]

/** A token's secret starts with this; the first eight characters are shown back as `prefix`. */
export const TOKEN_SECRET_PREFIX = 'atn_' as const

export const REPOSITORY_VISIBILITIES = ['private', 'public'] as const
export type RepositoryVisibility = typeof REPOSITORY_VISIBILITIES[number]

/** What the caller may do to a repository. A private repository the caller cannot read is a 404, never `none`. */
export const REPOSITORY_ROLES = ['none', 'read', 'write', 'admin', 'owner'] as const
export type RepositoryRole = typeof REPOSITORY_ROLES[number]

/** Roles an owner or admin can grant; `owner` is not grantable. */
export const COLLABORATOR_ROLES = ['read', 'write', 'admin'] as const
export type CollaboratorRole = typeof COLLABORATOR_ROLES[number]

export const REPOSITORY_SORTS = ['updated', 'created', 'name', 'forks'] as const
export type RepositorySort = typeof REPOSITORY_SORTS[number]

export const FILE_CHANGE_STATUSES = ['added', 'modified', 'deleted'] as const
export type FileChangeStatus = typeof FILE_CHANGE_STATUSES[number]

/** `POST /auth/device/token` answers 400 with one of these until the person decides. */
export const DEVICE_POLL_ERROR_CODES = ['authorization_pending', 'slow_down', 'expired_token', 'access_denied'] as const
export type DevicePollErrorCode = typeof DEVICE_POLL_ERROR_CODES[number]

/** Every `error.code` a Library API response can carry. Superset of `SYNC_ERROR_CODES`. */
export const LIBRARY_ERROR_CODES = [...SYNC_ERROR_CODES, ...DEVICE_POLL_ERROR_CODES, 'internal_error'] as const
export type LibraryErrorCode = typeof LIBRARY_ERROR_CODES[number]

/** HTTP status for each error code. Codes are the contract; messages are for people and may change. */
export const LIBRARY_ERROR_STATUS: Readonly<Record<LibraryErrorCode, number>> = {
  invalid_request: 400,
  not_found: 404,
  request_too_large: 413,
  unsupported_protocol: 400,
  unauthorized: 401,
  forbidden: 403,
  stale_base: 409,
  conflict: 409,
  invalid_path: 422,
  invalid_content: 422,
  limit_exceeded: 422,
  rate_limited: 429,
  authorization_pending: 400,
  slow_down: 400,
  expired_token: 400,
  access_denied: 400,
  internal_error: 500,
}

export const REVISION_ID_PATTERN = /^rev_[0-9a-f]{64}$/
export function isRevisionId(value: string): boolean {
  return REVISION_ID_PATTERN.test(value)
}

// --- service metadata --------------------------------------------------------------------------

export interface ApiLimits extends SyncLimits { maxPushBytes: number; maxChangesPerPush: number }
/** `GET /api/v1`. */
export interface ApiInfo {
  service: typeof API_SERVICE_NAME
  apiVersion: number
  syncProtocol: number
  limits: ApiLimits
  registration: 'open' | 'closed'
}
/** `GET /api/v1/stats`: public totals for a front page. */
export interface PublicStats { users: number; publicRepositories: number; revisions: number }

// --- users -------------------------------------------------------------------------------------

export interface PublicUser { username: string; displayName: string; bio: string; createdAt: string }
export interface SelfUser extends PublicUser { id: string; email: string | null }
/** `GET /user`: who the credential belongs to, and how it authenticated. */
export interface CurrentUser extends SelfUser { via: 'session' | 'token' }
/** `GET /users/:username`. */
export interface UserProfile extends PublicUser { publicRepositories: number }
/** `POST /auth/register` (201) and `POST /auth/login`. A browser session cookie is set alongside. */
export interface AuthResponse { user: SelfUser }
export interface RegisterRequest { username: string; email?: string; password: string; displayName?: string }
/** `username` may also be the account's email address. */
export interface LoginRequest { username: string; password: string }
export interface ProfilePatch { displayName?: string; bio?: string; email?: string | null }
export interface PasswordChange { current: string; next: string }
export interface DeleteAccountRequest { password: string }
export interface OkResponse { ok: true }

// --- repositories ------------------------------------------------------------------------------

export interface ForkOrigin { id: string; fullName: string }
/** The repository as every browsing view sees it. Private repositories the caller cannot read are 404, not hidden fields. */
export interface RepositoryView {
  id: string
  owner: PublicUser
  name: string
  fullName: string
  description: string
  visibility: RepositoryVisibility
  head: string | null
  forkedFrom: ForkOrigin | null
  forks: number
  fileCount: number
  sizeBytes: number
  createdAt: string
  updatedAt: string
  role: RepositoryRole
}
/** The pagination envelope used by every listing that can be long. */
export interface Page<T> { items: T[]; total: number; page: number; perPage: number }
/** A listing that is short by construction (`/user/repositories`, forks, collaborators). */
export interface ItemList<T> { items: T[] }
export type RepositoryPage = Page<RepositoryView>

export interface RepositoryListQuery {
  q?: string
  sort?: RepositorySort
  page?: number
  perPage?: number
  owner?: string
  /** Include private repositories the caller can see. */
  mine?: boolean
}
export interface UpdateRepository { name?: string; description?: string; visibility?: RepositoryVisibility }
export interface ForkRequest { name?: string; visibility?: RepositoryVisibility }
export interface CollaboratorView { user: PublicUser; role: CollaboratorRole }
export interface CollaboratorRequest { role: CollaboratorRole }

// --- reading a repository ----------------------------------------------------------------------

/** A revision as the browsing routes return it. The list route omits `manifest`; the single route includes it. */
export interface RevisionView {
  id: string
  parent: string | null
  treeHash: string
  author: string
  message: string
  createdAt: string
  mergedFrom?: string
  manifest?: ManifestEntry[]
}
/** `GET /repositories/:id/revisions`; `next` is the `before` value of the next page, or null. */
export interface RevisionList { revisions: RevisionView[]; next: string | null }
/** `POST /repositories/:id/push`: 201 with a new revision, 200 with the head when nothing changed. */
export interface PushResponse extends RevisionView { manifest: ManifestEntry[] }

export interface TreeEntry { name: string; path: string; type: 'file' | 'dir'; size?: number; sha256?: string }
/** `GET /repositories/:id/tree?rev=&path=`; `revision` is null for a repository with no revisions yet. */
export interface TreeListing { revision: string | null; path: string; entries: TreeEntry[] }
/** `GET /repositories/:id/file?rev=&path=`. `/raw` returns the same content as `text/plain`. */
export interface FileContent { revision: string; path: string; sha256: string; size: number; content: string }

export interface DiffLine { kind: 'context' | 'add' | 'remove'; text: string; oldLine: number | null; newLine: number | null }
export interface DiffHunk { oldStart: number; oldLines: number; newStart: number; newLines: number; lines: DiffLine[] }
export interface FileDiff {
  path: string
  status: FileChangeStatus
  additions: number
  deletions: number
  hunks: DiffHunk[]
  /** The file was too large to diff line by line; the middle is shown as replaced. */
  coarse: boolean
}
/** `GET /repositories/:id/compare?from=&to=`; without `from`, a revision is compared with its parent. */
export interface Comparison {
  from: string | null
  to: string
  files: FileDiff[]
  additions: number
  deletions: number
  /** More files changed than were diffed. */
  truncated: boolean
}

export interface RevisionFields {
  parent: string | null
  treeHash: string
  author: string
  message: string
  createdAt: string
  mergedFrom?: string | null
}
/**
 * The bytes whose SHA-256 is the revision id (`rev_` + 64 hex digits). Keys are alphabetical,
 * `mergedFrom` is present only when set, `parent` is `null` for a first revision. The server
 * assigns revision ids; a client never invents one.
 */
export function canonicalRevision(fields: RevisionFields): string {
  return JSON.stringify({
    author: fields.author,
    createdAt: fields.createdAt,
    ...(fields.mergedFrom ? { mergedFrom: fields.mergedFrom } : {}),
    message: fields.message,
    parent: fields.parent,
    treeHash: fields.treeHash,
  })
}

// --- access tokens -----------------------------------------------------------------------------

export interface TokenView {
  id: string
  name: string
  /** The first eight characters of the secret, enough to recognise it in a list. */
  prefix: string
  scopes: TokenScope[]
  /** Repository ids the token is limited to, or null for no limit. */
  repositories: string[] | null
  createdAt: string
  lastUsedAt: string | null
  expiresAt: string | null
}
export interface CreateTokenRequest {
  name: string
  scopes: TokenScope[]
  repositories?: string[] | null
  expiresInDays?: number | null
}
/** `POST /user/tokens` (201). `token` is the secret and is shown exactly once. */
export interface CreatedToken extends TokenView { token: string }

// --- device authorisation ----------------------------------------------------------------------

/** `POST /auth/device`. The response is `DeviceCode` from `sync.ts`. */
export interface DeviceAuthorizationRequest { client?: string }
/** `GET /auth/device/lookup?code=` (signed-in browser session only). */
export interface DeviceCodeLookup { userCode: string; client: string; expiresAt: string }
export interface DeviceApproveRequest { userCode: string; scopes?: TokenScope[]; repositories?: string[] | null }
export interface DeviceDenyRequest { userCode: string }
export interface DeviceTokenRequest { deviceCode: string }
/** `POST /auth/device/token` once approved; the grant is handed over exactly once. */
export interface DeviceTokenGrant extends DeviceToken { tokenType: 'bearer'; username: string; scopes: TokenScope[] }

// --- operator surface --------------------------------------------------------------------------

/** `GET /admin/me`: 401 anonymous, 403 for a signed-in person who is not an operator. */
export interface AdminOperator { operator: true; username: string }
export interface AdminStats {
  users: number
  repositories: number
  publicRepositories: number
  privateRepositories: number
  revisions: number
  blobs: number
  storedBytes: number
}
export interface AdminUser {
  id: string
  username: string
  email: string | null
  displayName: string
  createdAt: string
  repositoryCount: number
}
/** Metadata only. An operator route never returns file contents. */
export interface AdminRepository {
  id: string
  owner: string
  name: string
  fullName: string
  visibility: RepositoryVisibility
  head: string | null
  fileCount: number
  sizeBytes: number
  createdAt: string
  updatedAt: string
}
export interface AdminListQuery { page?: number; perPage?: number; q?: string }
export interface AdminRepositoryQuery extends AdminListQuery { visibility?: RepositoryVisibility | '' }
/**
 * Operators may only take a repository down. `visibility: 'public'` from an operator route is a
 * 403 `forbidden`: publishing someone's private memory is the owner's decision alone.
 */
export interface AdminRepositoryPatch { visibility: 'private' }

// --- routes ------------------------------------------------------------------------------------

type QueryValue = string | number | boolean | undefined | null

/** Appends the defined, non-empty values as a query string; returns `path` unchanged when none remain. */
export function withQuery(path: string, values: { [key: string]: QueryValue } = {}): string {
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== '') params.set(key, String(value))
  }
  const text = params.toString()
  return text ? `${path}?${text}` : path
}

const repo = (id: string) => `${API_PREFIX}/repositories/${encodeURIComponent(id)}`

/** Relative routes for everything outside the sync set (`syncRoutes` in `sync.ts`). */
export const libraryRoutes = {
  info: API_PREFIX,
  stats: `${API_PREFIX}/stats`,
  register: `${API_PREFIX}/auth/register`,
  login: `${API_PREFIX}/auth/login`,
  logout: `${API_PREFIX}/auth/logout`,
  user: `${API_PREFIX}/user`,
  userPassword: `${API_PREFIX}/user/password`,
  userDelete: `${API_PREFIX}/user/delete`,
  userRepositories: `${API_PREFIX}/user/repositories`,
  userTokens: `${API_PREFIX}/user/tokens`,
  userToken: (id: string) => `${API_PREFIX}/user/tokens/${encodeURIComponent(id)}`,
  deviceLookup: (code: string) => withQuery(`${API_PREFIX}/auth/device/lookup`, { code }),
  deviceApprove: `${API_PREFIX}/auth/device/approve`,
  deviceDeny: `${API_PREFIX}/auth/device/deny`,
  profile: (username: string) => `${API_PREFIX}/users/${encodeURIComponent(username)}`,
  profileRepositories: (username: string, query: RepositoryListQuery = {}) =>
    withQuery(`${API_PREFIX}/users/${encodeURIComponent(username)}/repositories`, { ...query }),
  repositories: (query: RepositoryListQuery = {}) => withQuery(`${API_PREFIX}/repositories`, { ...query }),
  forks: (id: string) => `${repo(id)}/forks`,
  collaborators: (id: string) => `${repo(id)}/collaborators`,
  collaborator: (id: string, username: string) => `${repo(id)}/collaborators/${encodeURIComponent(username)}`,
  revisionList: (id: string, query: { limit?: number; before?: string } = {}) => withQuery(`${repo(id)}/revisions`, { ...query }),
  tree: (id: string, query: { rev?: string; path?: string } = {}) => withQuery(`${repo(id)}/tree`, { ...query }),
  file: (id: string, path: string, rev?: string) => withQuery(`${repo(id)}/file`, { rev, path }),
  raw: (id: string, path: string, rev?: string) => withQuery(`${repo(id)}/raw`, { rev, path }),
  compare: (id: string, query: { from?: string; to?: string } = {}) => withQuery(`${repo(id)}/compare`, { ...query }),
} as const

/** The operator console's routes. They need a browser session of a listed operator. */
export const adminRoutes = {
  me: `${API_PREFIX}/admin/me`,
  stats: `${API_PREFIX}/admin/stats`,
  users: (query: AdminListQuery = {}) => withQuery(`${API_PREFIX}/admin/users`, { ...query }),
  repositories: (query: AdminRepositoryQuery = {}) => withQuery(`${API_PREFIX}/admin/repositories`, { ...query }),
  repository: (id: string) => `${API_PREFIX}/admin/repositories/${encodeURIComponent(id)}`,
} as const
