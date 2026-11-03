#!/usr/bin/env python3
"""Production speech exports + independent local ASR. Correctness QA, not a latency benchmark."""
import argparse
import datetime
import hashlib
import importlib.metadata
import json
from pathlib import Path
import re
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / 'docs/performance/developer-reading'
CORPUS = [
    {'id': 'references', 'text': 'Read PR #5, API v1.2.3, and CPU at 90%.'},
    {'id': 'numbers', 'text': '$1,234.50, 0xFF, 1.2e-3, 16GB and getUserName(user_id).'},
    {'id': 'terms', 'text': 'Kubernetes, OpenTelemetry and Codex use Rust and JSON.'},
    {'id': 'contractions', 'text': "I'll check main.rs:42. I'am ready, and we're reading PR #5."},
]
DICTIONARY = [
    {'word': 'PR', 'replacement': 'pull request', 'match_case': False},
    {'word': 'Codex', 'replacement': 'coding assistant', 'match_case': False},
]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def generate(voices):
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT))
    binary = Path(metadata['target_directory']) / 'release/voice-export'
    run = BASE / ('run-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:6])
    run.mkdir(parents=True)
    (run / 'corpus.json').write_text(json.dumps(CORPUS, indent=2) + '\n')
    (run / 'dictionary.json').write_text(json.dumps(DICTIONARY, indent=2) + '\n')
    provenance = {'kind': 'correctness QA; concurrent development means timings are not comparative performance results',
        'binary_sha256': digest(binary), 'script_sha256': digest(__file__), 'corpus_sha256': digest(run / 'corpus.json'),
        'dictionary_sha256': digest(run / 'dictionary.json'), 'voices': voices,
        'phonemizer_manifest_sha256': digest(ROOT / 'resources/phonemizer/PROVENANCE.json'),
        'phonemizer_executable_sha256': digest(ROOT / 'resources/phonemizer/phonemize'),
        'source_sha256': {name: digest(ROOT / name) for name in ['src/speech.rs', 'src/phonemes.rs', 'src/spoken.rs', 'src/dictionary.rs', 'src/text.rs', 'src/sound.rs', 'src/voices.rs', 'src/advanced.rs', 'native/speech.c', 'native/exceptions.cpp']}}
    (run / 'generation-provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    (run / 'phonemizer-manifest.json').write_bytes((ROOT / 'resources/phonemizer/PROVENANCE.json').read_bytes())
    with (run / 'recordings.jsonl').open('w') as records:
        for voice in voices:
            command = [str(binary), '--voice-id', str(voice), '--corpus', str(run / 'corpus.json'),
                '--dictionary', str(run / 'dictionary.json'), '--output', str(run / 'audio'), '--prefix', f'voice-{voice}']
            try:
                result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=360)
                (run / f'voice-{voice}.stdout').write_text(result.stdout)
                (run / f'voice-{voice}.stderr').write_text(result.stderr)
                rows = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
                name = next((r.get('voice_name') for r in rows if r.get('event') == 'loaded'), str(voice))
                recordings = [r for r in rows if r.get('event') == 'recording']
                if result.returncode or len(recordings) != len(CORPUS):
                    raise RuntimeError('Process failed or incomplete recordings')
                for row in recordings:
                    row.update(voice_id=voice, voice_name=name)
                    if row['success']:
                        row['wav_sha256'] = digest(row['wav'])
                        row['wav'] = str(Path(row['wav']).relative_to(run))
                    records.write(json.dumps(row) + '\n'); records.flush()
                print(json.dumps({'voice': voice, 'name': name, 'recordings': len(recordings), 'failures': sum(not r['success'] for r in recordings)}), flush=True)
            except (subprocess.TimeoutExpired, RuntimeError) as error:
                records.write(json.dumps({'voice_id': voice, 'success': False, 'error': str(error)}) + '\n'); records.flush()
                print(json.dumps({'voice': voice, 'error': str(error)}), flush=True)
    (BASE / 'LATEST.json').write_text(json.dumps({'run': str(run.relative_to(ROOT))}) + '\n')
    return run


def transcribe(run):
    import numpy as np
    import soundfile as sf
    from scipy.signal import resample_poly
    from faster_whisper import WhisperModel
    sys.path.insert(0, str(ROOT / '.cache/voice-eval'))
    from whisper_normalizers.english import EnglishTextNormalizer
    normal = EnglishTextNormalizer()
    asr = ROOT / '.cache/voice-eval/asr'
    provenance = {'script_sha256': digest(__file__), 'recordings_sha256': digest(run / 'recordings.jsonl'),
        'packages': {name: importlib.metadata.version(name) for name in ['faster-whisper', 'ctranslate2', 'numpy', 'soundfile', 'scipy']},
        'artifacts': {str(p.relative_to(ROOT)): digest(p) for p in sorted(asr.iterdir()) if p.is_file()}}
    (run / 'asr-provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    model = WhisperModel(str(asr), device='cpu', compute_type='int8', cpu_threads=2, num_workers=1)
    with (run / 'transcripts.jsonl').open('w') as output:
        for row in map(json.loads, (run / 'recordings.jsonl').read_text().splitlines()):
            if not row['success']: continue
            path = run / row['wav']; assert digest(path) == row['wav_sha256']
            samples, rate = sf.read(path, dtype='float32')
            assert rate == 24000 and np.isfinite(samples).all() and np.max(np.abs(samples)) > 1e-5
            audio = resample_poly(samples, 2, 3).astype(np.float32)
            segments, _ = model.transcribe(audio, language='en', beam_size=5, temperature=0,
                condition_on_previous_text=False, vad_filter=False)
            transcript = ' '.join(segment.text.strip() for segment in segments)
            expected = ' '.join(row['spoken'])
            result = {'voice_id': row['voice_id'], 'voice_name': row['voice_name'], 'passage': row['passage'],
                'input': row['text'], 'spoken': expected, 'transcript': transcript, 'wav': row['wav'],
                'wav_sha256': row['wav_sha256'], 'reference_normalized': normal(expected), 'transcript_normalized': normal(transcript)}
            output.write(json.dumps(result) + '\n'); output.flush()
            print(json.dumps({'voice': row['voice_name'], 'passage': row['passage'], 'transcript': transcript}), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--generate', action='store_true')
    parser.add_argument('--asr', action='store_true')
    parser.add_argument('--voices', default=','.join(map(str, range(14))))
    args = parser.parse_args()
    run = generate(list(map(int, args.voices.split(',')))) if args.generate else ROOT / json.loads((BASE / 'LATEST.json').read_text())['run']
    if args.asr: transcribe(run)
    print(json.dumps({'run': str(run)}))


if __name__ == '__main__':
    main()
