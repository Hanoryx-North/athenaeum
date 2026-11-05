import path from 'node:path';
import type { IntegrationConfig } from '../client/config.ts';

/** Stdio launch stanza for any MCP host that accepts a command and argument vector. */
export function mcpLaunch(config: Pick<IntegrationConfig, 'command' | 'repositories' | 'allow' | 'caller' | 'session' | 'capture'>, bin = 'athenaeum-mcp', configFile?: string) {
  return {
    command: bin,
    args: configFile
      ? ['--config', path.resolve(configFile), ...(config.session ? ['--session', config.session] : [])]
      : config.repositories.flatMap(repo => ['--repo', `${repo.name}=${repo.path}`]).concat(['--athenaeum', config.command, '--caller', config.caller, ...(config.capture === 'off' ? ['--capture', 'off'] : []), ...(config.session ? ['--session', config.session] : []), ...(config.allow.size ? ['--allow', [...config.allow].join(',')] : [])]),
  };
}
