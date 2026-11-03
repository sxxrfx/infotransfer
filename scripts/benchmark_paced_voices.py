#!/usr/bin/env python3
"""Refresh startup indicators for the changed native sentence policy."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'docs/performance/pacing'
EXE=Path('/Users/sxxrfx/~/.cargo/target/release/reading-bench')
source={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'src/reader.rs',ROOT/'src/text.rs',ROOT/'src/sound.rs',ROOT/'native/speech.c',ROOT/'src/bin/reading-bench.rs']}
(OUT/'startup-protocol.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(EXE.read_bytes()).hexdigest(),'source':source,'rounds':5,'runs_per_round':5,'warm_per_voice':20,'claim':'Request to actual CPAL callback with the current native sentence policy on M4 at 1x. Excludes selection, UI, and physical output. No concurrent model evaluation is intended.','load_average':subprocess.check_output(['uptime'],text=True).strip()},indent=2)+'\n')
with (OUT/'startup.jsonl').open('w') as output:
 for round_id in range(5):
  for offset in range(9):
   voice=(offset+round_id)%9
   result=subprocess.run([str(EXE),'--catalog','--engine','kitten-cpu','--voice',str(voice),'--runs','5'],cwd=ROOT,capture_output=True,text=True,timeout=60)
   (OUT/f'startup-r{round_id}-v{voice}.log').write_text(result.stderr)
   for line in result.stdout.splitlines():
    row=json.loads(line);row.update(voice=voice,round=round_id);output.write(json.dumps(row)+'\n')
   if result.returncode: output.write(json.dumps({'event':'process_failure','voice':voice,'round':round_id,'returncode':result.returncode,'stderr':result.stderr})+'\n')
   output.flush();print(json.dumps({'voice':voice,'round':round_id,'returncode':result.returncode}),flush=True)
