/**
 * Quote one argument for the shell an agent runs a hook command through. Claude Code hands the
 * command string to `sh` on macOS and Linux and to a Windows shell elsewhere; double quotes work
 * for both of the Windows ones, single quotes are the only fully literal form on POSIX.
 */
export function quote(value: string, platform: NodeJS.Platform = process.platform): string {
  if (platform === 'win32') {
    if (/["%\r\n]/.test(value)) throw new Error(`cannot safely quote ${JSON.stringify(value)} for a Windows shell`);
    return `"${value}"`;
  }
  return `'${value.replaceAll("'", "'\\''")}'`;
}

export function commandLine(parts: readonly string[], platform: NodeJS.Platform = process.platform): string {
  return parts.map((part) => (/^[A-Za-z0-9_@%+=:,./-]+$/.test(part) && platform !== 'win32' ? part : quote(part, platform))).join(' ');
}
