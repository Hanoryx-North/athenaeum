import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  cliArchiveName, cliTarget, defaultPrefix, launcherScript, layout, shQuote, withPath,
} from '../install/install.mjs'

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8')

describe('where an install goes', () => {
  it('uses each platform’s per-user program data location', () => {
    expect(defaultPrefix('win32', { LOCALAPPDATA: 'C:\\Users\\Ada\\AppData\\Local' }, 'C:\\Users\\Ada'))
      .toMatch(/Athenaeum[\\/]agent$/)
    expect(defaultPrefix('darwin', {}, '/Users/ada')).toBe('/Users/ada/Library/Application Support/Athenaeum/agent')
    expect(defaultPrefix('linux', {}, '/home/ada')).toBe('/home/ada/.local/share/athenaeum/agent')
  })

  it('honours XDG_DATA_HOME on Linux', () => {
    expect(defaultPrefix('linux', { XDG_DATA_HOME: '/data' }, '/home/ada')).toBe('/data/athenaeum/agent')
  })

  it('never installs outside the prefix it was given', () => {
    const l = layout('/opt/a', 'linux')
    for (const path of Object.values(l)) expect(path.startsWith('/opt/a')).toBe(true)
  })
})

describe('which athenaeum CLI to fetch', () => {
  it('maps every supported platform to a published target', () => {
    expect(cliTarget('linux', 'x64')).toBe('x86_64-unknown-linux-gnu')
    expect(cliTarget('linux', 'arm64')).toBe('aarch64-unknown-linux-gnu')
    expect(cliTarget('darwin', 'arm64')).toBe('aarch64-apple-darwin')
    expect(cliTarget('darwin', 'x64')).toBe('x86_64-apple-darwin')
    expect(cliTarget('win32', 'x64')).toBe('x86_64-pc-windows-msvc')
  })

  it('runs the x64 Windows build on Windows for ARM, since no native one is published', () => {
    expect(cliTarget('win32', 'arm64')).toBe('x86_64-pc-windows-msvc')
  })

  it('refuses a platform nothing is published for rather than guessing', () => {
    expect(() => cliTarget('freebsd', 'x64')).toThrow(/freebsd-x64/)
  })

  it('names archives exactly as the Athenaeum release workflow publishes them', () => {
    expect(cliArchiveName('x86_64-pc-windows-msvc')).toBe('athenaeum-cli-x86_64-pc-windows-msvc.zip')
    expect(cliArchiveName('aarch64-apple-darwin')).toBe('athenaeum-cli-aarch64-apple-darwin.tar.gz')
  })
})

describe('the launcher', () => {
  it('runs dsh’s own entry point with Node, not a .cmd shim, on every platform', () => {
    const unix = launcherScript({ prefix: '/p', nodeDir: '/p/node/bin', platform: 'linux' })
    expect(unix).toContain("exec '/p/node/bin/node' '/p/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js' --profile athenaeum \"$@\"")
    const win = launcherScript({ prefix: 'C:\\A B\\agent', nodeDir: 'C:\\A B\\agent\\node', platform: 'win32' })
    expect(win).toContain('"C:\\A B\\agent\\node\\node.exe" "C:\\A B\\agent\\runtime\\node_modules\\@deepseek-ai\\dsh\\lib\\bin.js" --profile athenaeum %*')
    expect(win).not.toMatch(/\.cmd/)
  })

  it('isolates the harness state from any dsh the person already uses', () => {
    expect(launcherScript({ prefix: '/p', nodeDir: '/n', platform: 'linux' })).toContain("DSH_HOME='/p/dsh-home'")
    expect(launcherScript({ prefix: 'C:\\p', nodeDir: 'C:\\n', platform: 'win32' })).toContain('set "DSH_HOME=C:\\p\\dsh-home"')
  })

  it('survives a path containing a single quote', () => {
    expect(shQuote("it's")).toBe("'it'\\''s'")
    const script = launcherScript({ prefix: "/tmp/o'x", nodeDir: '/n', platform: 'linux' })
    expect(script).toContain("DSH_HOME='/tmp/o'\\''x/dsh-home'")
  })

  it('uses Windows line endings in the batch file and Unix ones in the shell script', () => {
    expect(launcherScript({ prefix: 'C:\\p', nodeDir: 'C:\\n', platform: 'win32' })).toContain('\r\n')
    expect(launcherScript({ prefix: '/p', nodeDir: '/n', platform: 'linux' })).not.toContain('\r')
  })
})

describe('the PATH a child process gets', () => {
  it('updates the existing Path key on Windows instead of adding a second variable', () => {
    const env = withPath({ Path: 'C:\\Windows', SystemRoot: 'C:\\Windows' }, ['C:\\a', 'C:\\b'], 'win32')
    expect(env.Path).toBe('C:\\a;C:\\b;C:\\Windows')
    expect(Object.keys(env).filter(k => k.toLowerCase() === 'path')).toEqual(['Path'])
  })

  it('uses PATH with colons elsewhere, and survives having none', () => {
    expect(withPath({ PATH: '/usr/bin' }, ['/x'], 'linux').PATH).toBe('/x:/usr/bin')
    expect(withPath({}, ['/x'], 'linux').PATH).toBe('/x')
  })
})

describe('the pinned versions', () => {
  const pin = JSON.parse(read('../harness.pin.json'))

  it('pins an exact harness release, not a range or a dist-tag', () => {
    expect(pin.version).toMatch(/^\d+\.\d+\.\d+(-[\w.]+)?$/)
    expect(pin.gitTag).toBe(`dsh-v${pin.version}`)
  })

  it('pins the same Node, with the same checksums, in both bootstrap scripts', () => {
    const sh = read('../install/install.sh')
    const ps = read('../install/install.ps1')
    const version = /NODE_VERSION="(v[\d.]+)"/.exec(sh)?.[1]
    expect(version).toBeDefined()
    expect(ps).toContain(`$NodeVersion = '${version}'`)
    // Windows hashes are in the .ps1; macOS and Linux in the .sh. Every one is a full SHA-256.
    for (const hash of [...sh.matchAll(/sha=([0-9a-f]+)/g), ...ps.matchAll(/\$Sha = '([0-9a-f]+)'/g)]) {
      expect(hash[1]).toMatch(/^[0-9a-f]{64}$/)
    }
  })
})
