> Current source layout and root commands: [the consolidation guide](../../../docs/consolidation.md).
> Older sibling-checkout and pnpm development examples below are historical.

# Mounting the bundle, and the entry point Athenaeum drives

Two things use this bundle, and they compose it the same way: a person at a terminal running
`dsh`, and Athenaeum's chat panel driving a `dsh` runtime as a subprocess over stdio
JSON-RPC. Both are one profile with this bundle in its layer stack. Athenaeum owns its
client; this document owns the composition it launches.

Every step below was run against a real harness install and a real `dsh` boot.

## Prerequisites

The three repositories are checked out side by side, as
[AGENTS.md](../AGENTS.md#development) describes, and:

```sh
cd deepseek-harness && pnpm install && pnpm run build:native-system && pnpm run build:lib
cd ../athenaeum        && cargo build --release -p athenaeum-cli   # provides the `athenaeum` command
cd ../athenaeum_agent  && pnpm install && pnpm build
```

(`pnpm run build` in the harness does the same and also builds the web frontend, which this bundle
does not need. [development.md](development.md) says why each step is there.)

`pnpm build` here is not optional. `cordis.patch.yml` names its plugins by package subpath
(`athenaeum-agent/wiki`, `athenaeum-agent/tool-wiki`, …), which resolve through
`package.json` `exports` into `lib/`. A checkout with no `lib/` mounts as a bundle and then
fails to load.

`dsh plugin` shells out to `pnpm`, so `pnpm` must be on `PATH` — a `corepack`-managed shim
counts.

`dsh` below means the launcher. From a side-by-side development checkout there is no
installed binary on `PATH`; it is `node /path/to/deepseek-harness/apps/cli/lib/bin.js`, and
every command here works the same way spelled out like that. Set `DSH_HOME` to keep an
experiment out of `~/.dsh`.

## Mounting the bundle

```sh
dsh plugin --profile athenaeum add /path/to/athenaeum_agent
```

That one command initializes `$DSH_HOME/profiles/athenaeum` over the default bundle list
(`@deepseek-ai/dsh-base`), installs this package as a profile dependency, notices its
`dsh.bundle.patch` declaration, and appends `athenaeum-agent` to `dsh.profile.bundles`. The
resulting manifest:

```json
{
  "name": "dsh-profile-athenaeum",
  "private": true,
  "dependencies": { "athenaeum-agent": "link:/path/to/athenaeum_agent" },
  "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "athenaeum-agent"] } }
}
```

Confirm the five rows landed in the composed tree:

```sh
dsh --profile athenaeum --dump-config
```

They appear under a `# == athenaeum-agent` comment, in `cordis.patch.yml` order, and any of
them can be overridden by the profile's own patch layer.

`--dump-config` only composes; it does not load. A quick way to prove the plugins really
apply is to set `curation: true` without a `curatorModel` and boot: `tool-wiki` refuses at
load with `curation requires curatorModel`, which is a message that can only come from the
plugin having run.

## The entry point Athenaeum's chat panel uses

`@deepseek-ai/dsh-sdk-client` drives a Harness runtime as a subprocess over
newline-delimited JSON-RPC on stdio. `DeepSeekHarness` receives the installed profile, DSH home,
and an explicit path to the same private `dsh` CLI used by the agent launcher. The SDK owns the
child process:

```ts
import { DeepSeekHarness } from '@deepseek-ai/dsh-sdk-client'

await using harness = new DeepSeekHarness({
  profile: 'athenaeum',
  dshHome,
  dshBin: privateHarnessBin,
  cwd: projectDirectory,
  processCwd: projectDirectory,
  provider: 'deepseek-official',
  model: 'deepseek-v4-flash',
})
const result = await harness.run('why does the OAuth callback validate state first?', {
  sessionId,
  onNotification: (notification) => renderHarnessEvent(notification),
})
console.log(result.finalResponse)
```

**The runtime's working directory is what selects the knowledge base.** `wiki-athenaeum`
defaults its `workspace` to the runtime's working directory, and the harness treats the
invoking directory as the workspace root, so launching the runtime in the project is all
that is needed for the seam to consult that project's vault. A panel that opens several
projects at once should instead pin `workspace` per profile, or run one runtime per project.

### Packages the profile needs

The SDK server plugin is what makes a runtime speak that protocol at all, and **no bundle
mounts it.** `@deepseek-ai/dsh-sdk-jsonrpc-server` ships in the harness, but nothing in the
installation's dependency closure depends on it. The SDK client itself is a library the
desktop driver imports, so it also needs to be installed into the profile. Use the exact
Harness release from [`harness.pin.json`](../harness.pin.json):

```sh
dsh plugin --profile athenaeum add @deepseek-ai/dsh-sdk-jsonrpc-server@0.2.1-alpha.1
dsh plugin --profile athenaeum add @deepseek-ai/dsh-sdk-protocol@0.2.1-alpha.1
dsh plugin --profile athenaeum add @deepseek-ai/dsh-sdk-client@0.2.1-alpha.1
```

The SDK server declares no `dsh.bundle`, so it is installed as a plain dependency; the
profile's own patch inserts its runtime row. The SDK protocol and client are also plain
dependencies and do not add runtime rows. The client is constructed with an explicit `dshBin`
pointing to the same private runtime CLI as the installed-agent launcher, which keeps runtime
module resolution in that installation's dependency graph. Copy [`../profile/cordis.patch.yml`](../profile/cordis.patch.yml)
to `$DSH_HOME/profiles/athenaeum/cordis.patch.yml`. The Athenaeum Agent installer performs
these steps with the exact installed package versions and writes the install record the
desktop reads.

The release installer probe initializes and cleanly closes `DeepSeekHarness` against the
installed profile without contacting a model. The SDK's public notification callback carries
session status and events; it does not expose token-level deltas. It also has no per-turn
cancel method, so a client stops a run by closing the runtime.

## Why not ACP

The harness ships an ACP example, and ACP is a reasonable way for an editor to drive an
agent. It is not the path for this panel: it is automation-oriented, and Athenaeum needs the
session event stream and status the SDK protocol carries. The SDK client and the ACP agent
are alternative front doors on the same runtime, so choosing one costs nothing in the
composition below it.

## Degradation, on this path too

Nothing here changes what happens when knowledge is unavailable. A profile that mounts the
bundle in a project with no `.athenaeum/` boots normally, the brief is simply absent, and the
`wiki_*` tools fail per call with a structured error. A missing knowledge base is not a boot
failure, and it must never become one — see [agent-layer.md](agent-layer.md).
