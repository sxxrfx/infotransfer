#!/usr/bin/env python3
"""Five interleaved rounds, serial processes, retained errors and exact provenance."""
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/performance/five-models'
MODELS = ['phonon', 'qwen3', 'sopro', 'voxcpm2', 'cosyvoice3']
OUT.mkdir(parents=True, exist_ok=True)
claim = {'claim': 'Model-component startup, synthesis time, and process RSS for five explicitly configured local pipelines on this M4; not intrinsic quality, physical playback, selection latency, or M1 certification.', 'text': 'Reading at your own pace can make a difficult paragraph easier to follow. Take a breath, then continue when you are ready.', 'rounds': 5, 'runs_per_process': 'one first inference then one warm inference', 'timers': 'after Python process start; load includes imports and reference preparation; generation consumes the iterator and materializes PCM', 'rtf_definition': 'generation seconds / audio seconds; lower is faster', 'memory': 'Darwin ru_maxrss bytes, includes process host memory; MLX memory tracked separately and overlaps RSS; never add them', 'cold': 'unloaded new process with normal filesystem cache; not a reboot or uncached disk', 'hardware': platform.platform(), 'started_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'), 'load_average': subprocess.check_output(['uptime'], text=True).strip()}
for file in ['scripts/advanced_tts_worker.py', '.cache/advanced-tts/downloads.json', '.cache/advanced-tts/requirements-main.txt', '.cache/advanced-tts/requirements-cosyvoice.txt', '.cache/advanced-tts/reference.json', '.cache/advanced-tts/reference.wav']:
    claim.setdefault('provenance', {})[file] = hashlib.sha256((ROOT / file).read_bytes()).hexdigest()
(OUT / 'protocol.json').write_text(json.dumps(claim, indent=2) + '\n')
with (OUT / 'runs.jsonl').open('w') as results:
    for round_id in range(5):
        order = MODELS[round_id:] + MODELS[:round_id]
        for model in order:
            env = '.cache/cosyvoice-tts-env' if model == 'cosyvoice3' else '.cache/advanced-tts-env'
            command = [str(ROOT / env / 'bin/python'), '-u', str(ROOT / 'scripts/advanced_tts_worker.py'), '--model', model, '--round', str(round_id + 1), '--output', str(OUT)]
            start = time.perf_counter()
            with (OUT / f'{model}-r{round_id+1}.log').open('w') as log:
                try:
                    proc = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=log, text=True, timeout=240)
                    for line in proc.stdout.splitlines():
                        row = json.loads(line)
                        row['suite_provenance'] = claim['provenance']
                        results.write(json.dumps(row) + '\n')
                    if proc.returncode:
                        results.write(json.dumps({'event': 'process_failure', 'model': model, 'round': round_id + 1, 'returncode': proc.returncode, 'success': False}) + '\n')
                    print(json.dumps({'model': model, 'round': round_id + 1, 'returncode': proc.returncode, 'wall_s': time.perf_counter()-start}), flush=True)
                except subprocess.TimeoutExpired:
                    results.write(json.dumps({'event': 'timeout', 'model': model, 'round': round_id + 1, 'success': False, 'limit_s': 240}) + '\n')
            results.flush()
