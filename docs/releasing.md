# Releasing

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
   | macOS, Apple silicon | `Athenaeum_<v>_aarch64.dmg` | `Athenaeum_aarch64.app.tar.gz` |
   | macOS, Intel | `Athenaeum_<v>_x64.dmg` | `Athenaeum_x64.app.tar.gz` |
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

Each release also carries `install.sh`, `install.ps1`, `install.mjs` and `athenaeum-integrations.mjs`: the
one-line installer for coding agents and the single-file bundle it installs
([installing.md](installing.md)). The bundle is built, by the `integrations` job, from
`athenaeum_integrations` at the revision named in `integrations.pin.json`, next to
`athenaeum_protocol`. Merge the repositories in that order (protocol, integrations, then this one) so the pins
resolve. `ATHENAEUM_PROTOCOL_TOKEN` must be able to read **both** `athenaeum_protocol` and
`athenaeum_integrations`. The release is not published unless all four files are attached.

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
  signed and notarized and opens with no prompt.
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

- **The agent.** The in-app chat does not yet drive `athenaeum_agent`; that wiring is the
  first open item in [porting-llm-wiki.md](porting-llm-wiki.md#status). The CLI archives
  here are what that layer's installer fetches.
- **Linux package repositories.** `.deb` installs do not self-update; AppImage does.
- **Windows on ARM.**
