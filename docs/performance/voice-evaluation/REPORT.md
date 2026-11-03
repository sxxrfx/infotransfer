# Local voice screening — 2026-10-30

All **21 voices** completed the same **12 passages at 1×**, plus two whole-passage controls: **294 recordings**, no generation/scoring failures. Runs used this M4 Mac. Ratings are **UTMOS22 predicted naturalness**, not a human listening score. ASR word error is a separate intelligibility proxy; lower is better. Neither measures dyslexia-specific reading comprehension.

Current app voices were exported through the production native frontend and 32/180-character chunk policy. The two newer models used their author Python frontends with the same outer chunks. This compares complete experimental pipelines, not identical frontends or native app adapters. The first 32-character boundary can cut prosodic context. Source-specific cleanup, punctuation handling, style selection and tail trimming remain part of each result.

The current app frontend has a concrete pronunciation defect: its lexicon path omits unsupported tokens. Names, accented words, acronyms and numbers dominate the app's error rate. For example, the native Heart transcript loses PDF/BBC/URL/CPU and the amounts/time from the number passage. Backend logs corroborate unknown-token handling. These scores rate the current app pipeline, not the best possible quality of the underlying Kokoro/Kitten voices. Fix the frontend and rerun before judging those model families or promoting a replacement.

| Voice | Pipeline | Predicted naturalness, median (IQR) | ASR word error |
| --- | --- | ---: | ---: |
| af_heart | Paradee 1.2 · experimental | 4.42 (4.39–4.46) | 0.4% |
| am_puck | Paradee 1.2 · experimental | 4.31 (4.25–4.35) | 0.7% |
| bm_fable | Paradee 1.2 · experimental | 4.21 (4.06–4.25) | 0.0% |
| Hugo | Kitten 0.8 · experimental | 4.20 (4.01–4.36) | 0.7% |
| Jasper | Kitten 0.8 · experimental | 4.12 (3.93–4.35) | 1.4% |
| Bruno | Kitten 0.8 · experimental | 4.01 (3.90–4.20) | 1.4% |
| Luna | Kitten 0.8 · experimental | 3.90 (3.85–4.10) | 0.7% |
| bf_isabella | Paradee 1.2 · experimental | 3.90 (3.80–4.07) | 0.4% |
| Heart | Kokoro Heart · app | 3.76 (3.65–3.85) | 11.7% |
| Leo | Kitten 0.8 · experimental | 3.69 (3.60–3.81) | 0.7% |
| Rosie | Kitten 0.8 · experimental | 3.64 (3.59–3.80) | 0.7% |
| Bella | Kitten 0.8 · experimental | 3.60 (3.53–3.69) | 0.7% |
| Arthur | Kitten 0.1 · app | 3.54 (3.31–3.65) | 17.8% |
| Elliot | Kitten 0.1 · app | 3.50 (3.36–3.55) | 18.9% |
| Maya | Kitten 0.1 · app | 3.48 (3.17–3.62) | 23.5% |
| Theo | Kitten 0.1 · app | 3.35 (3.15–3.39) | 17.8% |
| Nora | Kitten 0.1 · app | 3.25 (3.10–3.46) | 17.4% |
| Owen | Kitten 0.1 · app | 3.20 (3.14–3.33) | 17.1% |
| Clara | Kitten 0.1 · app | 3.10 (2.93–3.21) | 16.7% |
| Ruby | Kitten 0.1 · app | 2.71 (2.61–2.78) | 18.5% |
| Kiki | Kitten 0.8 · experimental | 2.56 (2.37–2.81) | 0.7% |

The predictor's nominal scale is 1–5; values are not percentages. IQR describes variation across passages, not confidence from human listeners. Small differences and overlapping ranges do not establish a preference. No subjective listening or Heart speaker-similarity verdict is claimed. Paradee's `af_heart` is a distilled Heart-derived preset, distinct from authentic Kokoro Heart.

## What the error rate means

Whisper small.en (faster-whisper, local CPU int8, beam 5, temperature zero, no VAD, no previous-text conditioning) transcribed the saved 24 kHz recordings after conversion to 16 kHz PCM. A pinned official Whisper English normalizer is applied to both the declared spoken reference and transcript. PDF/BBC/URL/CPU spellings are joined, digit clock times are joined, numeric ordinal suffixes are removed, and diacritics are ignored. The literal text, spoken reference, transcripts, substitutions, deletions and insertions are retained in `results.json`. Recognition and proper-name spelling errors can inflate WER; it is not a direct count of mispronunciations. Homograph pronunciation is not established by a correct transcript.

Missing, empty, nonfinite and silent recordings are failures rather than omitted observations. Clipped samples, duration and leading/trailing signal threshold crossings are retained per passage. No loudness normalization, silence trimming or selective passage removal was applied during scoring. Waveforms were saved as 24 kHz PCM16 for every pipeline.

## Downloads, runtime and adoption

The new models are downloaded under `.cache/voice-eval`: Kitten nano 0.8 int8 (27,649,561 bytes for source graph/voices/config) and Paradee 1.2 int8 (8,463,946 bytes for graph/config). Those sums exclude frontend, licenses, Python environment and evaluation tools. The full experimental environment is not shipped in the app. Hashes, pinned revisions and exact downloaded byte counts are in `results.json`.

