/** Local UI preferences only. Credentials and connection truth belong to their owners. */
export function readUiPreference<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(`athenaeum.ui:${key}`)
    return value === null ? fallback : JSON.parse(value) as T
  } catch {
    return fallback
  }
}

export function saveUiPreference(key: string, value: unknown): void {
  try {
    localStorage.setItem(`athenaeum.ui:${key}`, JSON.stringify(value))
  } catch (error) {
    console.warn("Could not save UI preference:", key, error)
  }
}
