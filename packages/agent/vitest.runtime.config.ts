import { defineConfig } from "vitest/config"
export default defineConfig({ test: { include: ["test/harness-e2e.test.ts"] } })
