# Five local TTS configurations

Measured 2026-10-30 on Apple M4, 16 GB, macOS 27.2. **All 50 recordings completed, contained finite audible PCM, had no clipped samples, and scored zero word errors in the local Whisper small.en screen on one shared 24-word passage.** This is not a listening study or a broad pronunciation test. No model is adopted as a quality winner.

Five interleaved rounds, one fresh process per model per round; each process performs a first inference and a warm inference. The table summarizes the five warm inferences. Startup means the first materialized PCM chunk, excluding physical playback. No claim of M1/macOS 14 certification.

| Configuration | First PCM, median [range] | Full synthesis, median | RTF | Peak process RSS | MLX peak allocation |
|---|---:|---:|---:|---:|---:|
| Phonon 31M · q8 CPU | 7.7 ms [6.7 ms–9.3 ms] | 239.5 ms | 0.032 | 525 MB | 0 MB |
| Sopro v2 Turbo · int8 CPU | 218.3 ms [204.8 ms–334.5 ms] | 1.007 s | 0.151 | 1287 MB | 0 MB |
| Qwen3 Ryan · 4-bit MLX | 306.2 ms [299.1 ms–521.5 ms] | 4.404 s | 0.463 | 2711 MB | 3021 MB |
| CosyVoice3 · 4-bit MLX port | 5.784 s [5.489 s–8.041 s] | 5.784 s | 0.812 | 2715 MB | 3349 MB |
| VoxCPM2 · 4-bit MLX | 15.780 s [15.427 s–62.917 s] | 15.814 s | 2.411 | 2718 MB | 5163 MB |

RTF = generation seconds / audio seconds; below 1 means faster than playback. RSS and MLX allocation overlap; **do not add them**. MLX allocation is included because process RSS alone understates the larger pipelines. Load time includes Python imports, materialized model parameters, and preparation of the reference when applicable. New process is not an uncached disk or full application cold-start measurement.

## What the measurements support

- Phonon is a compact, fast candidate in this configuration. Its prebuilt runtime targets macOS 15+, so the app’s macOS 14 baseline still requires a source-built runtime and validation.
- Sopro streams promptly but its assets, ready memory, and peak memory exceed the original mandatory-model resource limits. Its CPU int8 configuration has not been compared against a tuned MPS configuration.
- Qwen CustomVoice streams in 0.5-second output chunks. The first inference is slower than warm generation; load and first inference are retained separately. The selected checkpoint offers preset Ryan plus instructions; it is not the Base cloning or VoiceDesign checkpoint.
- CosyVoice and VoxCPM2 returned only one PCM chunk for this input in the tested adapters. Their startup includes the whole utterance generation. Do not transfer advertised streaming latency from another runtime to these results.
- VoxCPM2 retains a 62.9-second warm-start outlier. No slow trial was discarded. The current test does not establish its cause or a stable tail-latency guarantee.
- All larger candidates remain optional experimental local profiles. The original native default remains available; initial required downloads and release budgets have not been redefined.

## Native integration

All five profiles passed one production Reader → PCM conversion → real CPAL callback → stop smoke test. These smoke tests include concurrent development/profiling activity and are **behavior verification, not comparative performance results**. See `app-voice-9.jsonl` through `app-voice-13.jsonl`. Their RSS values cover the Rust host, not the model subprocess.

The bridge keeps selected text and audio in memory, disables Hub/transformer networking during inference, suppresses backend text logs, and validates registered asset/script hashes before loading. One model is loaded at a time. Esc/replace cancels the active run; if inference must be aborted, the subprocess is terminated and the selected model reloads in the background. This recovery can make the next reading cold. Only Phonon and Sopro currently meet the original component startup target in every measured warm trial; the user has allowed higher startup latency for quality candidates. No acoustic or M1 performance certification is implied.

Native playback converts all profiles to 24 kHz and uses pinned Apache-2.0 Sonic for pitch-preserving 0.75×–2× tempo. Model component timings above exclude the IPC/tempo/EQ/UI pipeline. Capability filters describe documented checkpoint capabilities; emotion/design/dialogue authoring controls are not exposed by these reading profiles.

## Reproduce locally

```sh
uv venv --python 3.12 .cache/advanced-tts-env
uv pip sync --python .cache/advanced-tts-env/bin/python docs/performance/five-models/requirements-main.txt
uv venv --python 3.12 .cache/cosyvoice-tts-env
uv pip sync --python .cache/cosyvoice-tts-env/bin/python docs/performance/five-models/requirements-cosyvoice.txt
python3 scripts/prepare_advanced_tts.py
# Clone the Cosy port at the exact commit recorded in five-model-runtime-notes.md.
# Restore the synthetic reference.wav matching reference.json and its retained SHA-256.
python3 scripts/register_advanced_tts.py
python3 scripts/benchmark_advanced_tts.py
.cache/voice-eval-env/bin/python scripts/check_advanced_tts_audio.py
python3 scripts/report_advanced_tts.py
```

The registration is local development configuration (`~/Library/Application Support/InfoTransfer/experimental-runtimes.json`). It points at these virtual environments and verified caches. The 45 MB development bundle does not include Python, heavy model weights, or the reference voice. Runtimes and packaging remain release work. Do not distribute the cache as a certified release: model-conversion/auxiliary asset notices and the macOS 14 runtime build still need release review.

## Evidence and sources

- `protocol.json`, `runs.jsonl`, `summary.json`, `asr.jsonl`, all 50 retained WAVs and backend logs. Generation and scoring consume actual PCM; WAV hashes are checked before ASR.
- `downloads.json` pins every staged model artifact, byte count and SHA-256; the requirement freezes identify the runtime environments.
- [Verified capability and setup notes](../../five-model-runtime-notes.md) contain primary-source links and conditional feature definitions.
- [Qwen3](https://github.com/QwenLM/Qwen3-TTS), [VoxCPM2](https://github.com/OpenBMB/VoxCPM), [CosyVoice3](https://github.com/QwenAudio/CosyVoice), [Sopro](https://github.com/samuel-vitorino/sopro), [Phonon](https://github.com/gradium-ai/xn-ptts).
- [Cosy MLX port](https://github.com/DePasqualeOrg/mlx-audio-plus/tree/4c9ec6a8489e790b5ba8964ab1f1d63150476f9f); [Sonic pinned source](https://github.com/waywardgeek/sonic/tree/b93885dcb70aae50c6f76b0fe4e0868f029a077e).

Naturalness ratings from the previous native chunk policy were invalidated instead of being attached to new audio. Audition these files directly; do not interpret one-passage zero WER as a naturalness score.
