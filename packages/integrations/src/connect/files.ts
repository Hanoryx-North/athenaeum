import fs from 'node:fs';
import path from 'node:path';

export class UnreadableConfig extends Error {
  constructor(readonly file: string, reason: string) {
    super(`${file}: ${reason}. Nothing was changed; fix or remove the file and run this again.`);
  }
}

/**
 * Read a JSON object, or `{}` when the file does not exist. A file that exists but is not a JSON
 * object is refused rather than replaced: these are the user's own agent settings, and a parse
 * error is theirs to look at, not ours to overwrite.
 */
export function readJsonObject(file: string): Record<string, any> {
  let text: string;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (error: any) {
    if (error?.code === 'ENOENT') return {};
    throw new UnreadableConfig(file, `could not be read (${error?.message ?? error})`);
  }
  if (!text.trim()) return {};
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error: any) {
    throw new UnreadableConfig(file, `is not valid JSON (${error?.message ?? error})`);
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new UnreadableConfig(file, 'is not a JSON object');
  return value as Record<string, any>;
}

/** Write via a temporary file in the same directory so a crash never leaves half a config. */
export function writeAtomic(file: string, text: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.athenaeum-${process.pid}.tmp`;
  try {
    const mode = fs.existsSync(file) ? fs.statSync(file).mode & 0o777 : 0o600;
    fs.writeFileSync(temp, text, { mode });
    fs.renameSync(temp, file);
  } catch (error) {
    fs.rmSync(temp, { force: true });
    throw error;
  }
}

/** Keep the first version of a file we are about to edit, once, so the original is never lost. */
export function backupOnce(file: string): void {
  const backup = `${file}.athenaeum-backup`;
  if (fs.existsSync(file) && !fs.existsSync(backup)) fs.copyFileSync(file, backup);
}

export function writeJson(file: string, value: unknown): void {
  writeAtomic(file, `${JSON.stringify(value, null, 2)}\n`);
}

/** Replace (or add, or with `body === null` remove) the text between two marker lines. */
export function setManagedBlock(text: string, begin: string, end: string, body: string | null): string {
  const start = text.indexOf(begin);
  const stop = start === -1 ? -1 : text.indexOf(end, start);
  const found = start !== -1 && stop !== -1;
  if (body === null) {
    if (!found) return text;
    const joined = [text.slice(0, start).replace(/\s+$/, ''), text.slice(stop + end.length).replace(/^\s+/, '')].filter(Boolean).join('\n\n');
    return joined ? `${joined}\n` : '';
  }
  const block = `${begin}\n${body.replace(/\n+$/, '')}\n${end}`;
  if (found) return text.slice(0, start) + block + text.slice(stop + end.length);
  const base = text.replace(/\s+$/, '');
  return base ? `${base}\n\n${block}\n` : `${block}\n`;
}

export function hasManagedBlock(text: string, begin: string, end: string): boolean {
  const start = text.indexOf(begin);
  return start !== -1 && text.indexOf(end, start) !== -1;
}

/**
 * After removing our entries, delete a file we created ourselves (it is now empty and we never
 * took a backup of an earlier version). A file that existed before is left in place, however empty.
 */
export function removeIfOurs(file: string): void {
  try {
    const text = fs.readFileSync(file, 'utf8').trim();
    if ((text === '' || text === '{}') && !fs.existsSync(`${file}.athenaeum-backup`)) fs.rmSync(file);
  } catch {
    /* already gone */
  }
}
