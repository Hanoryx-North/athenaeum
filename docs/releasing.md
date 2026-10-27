# Releasing

## Local Windows candidate, 2026-10-27

The local 0.1.0 NSIS candidate is built with `tauri.release.conf.json` and packaged CLI, private Node,
MCP and agent installer resources. Its build output is
`target/release/bundle/nsis/Athenaeum_0.1.0_x64-setup.exe`. It is unsigned. See
`athenaeum_validation/RELEASE_COMPLETION.md` for the checksum, installed-app verification and limits.
The GitHub repository is private with no latest release. The automated workflow below describes
the publication mechanism; a public download, updater deployment or other-platform build is not
established by this local package. No repository privacy setting has been changed.

## Automated publication

Every push to `main` that changes more than documentation becomes a release. Nobody
tags anything by hand. This document says what that produces, the one-time setup it
needs, and what to do when it fails.

## What a push to main does

`.github/workflows/release.yml`:

1. **Plans** the version, runs the frontend typecheck and tests (a broken `main` must
   not ship), refuses to continue if auto-update is not set up, and opens a *draft*
   GitHub release.
2. **Builds** five targets in parallel and attaches the results to the draft:

   | Platform | Installer people download | Update bundle the app installs from |
   | --- | --- | --- |
   | Windows x64 | `Athenaeum_<v>_x64-setup.exe` (NSIS wizard) | the same `.exe` |
   | macOS, Apple silicon | `Athenaeum_<v>_aarch64.pkg` wizard, plus `.dmg` | `Athenaeum_aarch64.app.tar.gz` |
   | macOS, Intel | `Athenaeum_<v>_x64.pkg` wizard, plus `.dmg` | `Athenaeum_x64.app.tar.gz` |
   | Linux x64 | `.AppImage`, `.deb` | the `.AppImage` |
   | Linux arm64 | `.AppImage`, `.deb` | the `.AppImage` |

   Each also gets `athenaeum-cli-<target>.(tar.gz|zip)`: the `athenaeum` command line
   tool, which the agent layer talks to. These have stable names, so
   `…/releases/latest/download/athenaeum-cli-x86_64-unknown-linux-gnu.tar.gz` always
   resolves to the newest.

   Every update bundle is **verified against the public key committed in
   `tauri.conf.json`** before it is attached. A signing secret that does not match the
   committed key fails the build instead of shipping a release no installed copy would
   accept.
3. **Publishes**: writes `latest.json` (the manifest installed apps read) from the
   signed bundles, then flips the draft to a published release. Publishing last is what
   makes it atomic; `releases/latest/download/latest.json` does not exist until then.

Installed copies check at launch and every hour after, show **Update and restart** in a
banner, and install in place. If an in-place install is not possible (a `.deb` install,
no network), the button opens the download page instead of doing nothing.

### The installer and the agent integration

Every desktop package carries the architecture-matched CLI archive, a checksum-pinned private
Node (including npm), the single-file MCP integration, and the Athenaeum Agent bundle and
installer. `package-tools.mjs` stages them under `src-tauri/tools/` before Tauri bundles them.
The guided setup runs Node directly, without `sh`, a PowerShell bootstrap, or a system Node.
Base setup works offline; the optional agent setup installs its pinned Harness runtime from npm.

The `integrations` job builds `athenaeum_integrations`; `agent-package` builds the MIT agent
adapter against its pinned, unmodified upstream Harness. `package-agent.mjs` bundles the MIT
protocol dependency into the adapter tarball so the installed package needs no sibling checkout.
These stay separate artifacts and processes; no GPL engine code goes into either MIT adapter.
Revisions come from `integrations.pin.json`.

`ATHENAEUM_PROTOCOL_TOKEN` must read `athenaeum_protocol`, `athenaeum_integrations`, and
`athenaeum_agent`. The platform jobs wait for both adapters and download their workflow artifacts.
Merge protocol and adapter changes before updating the app's pins.

On macOS, `package-macos.mjs` uses Apple's `pkgbuild` and `productbuild` to wrap the built
`.app` in a native Installer wizard with welcome and completion instructions. The `.dmg`
and updater archive remain available. The `.pkg` installs the app into `/Applications`;
per-user CLI and agent setup happens in the app, without running installer scripts as root.
On Windows the existing NSIS wizard carries the same setup resources. Publishing checks for
both Mac `.pkg` architectures and the Windows setup executable as well as all CLI archives.

Releases also retain `install.sh`, `install.ps1`, `install.mjs` and
`athenaeum-integrations.mjs` for advanced or CLI-only installs. They are optional alternatives,
not the Mac/Windows onboarding path.

### Versions

`MAJOR.MINOR` come from `version` in `package.json`. The patch is the number of commits on
`main`, so every release is strictly newer than the last and re-running a build gives the
same version. For a bigger release, change `MAJOR.MINOR` in `package.json` (and the
workspace version in `Cargo.toml`) by hand.

Documentation-only pushes (`*.md`, `docs/`) do not release. Several pushes in a row release
only the newest.

## One-time setup: the update signing key

