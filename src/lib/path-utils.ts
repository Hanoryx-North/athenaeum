/**
 * Normalize a path to use forward slashes (works on both macOS and Windows).
 * Windows APIs accept forward slashes, so normalizing to / is safe everywhere.
 */
export function normalizePath(p: string): string {
  return p.replace(/\\/g, "/")
}

/**
 * Join path segments with forward slashes.
 */
export function joinPath(...segments: string[]): string {
  return segments
    .map((s) => s.replace(/\\/g, "/"))
    .join("/")
    .replace(/\/+/g, "/")
}

/**
 * Get the filename from a path (handles both / and \).
 */
export function getFileName(p: string): string {
  const normalized = p.replace(/\\/g, "/")
  return normalized.split("/").pop() ?? p
}

/**
 * Get the file stem (filename without extension).
 */
export function getFileStem(p: string): string {
  const name = getFileName(p)
  const lastDot = name.lastIndexOf(".")
  return lastDot > 0 ? name.slice(0, lastDot) : name
}

// Windows drive-letter and UNC paths are case-insensitive; fold them for
// comparison purposes only (never for the paths actually returned/written).
function caseFoldPath(normalized: string): string {
  return /^[A-Za-z]:\//.test(normalized) || normalized.startsWith("//")
    ? normalized.toLowerCase()
    : normalized
}

/**
 * Get relative path from base.
 */
export function getRelativePath(fullPath: string, basePath: string): string {
  const normalFull = normalizePath(fullPath)
  const normalBase = normalizePath(basePath).replace(/\/$/, "")
  const fullKey = caseFoldPath(normalFull)
  const baseKey = caseFoldPath(normalBase)
  if (fullKey.startsWith(baseKey + "/")) {
    // Slice by path segments rather than the original string length. Unicode
    // case folding can change UTF-16 length, so an offset derived from the
    // differently-cased base can split the returned relative path incorrectly.
    return normalFull.split("/").slice(normalBase.split("/").length).join("/")
  }
  return normalFull
}

/**
 * Cross-platform absolute-path detection.
 *
 * Unix:     "/foo/bar"
 * Windows:  "C:\foo", "C:/foo", "\\server\share", "//server/share"
 *
 * A bare `.startsWith("/")` check wrongly treats Windows paths like
 * "C:/project/file.pdf" as relative, which produced double-joined
 * garbage like "C:/project/C:/project/file.pdf" in the ingest queue.
 */
export function isAbsolutePath(p: string): boolean {
  if (!p) return false
  if (p.startsWith("/")) return true
  if (/^[A-Za-z]:[\\/]/.test(p)) return true
  if (p.startsWith("\\\\") || p.startsWith("//")) return true
  return false
}

/** Which platform's path rules apply to paths the backend will act on. */
export type HostConvention = "windows" | "unix" | "unknown"

/**
 * The host the Rust side is running on, read from the webview user agent —
 * the same source `main.tsx` already uses for platform-specific UI, chosen
 * over the Tauri OS plugin so this costs no new dependency or capability.
 */
export function hostConvention(): HostConvention {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent
  if (/Windows/i.test(ua)) return "windows"
  if (/Mac OS X|Macintosh|Linux|X11|CrOS|BSD/i.test(ua)) return "unix"
  return "unknown"
}

/**
 * Absolute *for the host the backend runs on*, mirroring `require_absolute_path`
 * in `src-tauri/src/commands/fs.rs` so the two layers agree about one path.
 *
 * Deliberately separate from `isAbsolutePath`, which stays cross-platform: the
 * ingest queue classifies stored path strings that may have been written on
 * another machine, where either convention is legitimate. Only a path about to
 * be handed to a write command has to satisfy the host it will be written on.
 *
 * An unidentified host falls back to the permissive answer. The backend is
 * authoritative either way, and guessing wrong here would refuse writes that
 * are perfectly valid — on Windows, POSIX rules would reject every `C:\…` path.
 */
export function isHostAbsolutePath(
  p: string,
  host: HostConvention = hostConvention(),
): boolean {
  if (!p) return false
  switch (host) {
    case "windows":
      // A drive root in either slash style, or a UNC share. A lone leading
      // slash is drive-relative on Windows, so it is not absolute.
      return /^[A-Za-z]:[\\/]/.test(p) || p.startsWith("\\\\") || p.startsWith("//")
    case "unix":
      // Only a POSIX root. `C:/wiki` names a directory called `C:` here, and
      // `\\server\share` is an ordinary relative file name.
      return p.startsWith("/")
    case "unknown":
      return isAbsolutePath(p)
  }
}
