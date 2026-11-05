> Current source layout and root commands: [the consolidation guide](../../../docs/consolidation.md).
> Older sibling-checkout and pnpm development examples below are historical.

# Developing this bundle

This bundle is a plugin inside the DeepSeek Harness and a client of the Athenaeum library, so
developing it means having both next to it, built. The harness is published (`@deepseek-ai/dsh`, which is what the installer ships), but `package.json`
links its packages from a sibling checkout of the release pinned in `harness.pin.json`, so the bundle is
compiled and tested against exactly what a user installs.

```text
parent/
├── deepseek-harness/     pnpm install
│                         pnpm run build:native-system
│                         pnpm run build:lib
├── athenaeum/            cargo build --release -p athenaeum-cli     # provides `athenaeum`
└── athenaeum_agent/      pnpm install && pnpm verify
```

`pnpm` must be the version the harness pins (`packageManager` in its `package.json`); `corepack`
fetches it. A `pnpm` from somewhere else reads `pnpm-workspace.yaml` differently and silently applies
its own defaults.

## Build order, and why each step is there

| In | Run | Because |
| --- | --- | --- |
| `deepseek-harness` | `pnpm install` | Installs the workspace the packages this bundle links against depend on. |
| `deepseek-harness` | `pnpm run build:native-system` | The `dsh` runtime loads a native addon at boot. Without it, boot fails with `Cannot find module …/system.node`. |
| `deepseek-harness` | `pnpm run build:lib` | Emits both halves of `lib/`. The host pass produces the declaration files this repository typechecks against and the entry points it imports; the client pass produces the typert and gateway entry points a profile mounts. With only the host pass, boot warns `3 entries did not activate` (`typert`, `typert-loader`, `typert-gateway`). |
| `athenaeum` | `cargo build --release -p athenaeum-cli` | The real-bridge tests and the harness test spawn it. Linux also needs the webview headers and `protobuf-compiler` + `libprotobuf-dev` only to build the desktop crate, not this CLI. |
| `athenaeum_agent` | `pnpm install` | Links the harness packages. |

`pnpm run build` in the harness runs the same two steps plus the web frontend, which this bundle does
not need.

## Verifying

```sh
pnpm verify                # typecheck, build, every test — and fails if any test was skipped
pnpm verify --allow-skips  # run what can run, report what could not
```

A plain `pnpm test` is deliberately usable without the siblings: the tests that need one self-skip.
That is also why a green `pnpm test` proves nothing about the layers it skipped. `pnpm verify` resolves
the siblings, says exactly what is not built and how to build it, runs the suite with the locations
exported, and refuses to call a run that skipped anything a pass. Its signal is
`128 passed, 0 failed, 0 skipped, of 128`.

The locations default to the layout above and can be overridden:

| Variable | Default | Used by |
| --- | --- | --- |
| `ATHENAEUM_BIN` | `../athenaeum/target/release/athenaeum` | the real-bridge, real-vault and harness tests |
| `ATHENAEUM_REPO` | `../athenaeum` | the vendored-fixture drift check |
| `DSH_REPO` | `../deepseek-harness` | the harness test |

### What each layer proves

1. **Client and fixture tests** run Athenaeum's published protocol v1 fixtures through this
   repository's client and types.
2. **Seam, provider and consumer tests** mount each plugin into a real Cordis context with a scripted
   provider.
3. **`end-to-end.test.ts`** builds a real knowledge base and a real vault with the real CLI and drives
   the real bridge: negotiation, the product thesis (a decision whose wording shares no term with the
   task is reached by a relationship, not by search), the page pointer, and an edit or a new page seen
   without a re-index.
4. **`harness-e2e.test.ts`** boots the real `dsh` runtime from the harness's built launcher with this
   checkout installed as a bundle, the real `athenaeum` behind it, and the harness's own scripted
   model server in front. It asserts what an agent actually does with the seam: the brief reaches the
   model, `wiki_open` runs over the bridge, the nodes the answer cited are journaled, and the loop the
   product is for — one session writes a wiki page with the harness's own `write` tool and a later
   session finds it through `wiki_search`, with no re-index in between. It is the only test that
   notices when the harness changes an API this bundle uses — the utility report once
   read `agent.session.events`, which the harness had removed, and every briefed turn failed here
   while the stubbed unit tests stayed green.

### Re-syncing the vendored fixtures

`fixtures/wiki-protocol/v1/` is a verbatim copy of Athenaeum's. When Athenaeum changes the protocol,
copy `requests.json` and `responses.json` from its checkout and set `Upstream commit` in
`MANIFEST.md` to the commit that last changed those two files
(`git log -1 --format=%H -- fixtures/wiki-protocol/v1` in Athenaeum), in the same change. The drift
check compares the copy with `$ATHENAEUM_REPO` and fails if they differ.

## Running the bundle by hand

[mounting.md](mounting.md) is the reference for installing it into a profile. Two things bite in
development:

- **Launch the harness through its built launcher**, `node /path/to/deepseek-harness/apps/cli/lib/bin.js`
  — what `dsh` means in these docs — never `pnpm dsh`. A source launch resolves the harness's own packages
  to `src/` while a profile resolves them to `lib/`, which gives the process two copies of `dsh-tools`.
  The agent loop then reads a scheduler symbol the tool registry never set, and the first tool call fails
  with `Cannot read properties of undefined (reading 'prepare')`, for a profile with this bundle and
  for one without.
- **Run it without a key** by putting the harness's scripted model server in front. Start
  `startMockLlmServer` from `packages/test-support/llm-mock-server`, set `DEEPSEEK_BASE_URL` to its
  `/v1` URL and `DEEPSEEK_API_KEY` to anything. The adapter speaks the Messages protocol and the mock
  server serves it, so no wire needs selecting (an older `protocol: chat-completions` row is now
  refused at load). Switch the session log off so a test run sends nothing anywhere:

  ```yaml
  - id: session-log-deepseek
    config:
      enabled: false
  ```

  `harness-e2e.test.ts` is the worked example.

## The retired Rust runtime

`crates/` is not part of any of this. Nothing here builds or tests it, and
[runtime-pivot.md](../../../docs/consolidation.md) records why it stays untouched.
