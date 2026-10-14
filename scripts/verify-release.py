import json
import os
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parent.parent
tag = sys.argv[1]
if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?', tag):
    raise SystemExit('Invalid release tag')
version = tag[1:]
for name in ['src-tauri/Cargo.toml', 'game-host/Cargo.toml', 'dependency-helper/Cargo.toml']:
    match = re.search(r'^version\s*=\s*"([^"]+)"', (root / name).read_text(), re.M)
    if not match or match[1] != version:
        raise SystemExit(f'{name} does not match {tag}')
for name in ['src-tauri/tauri.conf.json', 'package.json', 'package-lock.json']:
    if json.loads((root / name).read_text())['version'] != version:
        raise SystemExit(f'{name} does not match {tag}')
if json.loads((root / 'package-lock.json').read_text())['packages']['']['version'] != version:
    raise SystemExit('npm root package version does not match release tag')
notes = f'docs/releases/{version}.md'
if not (root / notes).is_file():
    raise SystemExit(f'Missing release notes: {notes}')
print(f'Validated {tag}')
if output := os.environ.get('GITHUB_OUTPUT'):
    with open(output, 'a') as stream:
        stream.write(f'version={version}\nprerelease={str("-" in version).lower()}\nnotes={notes}\n')
