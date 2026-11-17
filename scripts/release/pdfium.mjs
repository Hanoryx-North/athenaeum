// Shared by release installers and Relay's local app bundle.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PDFIUM_BY_TARGET, pdfiumUrl } from './lib.mjs'

export async function downloadPdfium(target, destination) {
  const entry = PDFIUM_BY_TARGET[target]
  if (!entry) throw new Error(`No PDFium build is pinned for target "${target}"`)
  const response = await fetch(pdfiumUrl(entry), { redirect: 'follow' })
  if (!response.ok) throw new Error(`PDFium download failed: HTTP ${response.status}`)
  const bytes = Buffer.from(await response.arrayBuffer())
  if (createHash('sha256').update(bytes).digest('hex') !== entry.sha256) {
    throw new Error(`PDFium ${entry.asset} checksum mismatch`)
  }
  const work = mkdtempSync(join(tmpdir(), 'pdfium-'))
  try {
    writeFileSync(join(work, entry.asset), bytes)
    execFileSync('tar', ['-xzf', entry.asset, entry.inArchive, 'LICENSE', 'licenses'], { cwd: work, stdio: 'inherit' })
    rmSync(destination, { recursive: true, force: true })
    mkdirSync(destination, { recursive: true })
    cpSync(join(work, entry.inArchive), join(destination, entry.file))
    cpSync(join(work, 'LICENSE'), join(destination, 'LICENSE'))
    cpSync(join(work, 'licenses'), join(destination, 'licenses'), { recursive: true })
    console.log(`Staged PDFium ${entry.asset}`)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}
