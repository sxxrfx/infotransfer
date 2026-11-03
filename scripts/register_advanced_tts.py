#!/usr/bin/env python3
"""Register the exact locally evaluated runtimes for this Mac; never downloads assets."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / '.cache/advanced-tts'
manifest = json.loads((CACHE / 'downloads.json').read_text())
entries = {}
for key in ['qwen3', 'voxcpm2', 'cosyvoice3', 'sopro', 'phonon']:
    artifacts = []
    required = [key] + (['s3tokenizer'] if key == 'cosyvoice3' else [])
    for artifact in manifest['artifacts']:
        if any(artifact['path'].startswith('.cache/advanced-tts/' + folder + '/') for folder in required):
            artifacts.append({'path': str(ROOT / artifact['path']), 'sha256': artifact['sha256']})
    for name in ['scripts/advanced_tts_server.py', 'scripts/advanced_tts_worker.py'] + (['.cache/advanced-tts/reference.wav', '.cache/advanced-tts/reference.json'] if key in ['voxcpm2', 'cosyvoice3', 'sopro'] else []):
        path = ROOT / name
        artifacts.append({'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    if key == 'cosyvoice3':
        for path in (CACHE / 'code/mlx-audio-plus/mlx_audio').rglob('*.py'):
            artifacts.append({'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    env = '.cache/cosyvoice-tts-env' if key == 'cosyvoice3' else '.cache/advanced-tts-env'
    entries[key] = {'python': str(ROOT / env / 'bin/python'), 'script': str(ROOT / 'scripts/advanced_tts_server.py'), 'model_dir': str(CACHE / key), 'artifacts': artifacts}
folder = Path.home() / 'Library/Application Support/InfoTransfer'
folder.mkdir(parents=True, exist_ok=True)
path = folder / 'experimental-runtimes.json'
with path.open('w') as out:
    os.chmod(path, 0o600)
    out.write(json.dumps(entries, indent=2) + '\n')
print(json.dumps({'registered': list(entries), 'config': str(path)}))
