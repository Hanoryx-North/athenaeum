/**
 * What state the knowledge surfaces are in, decided from the engine's own
 * words.
 *
 * The app has four honest failure states and one healthy one, and every panel
 * reads them from here rather than each inventing its own test:
 *
 * * `absent-vault` — there is no `.athenaeum/` or no vault here. The engine
 *   says so in a normal `status` answer (`initialized: false`), or refuses a
 *   deeper call with `workspace_not_found`.
 * * `stale-index` — the vault changed after the derived index last reconciled.
 * * `protocol-mismatch` — the engine speaks a version this app does not, or
 *   answered a shape it does not recognize.
 * * `unreachable` — the shell could not reach the engine at all.
 *
 * Nothing here recomputes knowledge; it only classifies what the engine said.
 */
import { ENGINE_PROTOCOL_VERSION, engineError, type EngineStatus } from "@/lib/engine-client"

export type EngineProductState =
  | { kind: "loading" }
  | { kind: "ready" }
  | { kind: "absent-vault"; message: string }
  | { kind: "never-indexed"; message: string }
  | { kind: "stale-index"; message: string; indexedAt: string | null }
  | { kind: "protocol-mismatch"; message: string }
  | { kind: "engine-error"; code: string; message: string }
  | { kind: "unreachable"; message: string }

/** A state that always carries a message: everything but healthy or pending. */
export type EngineProblemState = Exclude<
  EngineProductState,
  { kind: "ready" } | { kind: "loading" }
>

export function engineStateFromStatus(status: EngineStatus): EngineProductState {
  if (status.protocolVersion !== ENGINE_PROTOCOL_VERSION) {
    return {
      kind: "protocol-mismatch",
      message: `The engine speaks bridge protocol v${status.protocolVersion}; this window speaks v${ENGINE_PROTOCOL_VERSION}.`,
    }
  }
  if (!status.initialized || !status.hasVault) {
    return {
      kind: "absent-vault",
      message: !status.initialized
        ? "This project has no knowledge base yet (.athenaeum/ is missing)."
        : "This project has no Markdown vault yet (wiki/ is missing).",
    }
  }
  // `indexStale` is only ever true when an index exists and the vault has
  // moved on; a null `indexedAt` with a vault present means there is no index
  // to be behind yet.
  if (status.indexedAt === null) {
    return {
      kind: "never-indexed",
      message: "The derived index has never been built for this vault.",
    }
  }
  if (status.indexStale) {
    return {
      kind: "stale-index",
      message:
        "Files changed after the index was last built, so what is shown may be behind the vault.",
      indexedAt: status.indexedAt,
    }
  }
  return { kind: "ready" }
}

export function engineStateFromError(cause: unknown): EngineProblemState {
  const error = engineError(cause)
  switch (error.code) {
    case "workspace_not_found":
      return { kind: "absent-vault", message: error.message }
    case "unsupported_protocol":
      return {
        kind: "protocol-mismatch",
        message: `${error.message} (this window speaks bridge protocol v${ENGINE_PROTOCOL_VERSION})`,
      }
    case "engine_protocol_error":
      return { kind: "protocol-mismatch", message: error.message }
    case "incompatible_store":
      return { kind: "protocol-mismatch", message: error.message }
    case "transport":
      return { kind: "unreachable", message: error.message }
    default:
      return { kind: "engine-error", code: error.code, message: error.message }
  }
}

/** A short label for the status strip. */
export function engineStateLabel(state: EngineProductState): string {
  switch (state.kind) {
    case "loading":
      return "Checking the engine…"
    case "ready":
      return "Engine ready"
    case "absent-vault":
      return "No vault here"
    case "never-indexed":
      return "Never indexed"
    case "stale-index":
      return "Index behind the vault"
    case "protocol-mismatch":
      return "Protocol mismatch"
    case "unreachable":
      return "Engine unreachable"
    case "engine-error":
      return "Engine error"
  }
}

// ---------------------------------------------------------------------------
// The hosted Library, as far as this window can honestly see it
// ---------------------------------------------------------------------------

export type LibraryState =
  | { kind: "loading" }
  | { kind: "unlinked"; message: string }
  | { kind: "linked"; server: string; remoteId: string; remoteName: string | null; lastSynced: string | null; lastSyncAt: string | null }
  | { kind: "conflicted"; server: string; remoteId: string; conflicts: string[] }
  | { kind: "error"; message: string }

export function libraryStateFrom(link: {
  linked: boolean
  server: string | null
  remoteId: string | null
  remoteName: string | null
  lastSynced: string | null
  lastSyncAt: string | null
  conflicts: string[]
}): Exclude<LibraryState, { kind: "loading" }> {
  if (!link.linked || !link.server || !link.remoteId) {
    return {
      kind: "unlinked",
      message: "This project is not linked to a Library repository.",
    }
  }
  if (link.conflicts.length > 0) {
    return {
      kind: "conflicted",
      server: link.server,
      remoteId: link.remoteId,
      conflicts: link.conflicts,
    }
  }
  return {
    kind: "linked",
    server: link.server,
    remoteId: link.remoteId,
    remoteName: link.remoteName,
    lastSynced: link.lastSynced,
    lastSyncAt: link.lastSyncAt,
  }
}

export function libraryStateFromError(cause: unknown): Extract<LibraryState, { kind: "error" }> {
  const error = engineError(cause)
  return { kind: "error", message: error.message }
}
