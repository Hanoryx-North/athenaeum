export const API_VERSION = 1;
export const SYNC_PROTOCOL_VERSION = 1;
export const PORTABLE_FORMAT_VERSION = 1;
export const API_PREFIX = '/api/v1';
export const MANIFEST_FILE = 'athenaeum.json';
export const SYNC_EXTENSIONS = ['md'];
export const DEFAULT_SYNC_LIMITS = { maxFileBytes: 1024 * 1024, maxRepositoryBytes: 50 * 1024 * 1024 };
/** What a repository id looks like, unanchored so it can feed an HTML `pattern` attribute. */
export const REPOSITORY_ID_SOURCE = 'repo_[A-Za-z0-9_-]{1,60}';
/**
 * The id in `athenaeum.json` and in the hosted API are one value. A device may request its local id
 * when it creates the hosted repository, or adopt the hosted id for a vault that has none yet.
 */
export const REPOSITORY_ID_PATTERN = new RegExp(`^${REPOSITORY_ID_SOURCE}$`);
export const SYNC_ERROR_CODES = [
    'invalid_request', 'not_found', 'request_too_large', 'unsupported_protocol',
    'unauthorized', 'forbidden', 'stale_base', 'conflict', 'invalid_path',
    'invalid_content', 'limit_exceeded', 'rate_limited',
];
/** Relative routes; callers supply their own same-origin or remote base URL. */
export const syncRoutes = {
    info: API_PREFIX,
    device: `${API_PREFIX}/auth/device`,
    deviceToken: `${API_PREFIX}/auth/device/token`,
    repositories: `${API_PREFIX}/repositories`,
    repository: (id) => `${API_PREFIX}/repositories/${encodeURIComponent(id)}`,
    namedRepository: (username, name) => `${API_PREFIX}/users/${encodeURIComponent(username)}/repositories/${encodeURIComponent(name)}`,
    revisions: (id) => `${syncRoutes.repository(id)}/revisions`,
    revision: (id, revision) => `${syncRoutes.revisions(id)}/${encodeURIComponent(revision)}`,
    blob: (id, sha256) => `${syncRoutes.repository(id)}/blobs/${encodeURIComponent(sha256)}`,
    push: (id) => `${syncRoutes.repository(id)}/push`,
    compare: (id, from, to) => `${syncRoutes.repository(id)}/compare?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
};
export function isRepositoryId(value) {
    return REPOSITORY_ID_PATTERN.test(value);
}
/** Safe relative path syntax. Repository sync policy is checked separately by `isSyncPath`. */
export function validateSyncPath(path) {
    for (let index = 0; index < path.length; index++) {
        const unit = path.charCodeAt(index);
        if (unit >= 0xd800 && unit <= 0xdbff) {
            const next = path.charCodeAt(++index);
            if (!(next >= 0xdc00 && next <= 0xdfff))
                return 'invalid_path';
        }
        else if (unit >= 0xdc00 && unit <= 0xdfff)
            return 'invalid_path';
    }
    const bytes = new TextEncoder().encode(path);
    if (!path || bytes.length > 1024)
        return 'invalid_path';
    if (path.startsWith('/') || path.includes('\\') || /^[A-Za-z]:/.test(path) || /[\u0000-\u001f\u007f-\u009f]/u.test(path))
        return 'invalid_path';
    for (const segment of path.split('/')) {
        if (!segment || segment === '.' || segment === '..' || segment.startsWith('.') || new TextEncoder().encode(segment).length > 255)
            return 'invalid_path';
    }
    return null;
}
export function isSyncPath(path) {
    if (validateSyncPath(path))
        return false;
    return path === MANIFEST_FILE || (path.startsWith('wiki/') && path.endsWith('.md'));
}
export function validateSyncContent(bytes, limits = DEFAULT_SYNC_LIMITS) {
    if (bytes.byteLength > limits.maxFileBytes || bytes.includes(0))
        return 'invalid_content';
    try {
        new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    }
    catch {
        return 'invalid_content';
    }
    return null;
}
/** Canonical manifest serialization is compact JSON with UTF-8 path sort and key order. */
export function canonicalManifest(entries) {
    const encoder = new TextEncoder();
    const sorted = [...entries].sort((a, b) => {
        const left = encoder.encode(a.path);
        const right = encoder.encode(b.path);
        for (let index = 0; index < Math.min(left.length, right.length); index++) {
            if (left[index] !== right[index])
                return left[index] - right[index];
        }
        return left.length - right.length;
    });
    return JSON.stringify(sorted.map(({ path, sha256, size }) => ({ path, sha256, size })));
}
export function isCompatibleServer(info) {
    return info.apiVersion === API_VERSION && info.syncProtocol === SYNC_PROTOCOL_VERSION;
}
