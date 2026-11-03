#!/usr/bin/env python3
"""Independent ASR screen of retained PCM, not a human naturalness score."""
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/performance/five-models'
sys.path.insert(0, str(ROOT / '.cache/voice-eval'))
from faster_whisper import WhisperModel
from whisper_normalizers.english import EnglishTextNormalizer
import numpy as np
import soundfile as sf
from scipy.signal import resample_poly
from math import gcd

import importlib.metadata
provenance = {'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'generation_results_sha256': hashlib.sha256((OUT / 'runs.jsonl').read_bytes()).hexdigest(), 'packages': {name: importlib.metadata.version(name) for name in ['faster-whisper','ctranslate2','numpy','scipy','soundfile']}, 'artifacts': {}}
for path in list((ROOT / '.cache/voice-eval/asr').rglob('*')) + list((ROOT / '.cache/voice-eval/whisper_normalizers').glob('*')):
    if path.is_file():
        provenance['artifacts'][str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
(OUT / 'asr-provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
normal = EnglishTextNormalizer()
asr = WhisperModel(str(ROOT / '.cache/voice-eval/asr'), device='cpu', compute_type='int8', cpu_threads=2, num_workers=1)

def words(text):
    return re.findall(r"[a-z0-9]+", normal(text))

def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        new = [i]
        for j, y in enumerate(b, 1):
            new.append(min(new[-1] + 1, row[j] + 1, row[j-1] + (x != y)))
        row = new
    return row[-1]

rows = [json.loads(line) for line in (OUT / 'runs.jsonl').read_text().splitlines()]
with (OUT / 'asr.jsonl').open('w') as output:
    for row in rows:
        if row['event'] != 'recording': continue
        path = ROOT / row['wav']
        assert hashlib.sha256(path.read_bytes()).hexdigest() == row['wav_sha256']
        audio, rate = sf.read(path, dtype='float32')
        assert np.isfinite(audio).all() and np.max(np.abs(audio)) > 1e-5
        div = gcd(rate, 16000)
        audio = resample_poly(audio, 16000 // div, rate // div).astype(np.float32)
        segments, _ = asr.transcribe(audio, language='en', beam_size=5, temperature=0, condition_on_previous_text=False, vad_filter=False)
        hypothesis = ' '.join(segment.text.strip() for segment in segments)
        reference = words(row['text'])
        errors = distance(reference, words(hypothesis))
        result = {'model': row['model'], 'round': row['round'], 'trial': row['trial'], 'wav': row['wav'], 'wav_sha256': row['wav_sha256'], 'hypothesis': hypothesis, 'reference': row['text'], 'word_errors': errors, 'reference_words': len(reference), 'wer': errors/len(reference)}
        output.write(json.dumps(result) + '\n');output.flush()
        print(json.dumps({'model': row['model'], 'round': row['round'], 'trial': row['trial'], 'wer': result['wer'], 'hypothesis': hypothesis}), flush=True)
