# Sentence pacing and audio settings

The old policy split the first sentence after 32 characters and reduced native silence to 0.15×. The new default keeps up to 180 characters of sentence context, separates short sentences, restores native 1× silence, and enforces minimum sentence/paragraph quiet times. Default gaps are 300/650 ms; existing model quiet tails count toward these minimums. Gaps do not shrink when reading speed increases. No extra pause is appended after the final chunk.

Kitten recordings ended sentences near 0.04 amplitude before a hard jump to silence. A 5 ms streaming edge fade now brings sentence and paragraph boundaries to zero, without changing interior samples in Natural tone. Mid-sentence continuation chunks do not get punctuation gaps. A 5 ms PCM tail is retained on the producer thread, away from the realtime audio callback.

Settings now persist **Speed**, **Sentence pause** (0–1000 ms), **Paragraph pause** (0–2000 ms), **Volume** (25–150%), and **Tone** (Natural/Clear/Soft). Preview and Reset are in Settings; the playback popup remains button-free. Reset selects 1×, 300/650 ms, 100%, Natural. Existing settings gain these defaults without losing the selected voice.

Natural preserves the model’s tonal balance. Clear uses two RBJ peaking filters: −2 dB at 250 Hz and +1.5 dB at 2.5 kHz, Q=0.7. Soft uses a broad −3 dB band at 4.5 kHz, Q=0.5. A smooth over-range knee above 0.88 caps peaks below 0.98. These are listening choices, not a claim that EQ repairs prosody or pronunciation. [Filter equations](https://www.w3.org/TR/audio-eq-cookbook/).

## Current native startup

Five interleaved rounds per voice, five readings each, with the first inference excluded from warm summaries: **20 warm readings per voice, 180 total**, plus 45 first inferences. Zero failed requests. Real CPAL callback measurements on M4/16 GB, built-in speakers; selection capture, GPUI and physical/acoustic output are excluded. p95 uses nearest rank.

| Voice | Median | p95 | Full range | Worst stop estimate |
|---|---:|---:|---:|---:|
| Arthur | 319.7 ms | 382.8 ms | 318.3–383.3 ms | 16.4 ms |
| Clara | 426.0 ms | 563.9 ms | 404.4–639.1 ms | 16.3 ms |
| Elliot | 383.1 ms | 394.3 ms | 371.9–403.3 ms | 17.2 ms |
| Maya | 426.4 ms | 468.3 ms | 415.3–468.4 ms | 14.6 ms |
| Theo | 351.3 ms | 362.2 ms | 340.7–373.2 ms | 16.5 ms |
| Nora | 404.2 ms | 511.7 ms | 393.9–532.3 ms | 17.1 ms |
| Owen | 489.6 ms | 500.1 ms | 478.3–500.7 ms | 16.8 ms |
| Ruby | 356.4 ms | 383.5 ms | 350.2–447.8 ms | 16.0 ms |
| Heart | 2511.4 ms | 2847.0 ms | 2217.9–2899.5 ms | 16.1 ms |

**Clara, Nora, Owen and Heart miss the original 500 ms p95 target in this sample.** The user allowed more startup latency for quality; these are measured tradeoffs, not a relaxed M1 certification. Worst stop estimates remain below 18 ms in the warm sample. CPAL estimates are not acoustic measurements.

The old startup values were replaced in the app. Old naturalness predictions were invalidated because the chunk policy changed; the app shows Unrated rather than assigning stale scores to new audio. Previous scores remain in `previous-voice-ratings.json` for comparison.

## Evidence

- `startup.jsonl`, `startup-protocol.json`, `startup-summary.json`: exact benchmark binary, source hashes, all trials and failure counts.
- `unfaded-startup.jsonl` is an intermediate run before the edge fix and is not the source of current indicators.
- `.cache/pacing/before-sentences.wav` reproduces the short first fragment; `.cache/pacing/final-sentences.wav` has complete sentence context and ends each gap boundary at zero.
- The native lexicon frontend’s known unsupported-word omission issue remains unresolved. EQ and pacing do not fix it. The five newer model recordings are evaluated separately in `../five-models/REPORT.md`.

Tests cover complete sentence context, short sentences, abbreviations, quotations, blank-line paragraphs, Unicode content preservation, pause accounting, callback-size-independent EQ/fades, unchanged Natural interiors, and pitch/duration preservation through 48→24 kHz tempo conversion.
