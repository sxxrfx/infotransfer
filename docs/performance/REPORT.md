# Performance report

M4 software playback measurements are below the proposed startup budgets at p95. The approved M1 end-to-end acceptance gate remains open. The initial matrix below covers Arthur. A later matched nine-voice run, including authentic Kokoro Heart, is recorded separately below.

## Current playback results

Apple M4, 16 GB RAM, macOS 27.2, built-in MacBook Air Speakers. Release build, optimized Kitten nano v0.1 CPU graph, two inference threads, voice 0 (Arthur), 1× speed, first chunk up to 32 characters and following chunks up to 180 characters.

The timer starts when the reading request enters the Rust reader. It ends when the real audio callback renders a sample at or above −50 dBFS. It includes validation, the controller, inference, the PCM queue and resampling. It excludes selection capture, GPUI and physical output latency.

| Scenario | Summarized trials | p50 | p95 | Worst |
| --- | ---: | ---: | ---: | ---: |
| Short selection | 100 | 74.0 ms | 74.6 ms | 95.5 ms |
| Typical paragraph | 100 | 136.6 ms | 169.3 ms | 188.5 ms |
| 100,000 characters | 100 | 143.0 ms | 182.5 ms | 196.3 ms |
| Unpunctuated text | 100 | 150.7 ms | 188.9 ms | 206.4 ms |
| Replacement under load | 100 | 215.6 ms | 293.7 ms | 1015.6 ms |
| Unloaded model | 20 | 761.1 ms | 807.7 ms | 830.6 ms |

There were **0 recorded failures across 545 raw trials**. The table summarizes 520 trials; 25 first-inference warmup trials are retained in the raw files and excluded from the warm summaries. Percentiles use the nearest-rank method. Each warm scenario has five process rounds; cold uses twenty separate processes.

Replacement under load overlapped a render-check rebuild. Its 1,015.6 ms outlier remains in the report. Treat that run as a stress observation, rather than a clean model comparison.

“Unloaded model” includes loading the model while the audio output is already running. It does not simulate reboot, cold filesystem caches, or launching the complete GUI app.

## Other measurements

- Controller feedback p95: 0.16–0.45 ms across these scenarios. This is internal state feedback, not visible UI latency.
- Stop-to-silence estimate p95: 16.7–20.8 ms. CPAL uses an assumed double-buffer delay on macOS; physical silence is unmeasured.
- Maximum sampled resident memory in the built-in-output benchmark set: 295.5 MB, excluding GPUI. This does not certify whole-app ready or peak memory.
- Default Kitten speech download: 36.5 MB. Additional optional model downloads are outside that initial download.

## Optimization evidence

A five-round interleaved original-versus-converted Kitten experiment measured replacement to first nonzero render over the same Bluetooth output: p95 617.9 ms → 260.3 ms, 25 warm trials per configuration, zero failures. Original range: 431.8–640.9 ms; converted range: 200.5–297.6 ms. This uses a different signal threshold and output device from the built-in speaker table; compare the two sides within this experiment only.

A separate CPU profile identified quantized ConvInteger/Im2col work as the main limiter. Replacing 74 integer convolutions with equivalent float operations over the same quantized values reduced that work. A native ONNX Runtime 1.28.2 check found exact seeded PCM hashes in all 72 cases: eight voices × three speeds × three inputs. This establishes the tested numerical equivalence, not listening or pronunciation quality.

## Acceptance status

| Approved requirement | Status |
| --- | --- |
| Visible feedback ≤100 ms at p95 | GPUI visibility not timed |
| Warm audible speech ≤500 ms at p95 | M4 software render proxy below target; actual end-to-end and M1 unverified |
| Stop to physical silence ≤100 ms at p95 | Software estimate below target; acoustic test pending |
| Cold first audible speech ≤3 seconds at p95 | Unloaded-model software proxy below target; complete app launch and M1 unverified |
| M1 / 8 GB / macOS 14+ baseline | No matching test machine available |
| Ready memory ≤500 MB; active peak ≤1 GB | Harness below limits; whole-app measurements pending |
| Initial speech download ≤200 MB | Default manifest 36.5 MB |
| App plus required assets ≤500 MB | Final packaged installation audit pending |
| All voices and 0.75×–2× speeds meet targets | PCM equivalence checked; full latency and quality matrix pending |

## Raw reports

- [Typical paragraph](builtin-typical.json)
- [Short selection](builtin-short.json)
- [100,000-character selection](builtin-long.json)
- [Unpunctuated selection](builtin-unpunctuated.json)
- [Replacement stress](builtin-replacement.json)
- [Unloaded model](builtin-cold.json)
- [Interleaved model comparison](float-replacement-experiment.json)
- [Inference thread comparison](first-chunk-threads.json)
- [Native 72-case PCM equivalence](native-float-equivalence.json)

The JSON files record raw trials, failures, hardware, load, model and binary hashes, and the exact commands used. Existing measurements predate the latest voice-label UI changes.

## Matched nine-voice startup run

A later release build exercised the application catalog and model-to-speaker mapping. Five shuffled process rounds per voice, five readings per process: 20 warm readings per voice after excluding each process's first inference. All 225 raw readings succeeded; 180 warm readings are summarized below. Same M4, built-in output, typical synthetic text, two threads, 1× speed, 32/180-character chunk limits and −50 dBFS real-render threshold. No heavy build ran during sampling. User applications remained running; the starting load is retained in the JSON.

| Voice | p50 | p95 | Worst |
| --- | ---: | ---: | ---: |
| Arthur | 132.0 ms | 167.8 ms | 167.9 ms |
| Clara | 212.5 ms | 223.6 ms | 223.8 ms |
| Elliot | 202.4 ms | 223.1 ms | 223.3 ms |
| Maya | 234.3 ms | 255.9 ms | 265.9 ms |
| Theo | 159.1 ms | 191.0 ms | 191.1 ms |
| Nora | 189.9 ms | 194.0 ms | 201.8 ms |
| Owen | 255.3 ms | 276.2 ms | 319.2 ms |
| Ruby | 212.2 ms | 213.0 ms | 213.3 ms |
| Heart | 985.7 ms | 1067.8 ms | 1071.8 ms |

**Heart misses the 500 ms warm software-onset target in this configuration.** It remains an optional voice and is not substituted with a distilled approximation. The eight Kitten voices are below that software threshold here. Neither result certifies end-to-end audible latency or M1 performance. Close differences between voices are not evidence of a statistically significant rank.

The maximum sampled process peak in this run was 398.8 MB (Heart), excluding GPUI. Stop p95 remains an assumed-buffer software estimate. Listening quality is unrated for all voices. The UI uses these p95 values and exposes measurement scope in descriptions/hover details; its speed bar is `clamp(1 − latency_ms / 1000, 0, 1)`, not a quality grade or a claim about the user's current machine.

[Full per-voice raw report](voice-startup.json). The catalog behavior check also verified that a Heart preview preserves saved voice 0, loads the correct model, cannot resume after Stop, and returns to Kitten after rapid model selections. Native UI checks are recorded separately in the implementation notes.
