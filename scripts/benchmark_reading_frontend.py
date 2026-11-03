#!/usr/bin/env python3
"""One-host absolute timing check. No acoustic, M1 or before/after claims."""
import datetime
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / 'docs/performance/developer-reading'


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--device', required=True, help='Exact CPAL output name; pin the same device across voices')
    args = parser.parse_args()
    if subprocess.run(['pgrep', '-f', r'/InfoTransfer\.app/Contents/MacOS/infotransfer($| )'], capture_output=True).returncode == 0:
        raise RuntimeError('Quit InfoTransfer before timing the reader')
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT))
    binary = Path(metadata['target_directory']) / 'release/reading-bench'
    run = BASE / ('timing-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:6])
    run.mkdir(parents=True)
    (run / 'runner.py').write_bytes(Path(__file__).read_bytes())
    provenance = {
        'claim': 'Absolute release-build timing on this host for nine native voices, not a speedup or certification',
        'binary_sha256': digest(binary), 'script_sha256': digest(__file__),
        'output_device': args.device,
        'host': {'os': platform.platform(), 'cpu': subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip(),
                 'cores': os.cpu_count(), 'memory_bytes': int(subprocess.check_output(['sysctl', '-n', 'hw.memsize'])),
                 'load_average': os.getloadavg()},
        'method': 'One first inference plus 20 warm trials per voice; nearest-rank p95; technical passage; CPAL signal threshold -50 dBFS; 25 ms playback then stop',
        'includes': 'validation, normalization, pronunciation, synthesis, PCM buffering, resampling and real audio callback',
        'excludes': 'selection capture, global shortcut, GUI and physical/acoustic output latency',
        'source_sha256': {name: digest(ROOT / name) for name in ['src/bin/reading-bench.rs', 'src/reader.rs', 'src/audio.rs', 'src/phonemes.rs', 'src/spoken.rs', 'src/speech.rs', 'src/dictionary.rs', 'src/sound.rs', 'src/voices.rs']},
        'phonemizer_manifest_sha256': digest(ROOT / 'resources/phonemizer/PROVENANCE.json'),
    }
    (run / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    summaries = []
    for voice in range(9):
        command = [str(binary), '--catalog', '--engine', 'kitten-cpu', '--voice', str(voice), '--runs', '21', '--bucket', 'technical', '--play-ms', '25', '--device', args.device]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=180)
        output = run / f'voice-{voice}.jsonl'; output.write_text(result.stdout)
        (run / f'voice-{voice}.stderr').write_text(result.stderr)
        rows = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        configuration = next((r for r in rows if r.get('event') == 'configuration'), {})
        trials = [r for r in rows if r.get('event') == 'trial']
        warm = [r for r in trials if r.get('phase') == 'warm' and r.get('success')]
        failures = sum(not r.get('success') for r in trials)
        complete = result.returncode == 0 and len(trials) == 21 and len(warm) == 20 and not failures and configuration.get('device') == args.device
        summary = {'voice_id': voice, 'complete': complete, 'failures': failures, 'trial_count': len(trials),
                   'warm_count': len(warm), 'configuration': configuration, 'command': command, 'output_sha256': digest(output)}
        if complete:
            for field in ['first_pcm_ms', 'first_signal_playback_estimate_ms', 'stop_silence_estimate_ms']:
                values = sorted(r[field] for r in warm)
                summary[field] = {'p50': (values[9] + values[10]) / 2, 'p95': values[math.ceil(len(values) * .95) - 1], 'min': values[0], 'max': values[-1]}
            summary['peak_resident_bytes'] = max(r['peak_resident_bytes'] for r in trials)
        summaries.append(summary)
        (run / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
        print(json.dumps({'voice': voice, 'complete': complete, 'warm': len(warm), 'startup': summary.get('first_signal_playback_estimate_ms')}), flush=True)
    (run / 'environment-after.json').write_text(json.dumps({'load_average': os.getloadavg()}, indent=2) + '\n')
    (BASE / 'LATEST-TIMING.json').write_text(json.dumps({'run': str(run.relative_to(ROOT))}) + '\n')
    print(json.dumps({'run': str(run)}), flush=True)
    if not all(row['complete'] for row in summaries):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