Updates are signed so that nobody who gets into GitHub's storage, or the network between
it and a user, can push code to installed copies. The app trusts exactly one public key,
compiled into it. You create the key pair once:

```sh
npm ci
node scripts/release/setup-updater-key.mjs
```

It writes the key pair to `~/.athenaeum-release/`, puts the **public** key in
`src-tauri/tauri.conf.json`, and (with the GitHub CLI logged in) stores the private key and
its password as the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Then commit `tauri.conf.json` and push. Without
`gh`, run it with `--no-secrets` and it tells you what to paste where.

> **Back up `~/.athenaeum-release/`.** Lose the private key and every installed copy is
> stranded on its current version, because it will only accept updates signed by that key;
> the fix is a manual reinstall. Leak it and anyone can push code to every user. The script
> never prints the key and never puts it in the repository.

Until this is done, pushes to `main` fail at the first step with a message saying so. That
is deliberate: a release built without the key installs fine but can never update itself,
and the people who installed it could not be rescued.

## Dry runs

*Actions → Release → Run workflow* on any branch builds and signs everything with a
throwaway key and keeps the bundles as workflow artifacts. It publishes nothing, so use it
to check a change to the installers before it reaches `main`. Ticking **publish** on a run
from `main` releases without a push.

To try one platform rather than all five, give **only** a target triple (for example
`aarch64-unknown-linux-gnu`). A run that publishes always builds everything.

### Checking packaged setup locally

The packaging smoke tests live in `athenaeum_validation`, not this repository. With the
architecture-matched `src-tauri/tools` directory staged by `package-tools.mjs`, run:

```sh
cd ../athenaeum_validation
ATHENAEUM_NATIVE_TOOLS=/absolute/path/to/athenaeum/src-tauri/tools python3 verify_all.py --only native-packaging
```

This verifies offline base installation and update, self-contained adapter packaging, real
agent SDK initialization without a model call, and the Mac `.pkg` structure on macOS.
Use the release workflow's target dry runs to verify the full Windows NSIS and Mac app bundles.

### What a release costs

A full release is five builds, the Windows one taking over an hour. **In a private repository
Actions minutes are billed, and macOS minutes count ten times**, so one release is on the order of
five hundred billable minutes against a free allowance of two thousand a month. In a public
repository they are free. If this repository stays private, expect a few releases a month to use the
allowance, and consider releasing less often (change the trigger to tags, or to a schedule) or
adding a paid plan.

## First launch on macOS and Windows

The installers are not yet signed with a paid code-signing identity, so each operating
system asks for a one-time approval. Updates after that do not ask again, because the app
downloads them itself.

- **Windows:** SmartScreen shows "Windows protected your PC". Click **More info**, then
  **Run anyway**.
- **macOS:** open Athenaeum from Applications; it is blocked. Open **System Settings →
  Privacy & Security**, scroll to the message about Athenaeum and click **Open Anyway**. On
  older macOS, right-click the app and choose **Open** instead.

### Removing the prompts

Both are optional secrets; the workflow uses them when present and never fails without them.

- **macOS** needs an Apple Developer ID (US$99/year): set `APPLE_CERTIFICATE`
  (base64 of the `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. The app is then
  signed and notarized. For the `.pkg`, also set `APPLE_INSTALLER_SIGNING_IDENTITY` to a
  **Developer ID Installer** identity, plus `APPLE_INSTALLER_CERTIFICATE` (base64 of its
  `.p12`) and `APPLE_INSTALLER_CERTIFICATE_PASSWORD`. Packaging imports this into a separate
  temporary keychain and deletes it after signing. The package is signed
  and, when Apple account credentials are present, notarized and stapled separately.
- **Windows** needs a code-signing certificate. [SignPath Foundation](https://signpath.org/)
  signs open-source projects for free; adding it is a change to the build step, not yet
  made here.

## When something fails

| Symptom | Cause and fix |
| --- | --- |
| Run fails in *Plan* with "Updater signing key missing" | The one-time setup above has not been done. |
| Build fails with "does not verify against the updater public key" | `TAURI_SIGNING_PRIVATE_KEY` is not the private half of the key in `tauri.conf.json`. Re-run `setup-updater-key.mjs`. |
| Build fails with "no unique signed bundle for …" | A platform's update bundle was not produced or was renamed. The *Collect the release assets* step lists what was built. |
| Linux arm64 stops with "the runner has received a shutdown signal" | The runner ran out of memory in the final link. That leg builds with thin LTO for this reason (`thin_lto` in `scripts/release/lib.mjs`). If it still fails, drop the platform from `BUILDS` rather than ship without it silently: the update manifest refuses to build with a platform missing. |
| A run fails after the draft was created | The draft stays, unpublished and invisible to the app. Fixing and re-running the same commit reuses it. |
| An installed copy never offers the update | It was installed from a build made before the key was set up, or from a `.deb`. Reinstall from the latest release once. |

## What this does not cover

- **Agent runtime updates during app auto-update.** The app carries updated setup resources;
  run guided setup again to apply them to the separately installed agent. The app chat uses
  that installed runtime through the public Harness SDK.
- **Linux package repositories.** `.deb` installs do not self-update; AppImage does.
- **Windows on ARM.**