Experimental generation used ONNX Runtime 1.24.4 CPU, two intra-op threads and one inter-op thread. Existing voices used the app's sherpa 2.0.1 / ORT 1.28.2 runtime. Candidate `generated-*.json` files retain diagnostic CPU generation times and Python-process high-water RSS, but those runs overlapped scoring/build activity. They are not uncontended app-startup benchmarks, ready-state memory measurements or M1 certification. Do not compare them directly to the app's request-to-render latency bars.

The shipping catalog and default engine have not been replaced. A candidate still needs a native frontend/runtime adapter, license/notices review of the complete distribution, app onset/cancellation tests, whole-app resource checks, and M1/8 GB/macOS 14 validation. Authentic Heart's existing M4 startup p95 remains about 1.07 seconds, exceeding the 500 ms warm target.

## Reproduce

Use the isolated environment pinned in `requirements.txt`. It is a development evaluation dependency set, not an app dependency. Install the spaCy English model at the URL in that file. Fetch SpeechMOS v1.2.0 at commit `ed25eacbfa42b99156c36ebec67a733b5dbb9b79` into `.cache/voice-eval/SpeechMOS`, and retain its MIT license.

```sh
python3 scripts/prepare_voice_eval.py
cargo build --release --no-default-features --bin voice-export
.cache/voice-eval-env/bin/python scripts/evaluate_voices.py generate
.cache/voice-eval-env/bin/python scripts/evaluate_voices.py score
.cache/voice-eval-env/bin/python scripts/evaluate_voices.py summarize
python3 scripts/report_voice_evaluation.py
```

The generation and scoring stages resume completed recordings. To obtain a new stochastic run, move the evaluation cache to a separate directory first. ONNX model randomness was left as published; scoring seeds are fixed. A normalizer API mismatch was caught in an initial harness attempt; those invalid rows are archived separately and absent from the complete results. Final WER is recomputed uniformly from all retained transcripts.

Fresh runs capture stage inputs before execution. Resume checks the corpus, evaluator code, package versions, model/scorer artifacts and retained WAV hashes; changed inputs are rejected rather than attached to old scores. This original run predates those guards. Its provenance is explicitly marked `post_run_verified_import`: an audit checked the published corpus/binary/checkpoint hashes, downloaded artifacts, all retained waveforms, transcripts and scores. It freezes that evidence without claiming to reconstruct missing execution-time code fingerprints. The frozen corpus, including literal inputs and spoken references, is embedded in `results.json`.

[UTMOS wrapper and metric description](https://github.com/tarepan/SpeechMOS/tree/v1.2.0), [Kitten reference frontend](https://github.com/KittenML/KittenTTS/blob/0.8.1/kittentts/onnx_model.py), [Paradee reference frontend](https://github.com/sahilmahendrakar/paradee/blob/v1.2/paradee/tts.py). Candidate licenses and integration details are in [the model research](../../current-tts-models.md).

## Auditions

The same opening prose is preserved below for every voice. These files are generated evaluation samples, not evidence of a listening study.

- [af_heart — Paradee 1.2 · experimental](audio/paradee12-af_heart-prose.wav)
- [am_puck — Paradee 1.2 · experimental](audio/paradee12-am_puck-prose.wav)
- [bm_fable — Paradee 1.2 · experimental](audio/paradee12-bm_fable-prose.wav)
- [Hugo — Kitten 0.8 · experimental](audio/kitten08-Hugo-prose.wav)
- [Jasper — Kitten 0.8 · experimental](audio/kitten08-Jasper-prose.wav)
- [Bruno — Kitten 0.8 · experimental](audio/kitten08-Bruno-prose.wav)
- [Luna — Kitten 0.8 · experimental](audio/kitten08-Luna-prose.wav)
- [bf_isabella — Paradee 1.2 · experimental](audio/paradee12-bf_isabella-prose.wav)
- [Heart — Kokoro Heart · app](audio/native-8-prose.wav)
- [Leo — Kitten 0.8 · experimental](audio/kitten08-Leo-prose.wav)
- [Rosie — Kitten 0.8 · experimental](audio/kitten08-Rosie-prose.wav)
- [Bella — Kitten 0.8 · experimental](audio/kitten08-Bella-prose.wav)
- [Arthur — Kitten 0.1 · app](audio/native-0-prose.wav)
- [Elliot — Kitten 0.1 · app](audio/native-2-prose.wav)
- [Maya — Kitten 0.1 · app](audio/native-3-prose.wav)
- [Theo — Kitten 0.1 · app](audio/native-4-prose.wav)
- [Nora — Kitten 0.1 · app](audio/native-5-prose.wav)
- [Owen — Kitten 0.1 · app](audio/native-6-prose.wav)
- [Clara — Kitten 0.1 · app](audio/native-1-prose.wav)
- [Ruby — Kitten 0.1 · app](audio/native-7-prose.wav)
- [Kiki — Kitten 0.8 · experimental](audio/kitten08-Kiki-prose.wav)
