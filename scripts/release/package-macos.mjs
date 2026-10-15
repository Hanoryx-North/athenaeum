#!/usr/bin/env node
// Apple's Installer.app provides the native Mac wizard; Tauri still produces
// the .app, DMG, and signed updater archive. This does not alter the signed app.
import { execFileSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const [app, version, arch, output] = process.argv.slice(2)
if (process.platform !== 'darwin' || !app || !/^\d+\.\d+\.\d+$/.test(version ?? '') || !['aarch64', 'x64'].includes(arch) || !output || !existsSync(app)) {
  throw new Error('Usage (macOS): package-macos.mjs APP VERSION aarch64|x64 OUTPUT_DIR')
}
const work = mkdtempSync(join(tmpdir(), 'athenaeum-pkg-'))
const run = (cmd, args) => {
  try { return execFileSync(cmd, args, { stdio: 'inherit' }) }
  catch (error) { throw new Error(`${cmd} failed (exit ${error.status ?? 'unknown'}). Review the output above.`) }
}
const security = args => {
  try { return execFileSync('security', args) }
  catch { throw new Error(`Installer signing keychain operation failed: ${args[0]}. Check the certificate and signing identity.`) }
}
let keychain
try {
  mkdirSync(output, { recursive: true })
  const component = join(work, 'Athenaeum-component.pkg')
  run('pkgbuild', ['--component', resolve(app), '--install-location', '/Applications',
    '--identifier', 'com.hanoryx.athenaeum', '--version', version, component])
  writeFileSync(join(work, 'welcome.html'), '<html><body><h1>Install Athenaeum</h1><p>Code with Athenaeum Agent or connect your existing coding agent. Both build the same local Markdown wiki.</p><p>This package includes the desktop app, the athenaeum CLI, a private Node runtime, and coding-agent integration. After installing, open a project and finish setup in Knowledge → Library. Athenaeum Agent setup downloads its runtime when selected.</p></body></html>\n')
  writeFileSync(join(work, 'conclusion.html'), '<html><body><h1>Open Athenaeum to finish setup</h1><p>Open Athenaeum from Applications, open your project, then go to Knowledge → Library. Choose your coding agent and optionally connect Athenaeum Library.</p><p>Review agent proposals before accepting them as wiki pages. Only accepted, explicitly shared wiki text is synced.</p></body></html>\n')
  writeFileSync(join(work, 'distribution.xml'), `<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>Athenaeum</title>
  <welcome file="welcome.html"/>
  <conclusion file="conclusion.html"/>
  <options customize="never" require-scripts="false" hostArchitectures="${arch === 'aarch64' ? 'arm64' : 'x86_64'}"/>
  <domains enable_localSystem="true" enable_currentUserHome="false" enable_anywhere="false"/>
  <choices-outline><line choice="app"/></choices-outline>
  <choice id="app" title="Athenaeum"><pkg-ref id="com.hanoryx.athenaeum"/></choice>
  <pkg-ref id="com.hanoryx.athenaeum" version="${version}">Athenaeum-component.pkg</pkg-ref>
</installer-gui-script>
`)
  const args = ['--distribution', join(work, 'distribution.xml'), '--resources', work, '--package-path', work]
  if (process.env.APPLE_INSTALLER_CERTIFICATE) {
    if (!process.env.APPLE_INSTALLER_SIGNING_IDENTITY) throw new Error('Installer certificate needs APPLE_INSTALLER_SIGNING_IDENTITY')
    const certificate = join(work, 'installer.p12')
    writeFileSync(certificate, Buffer.from(process.env.APPLE_INSTALLER_CERTIFICATE, 'base64'))
    keychain = join(work, 'installer.keychain-db')
    const password = randomBytes(32).toString('hex')
    // Keep this separate from Tauri's temporary app-signing keychain, whose
    // lifecycle ends with tauri build. Never install signing keys on a user's PC.
    security(['create-keychain', '-p', password, keychain])
    security(['unlock-keychain', '-p', password, keychain])
    security(['import', certificate, '-k', keychain, '-P', process.env.APPLE_INSTALLER_CERTIFICATE_PASSWORD ?? '', '-T', '/usr/bin/productbuild', '-T', '/usr/bin/productsign'])
    security(['set-key-partition-list', '-S', 'apple-tool:,apple:', '-s', '-k', password, keychain])
    args.push('--keychain', keychain)
  }
  if (process.env.APPLE_INSTALLER_SIGNING_IDENTITY) args.push('--sign', process.env.APPLE_INSTALLER_SIGNING_IDENTITY)
  const pkg = resolve(output, `Athenaeum_${version}_${arch}.pkg`)
  run('productbuild', [...args, pkg])
  // Installer signatures and app signatures use different Apple identities.
  if (process.env.APPLE_ID && process.env.APPLE_PASSWORD && process.env.APPLE_TEAM_ID && process.env.APPLE_INSTALLER_SIGNING_IDENTITY) {
    run('xcrun', ['notarytool', 'submit', pkg, '--apple-id', process.env.APPLE_ID,
      '--password', process.env.APPLE_PASSWORD, '--team-id', process.env.APPLE_TEAM_ID, '--wait'])
    run('xcrun', ['stapler', 'staple', pkg])
  }
} finally {
  if (keychain) {
    try { execFileSync('security', ['delete-keychain', keychain]) } catch { /* temp files are removed below */ }
  }
  rmSync(work, { recursive: true, force: true })
}
