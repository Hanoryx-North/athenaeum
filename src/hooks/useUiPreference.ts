import { useState, type SetStateAction } from "react"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

/** Use a stable key for the lifetime of the mounted component. */
export function useUiPreference<T>(key: string, fallback: T, valid: (value: T) => boolean = () => true) {
  const [value, setValue] = useState<T>(() => {
    const saved = readUiPreference(key, fallback)
    return valid(saved) ? saved : fallback
  })
  function update(next: SetStateAction<T>) {
    setValue((current) => {
      const resolved = typeof next === "function" ? (next as (value: T) => T)(current) : next
      saveUiPreference(key, resolved)
      return resolved
    })
  }
  return [value, update] as const
}
