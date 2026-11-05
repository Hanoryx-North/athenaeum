import type { ArgumentSchema } from "@/types/desktop-control"

export function matchesArgument(value: unknown, schema: ArgumentSchema): boolean {
  if (schema.anyOf) return schema.anyOf.some(part => matchesArgument(value, part))
  if (schema.enum) return schema.enum.includes(value)
  switch (schema.type) {
    case "never": return false
    case "null": return value === null
    case "array": return Array.isArray(value)
    case "object": {
      if (!value || typeof value !== "object" || Array.isArray(value)) return false
      const object = value as Record<string, unknown>
      if (schema.required?.some(key => !(key in object))) return false
      return Object.entries(schema.properties ?? {}).every(([key, child]) => !(key in object) || matchesArgument(object[key], child))
    }
    case "number": return typeof value === "number" && Number.isFinite(value)
    case "string": return typeof value === "string"
    case "boolean": return typeof value === "boolean"
    default: return true
  }
}

export function decodeArgument(value: unknown, schema: ArgumentSchema): unknown {
  if (schema.anyOf) {
    const matching = schema.anyOf.find(part => matchesArgument(value, part))
    return matching ? decodeArgument(value, matching) : value
  }
  if (schema.format === "set" && Array.isArray(value)) return new Set(value)
  if (schema.format === "date" && typeof value === "string") {
    const date = new Date(value)
    if (!Number.isFinite(date.getTime())) throw new Error("Invalid ISO date argument")
    return date
  }
  if (value && typeof value === "object" && !Array.isArray(value) && schema.properties) {
    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, schema.properties?.[key] ? decodeArgument(child, schema.properties[key]) : child]))
  }
  return value
}

export function safeDesktopResult(value: unknown, reveal = false): unknown {
  const seen = new WeakSet<object>()
  return JSON.parse(JSON.stringify(value ?? null, (key, item: unknown) => {
    if (!reveal && /api.?key|token|secret|password|authorization|customheaders|cookie/i.test(key)) return "[redacted]"
    if (typeof item === "function") return undefined
    if (item instanceof Set) return [...item]
    if (item instanceof Map) return Object.fromEntries(item)
    if (item && typeof item === "object") {
      if (seen.has(item)) return "[circular]"
      seen.add(item)
    }
    return item
  })) as unknown
}

