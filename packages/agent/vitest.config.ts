import { defineConfig } from 'vitest/config'
import tsconfigPaths from 'vite-tsconfig-paths'

export default defineConfig({
  plugins: [tsconfigPaths()],
  test: {
    include: ['test/**/*.test.{ts,mjs}'],
    exclude: ['test/harness-e2e.test.ts'],
  },
})
