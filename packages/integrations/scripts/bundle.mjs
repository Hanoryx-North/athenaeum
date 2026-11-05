// Bundles the command line, the hook handlers and the MCP server into ONE file that needs nothing
// but Node: `dist/athenaeum-integrations.mjs`. That is what the Athenaeum installer ships, so an
// agent's hook command is "node that-file hook claude …" with no node_modules to go missing.
import { build } from 'esbuild';
import { chmodSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

mkdirSync(new URL('../dist', import.meta.url), { recursive: true });
const outfile = fileURLToPath(new URL('../dist/athenaeum-integrations.mjs', import.meta.url));

await build({
  entryPoints: [fileURLToPath(new URL('../src/cli.ts', import.meta.url))],
  outfile,
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  // The MCP server is only loaded for `mcp`, so hooks (which run on every prompt) start fast.
  splitting: false,
  minify: false,
  legalComments: 'none',
  // Some bundled dependencies still use require(); give them one in an ES module.
  banner: { js: "import { createRequire as __cr } from 'node:module'; const require = __cr(import.meta.url);" },
  logLevel: 'info',
});
chmodSync(outfile, 0o755);
