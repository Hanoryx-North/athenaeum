export interface ArgumentSchema {
  type?: string
  enum?: unknown[]
  anyOf?: ArgumentSchema[]
  format?: "set" | "date"
  properties?: Record<string, ArgumentSchema>
  required?: string[]
}

export interface DesktopAction {
  name: string
  kind: "store" | "service" | "app"
  module?: string
  exportName?: string
  member?: string | null
  signature: string
  source?: string
  description?: string
  parameters: Array<{ name: string; type: string; optional: boolean; schema: ArgumentSchema }>
}

export interface DesktopStore {
  name: string
  module: string
  exportName: string
}

export interface DesktopCatalog {
  entries: DesktopAction[]
  stores: DesktopStore[]
  native: Array<{ name: string; signature: string; source: string; transport: string }>
}
