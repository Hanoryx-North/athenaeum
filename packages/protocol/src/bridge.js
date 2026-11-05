/** Stable v1 JSON contract for the one-shot `athenaeum bridge` process. */
export const BRIDGE_PROTOCOL_VERSION = 1;
export const BRIDGE_METHODS = [
    'capabilities', 'search', 'open', 'navigate', 'related', 'evidence', 'history',
    'context', 'create', 'update', 'link', 'attach_evidence', 'record_use',
    'proposals', 'accept', 'reject', 'status', 'review', 'sources', 'index', 'path',
];
/** Why a review finding was raised. A closed wire vocabulary, not a knowledge vocabulary. */
export const BRIDGE_REVIEW_KINDS = [
    'contradiction', 'dispute', 'stale_anchor', 'unsupported_rule', 'unreviewed_proposal',
    'duplicate', 'near_duplicate', 'superseded', 'incomplete_scan', 'orphan',
];
/** How much a review finding should worry a reader before it worries a build. */
export const BRIDGE_REVIEW_SEVERITIES = ['blocking', 'warning', 'info'];
/** Which scan read a source: the vault pass or the repository pass. Never `all`. */
export const BRIDGE_SOURCE_SCOPES = ['vault', 'repository'];
/** What `index` rescans: the vault, the repository, or both. */
export const BRIDGE_INDEX_SCOPES = ['vault', 'repository', 'all'];
export const BRIDGE_ERROR_CODES = [
    'invalid_request', 'not_found', 'incompatible_store', 'knowledge_error',
    'request_too_large', 'unsupported_protocol', 'workspace_not_found',
];
export function bridgeRequest(request, requestId) {
    return { protocolVersion: BRIDGE_PROTOCOL_VERSION, ...(requestId ? { requestId } : {}), request };
}
export function existingNodesFromError(error) {
    const value = error.details?.existingNodes;
    return Array.isArray(value) ? value.filter((item) => typeof item === 'string') : [];
}
