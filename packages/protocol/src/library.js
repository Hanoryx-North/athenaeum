import { API_PREFIX, SYNC_ERROR_CODES } from './sync.js';
/** What `GET /api/v1` reports as `service`. The retired name `athenaeum-library` is not accepted. */
export const API_SERVICE_NAME = 'athenaeum-api';
// --- vocabularies ------------------------------------------------------------------------------
export const TOKEN_SCOPES = ['repo:read', 'repo:write', 'repo:admin'];
/** A token's secret starts with this; the first eight characters are shown back as `prefix`. */
export const TOKEN_SECRET_PREFIX = 'atn_';
export const REPOSITORY_VISIBILITIES = ['private', 'public'];
/** What the caller may do to a repository. A private repository the caller cannot read is a 404, never `none`. */
export const REPOSITORY_ROLES = ['none', 'read', 'write', 'admin', 'owner'];
/** Roles an owner or admin can grant; `owner` is not grantable. */
export const COLLABORATOR_ROLES = ['read', 'write', 'admin'];
export const REPOSITORY_SORTS = ['updated', 'created', 'name', 'forks'];
export const FILE_CHANGE_STATUSES = ['added', 'modified', 'deleted'];
/** `POST /auth/device/token` answers 400 with one of these until the person decides. */
export const DEVICE_POLL_ERROR_CODES = ['authorization_pending', 'slow_down', 'expired_token', 'access_denied'];
/** Every `error.code` a Library API response can carry. Superset of `SYNC_ERROR_CODES`. */
export const LIBRARY_ERROR_CODES = [...SYNC_ERROR_CODES, ...DEVICE_POLL_ERROR_CODES, 'internal_error'];
/** HTTP status for each error code. Codes are the contract; messages are for people and may change. */
export const LIBRARY_ERROR_STATUS = {
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
};
export const REVISION_ID_PATTERN = /^rev_[0-9a-f]{64}$/;
export function isRevisionId(value) {
    return REVISION_ID_PATTERN.test(value);
}
/**
 * The bytes whose SHA-256 is the revision id (`rev_` + 64 hex digits). Keys are alphabetical,
 * `mergedFrom` is present only when set, `parent` is `null` for a first revision. The server
 * assigns revision ids; a client never invents one.
 */
export function canonicalRevision(fields) {
    return JSON.stringify({
        author: fields.author,
        createdAt: fields.createdAt,
        ...(fields.mergedFrom ? { mergedFrom: fields.mergedFrom } : {}),
        message: fields.message,
        parent: fields.parent,
        treeHash: fields.treeHash,
    });
}
/** Appends the defined, non-empty values as a query string; returns `path` unchanged when none remain. */
export function withQuery(path, values = {}) {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(values)) {
        if (value !== undefined && value !== null && value !== '')
            params.set(key, String(value));
    }
    const text = params.toString();
    return text ? `${path}?${text}` : path;
}
const repo = (id) => `${API_PREFIX}/repositories/${encodeURIComponent(id)}`;
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
    userToken: (id) => `${API_PREFIX}/user/tokens/${encodeURIComponent(id)}`,
    deviceLookup: (code) => withQuery(`${API_PREFIX}/auth/device/lookup`, { code }),
    deviceApprove: `${API_PREFIX}/auth/device/approve`,
    deviceDeny: `${API_PREFIX}/auth/device/deny`,
    profile: (username) => `${API_PREFIX}/users/${encodeURIComponent(username)}`,
    profileRepositories: (username, query = {}) => withQuery(`${API_PREFIX}/users/${encodeURIComponent(username)}/repositories`, { ...query }),
    repositories: (query = {}) => withQuery(`${API_PREFIX}/repositories`, { ...query }),
    forks: (id) => `${repo(id)}/forks`,
    collaborators: (id) => `${repo(id)}/collaborators`,
    collaborator: (id, username) => `${repo(id)}/collaborators/${encodeURIComponent(username)}`,
    revisionList: (id, query = {}) => withQuery(`${repo(id)}/revisions`, { ...query }),
    tree: (id, query = {}) => withQuery(`${repo(id)}/tree`, { ...query }),
    file: (id, path, rev) => withQuery(`${repo(id)}/file`, { rev, path }),
    raw: (id, path, rev) => withQuery(`${repo(id)}/raw`, { rev, path }),
    compare: (id, query = {}) => withQuery(`${repo(id)}/compare`, { ...query }),
};
/** The operator console's routes. They need a browser session of a listed operator. */
export const adminRoutes = {
    me: `${API_PREFIX}/admin/me`,
    stats: `${API_PREFIX}/admin/stats`,
    users: (query = {}) => withQuery(`${API_PREFIX}/admin/users`, { ...query }),
    repositories: (query = {}) => withQuery(`${API_PREFIX}/admin/repositories`, { ...query }),
    repository: (id) => `${API_PREFIX}/admin/repositories/${encodeURIComponent(id)}`,
};
