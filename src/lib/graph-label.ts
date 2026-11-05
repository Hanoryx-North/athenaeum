export const GRAPH_LABEL_LIMITS = { words: 4, characters: 32 } as const
const MAX_LABEL_WORDS = GRAPH_LABEL_LIMITS.words
const MAX_LABEL_CHARACTERS = GRAPH_LABEL_LIMITS.characters

/** Compact canvas text only; keep the original title for search and inspection. */
export function graphLabel(title: string): string {
  const normalized = title.trim().replace(/\s+/gu, " ")
  const words = normalized.split(" ")
  const prefix = words.slice(0, MAX_LABEL_WORDS).join(" ")
  const characters = Array.from(prefix)

  if (words.length <= MAX_LABEL_WORDS && characters.length <= MAX_LABEL_CHARACTERS) {
    return normalized
  }

  if (characters.length < MAX_LABEL_CHARACTERS) return `${prefix}…`

  const clipped = characters.slice(0, MAX_LABEL_CHARACTERS - 1).join("")
  const lastSpace = clipped.lastIndexOf(" ")
  return `${lastSpace > 0 ? clipped.slice(0, lastSpace) : clipped}…`
}
