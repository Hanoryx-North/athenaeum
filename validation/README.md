# Athenaeum validation

Run from the repository root:

```sh
python3 validation/verify_all.py --list
python3 validation/verify_all.py --only protocol,engine,agent,product-cli --fail-on-skip
python3 validation/verify_all.py --results validation/results.json --fail-on-skip
```

The runner tests the current source tree directly. Unit tests live next to the
code they exercise; `product_tests/` drives the real CLI, bridge, MCP server and
agent client in temporary projects/homes. No sibling repositories, historical
Git revisions or restored scratch copies are needed. Build workspace dependencies
with `npm ci && npm run build:packages` first. The runner builds the CLI when
needed; `ATHENAEUM_BIN` may select an existing build.

`--only`, `--skip` and `--with` take comma-separated step names. A missing tool or
an individual skipped test is recorded as skipped, never as passed; use
`--fail-on-skip` for strict runs. `app-desktop`, `native-packaging`, `harness-runtime`
and `bench` are optional and describe their requirements in `--list`.

The upstream Harness test requires `DSH_REPO` pointing at a built checkout of the
pinned release. The source-verified native runtime is built with `npm run native:build -w
athenaeum-agent`; `native-agent-smoke` exercises the executable and desktop adapter
against a scripted loopback provider. Native packaging requires `ATHENAEUM_NATIVE_TOOLS`; neither is
claimed by the ordinary local adapter/product checks. Browser setup checks remain
available through `node validation/product_tests/setup_ui_smoke.cjs` with a Vite
server and Playwright. These simulate the Tauri boundary.
