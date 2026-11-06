import type { ProtocolError } from './bridge.js'

export const API_VERSION = 1 as const
export const SYNC_PROTOCOL_VERSION = 1 as const
export const PORTABLE_FORMAT_VERSION = 1 as const
export const API_PREFIX = '/api/v1' as const
export const MANIFEST_FILE = 'athenaeum.json' as const
export const SYNC_EXTENSIONS = ['md'] as const
export const DEFAULT_SYNC_LIMITS = { maxFileBytes: 1024 * 1024, maxRepositoryBytes: 50 * 1024 * 1024 } as const

/** What a repository id looks like, unanchored so it can feed an HTML `pattern` attribute. */
export const REPOSITORY_ID_SOURCE = 'repo_[A-Za-z0-9_-]{1,60}' as const
/**
 * The id in `athenaeum.json` and in the hosted API are one value. A device may request its local id
 * when it creates the hosted repository, or adopt the hosted id for a vault that has none yet.
 */
export const REPOSITORY_ID_PATTERN = new RegExp(`^${REPOSITORY_ID_SOURCE}$`)

/** Unknown top-level keys must survive a manifest rewrite. */
export interface RepositoryManifest {
  formatVersion: number
  repositoryId: string
  name: string
  createdAt: string
  [key: string]: unknown
}
export interface ManifestEntry { path: string; sha256: string; size: number }
export interface SyncLimits { maxFileBytes: number; maxRepositoryBytes: number }
export interface ServerInfo { service: string; apiVersion: number; syncProtocol: number; limits?: SyncLimits }
export interface RepositoryOwner { username: string }
export interface Repository {
  id: string; name: string; owner?: RepositoryOwner; visibility?: string; head?: string | null
  [key: string]: unknown
}
export interface Revision {
  id: string; parent?: string | null; treeHash: string; author?: string; message?: string
  createdAt?: string; mergedFrom?: string; manifest: ManifestEntry[]
  [key: string]: unknown
}
export type Change = { path: string; content: string; delete?: false } | { path: string; delete: true; content?: never }
export interface PushRequest { base: string | null; message: string; changes: Change[]; mergedFrom?: string }
/**
 * `id` requests this exact identity. The server returns 409 `conflict` if it is already taken.
 * Omit it to let the server choose. It is never `null`.
 */
export interface CreateRepository { id?: string; name: string; visibility: 'private' | 'public'; description?: string }
export interface ApiErrorEnvelope { error: ProtocolError; requestId?: string }
export interface DeviceCode {
  deviceCode: string; userCode: string; verificationUri: string
  verificationUriComplete?: string; interval?: number; expiresIn?: number
}
export interface DeviceToken { accessToken: string; username?: string }

export const SYNC_ERROR_CODES = [
  'invalid_request', 'not_found', 'request_too_large', 'unsupported_protocol',
  'unauthorized', 'forbidden', 'stale_base', 'conflict', 'invalid_path',
  'invalid_content', 'limit_exceeded', 'rate_limited',
] as const

/** Relative routes; callers supply their own same-origin or remote base URL. */
export const syncRoutes = {
  info: API_PREFIX,
  device: `${API_PREFIX}/auth/device`,
  deviceToken: `${API_PREFIX}/auth/device/token`,
  repositories: `${API_PREFIX}/repositories`,
  repository: (id: string) => `${API_PREFIX}/repositories/${encodeURIComponent(id)}`,
  namedRepository: (username: string, name: string) => `${API_PREFIX}/users/${encodeURIComponent(username)}/repositories/${encodeURIComponent(name)}`,
  revisions: (id: string) => `${syncRoutes.repository(id)}/revisions`,
  revision: (id: string, revision: string) => `${syncRoutes.revisions(id)}/${encodeURIComponent(revision)}`,
  blob: (id: string, sha256: string) => `${syncRoutes.repository(id)}/blobs/${encodeURIComponent(sha256)}`,
  push: (id: string) => `${syncRoutes.repository(id)}/push`,
  compare: (id: string, from: string, to: string) => `${syncRoutes.repository(id)}/compare?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
} as const

export function isRepositoryId(value: string): boolean {
  return REPOSITORY_ID_PATTERN.test(value)
}

/** Safe relative path syntax. Repository sync policy is checked separately by `isSyncPath`. */
export function validateSyncPath(path: string): string | null {
  for (let index = 0; index < path.length; index++) {
    const unit = path.charCodeAt(index)
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = path.charCodeAt(++index)
      if (!(next >= 0xdc00 && next <= 0xdfff)) return 'invalid_path'
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return 'invalid_path'
  }
  const bytes = new TextEncoder().encode(path)
  if (!path || bytes.length > 1024) return 'invalid_path'
  if (path.startsWith('/') || path.includes('\\') || /^[A-Za-z]:/.test(path) || /[\u0000-\u001f\u007f-\u009f]/u.test(path)) return 'invalid_path'
  for (const segment of path.split('/')) {
    if (!segment || segment === '.' || segment === '..' || segment.startsWith('.') || new TextEncoder().encode(segment).length > 255) return 'invalid_path'
  }
  return null
}

export function isSyncPath(path: string): boolean {
  if (validateSyncPath(path)) return false
  return path === MANIFEST_FILE || ((path.startsWith('wiki/') || !path.includes('/')) && path.endsWith('.md'))
}

export function validateSyncContent(bytes: Uint8Array, limits: SyncLimits = DEFAULT_SYNC_LIMITS): string | null {
  if (bytes.byteLength > limits.maxFileBytes || bytes.includes(0)) return 'invalid_content'
  try { new TextDecoder('utf-8', { fatal: true }).decode(bytes) } catch { return 'invalid_content' }
  return null
}

/** Canonical manifest serialization is compact JSON with UTF-8 path sort and key order. */
export function canonicalManifest(entries: readonly ManifestEntry[]): string {
  const encoder = new TextEncoder()
  const sorted = [...entries].sort((a, b) => {
    const left = encoder.encode(a.path)
    const right = encoder.encode(b.path)
    for (let index = 0; index < Math.min(left.length, right.length); index++) {
      if (left[index] !== right[index]) return left[index]! - right[index]!
    }
    return left.length - right.length
  })
  return JSON.stringify(sorted.map(({ path, sha256, size }) => ({ path, sha256, size })))
}

export function isCompatibleServer(info: ServerInfo): boolean {
  return info.apiVersion === API_VERSION && info.syncProtocol === SYNC_PROTOCOL_VERSION
}
