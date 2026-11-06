#!/usr/bin/env python3
"""Check syntax and wiring of the owned runtime embedded in the platform launcher."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent
launcher = (root / 'native-cli/src/main.rs').read_text(encoding='utf-8')
modules = sorted(p for p in (root / 'src').glob('*.mjs') if not p.name.endswith('.test.mjs'))
if not modules:
    raise SystemExit('No runtime modules found')
for module in modules:
    if f'../../src/{module.name}' not in launcher:
        raise SystemExit(f'Runtime module is not embedded in the launcher: {module.name}')
    subprocess.run(['node', '--check', str(module)], check=True)
print(f'Verified syntax and launcher wiring for {len(modules)} owned runtime modules')
