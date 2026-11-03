# Implementation record

The user approved SPEC.md through Q31, then requested continued iteration until the performance goal is reached. Implementation is authorized. Hardware/release certification remains subject to the explicit spec gates.

## Ownership

The Rust reading controller owns one request identity and cancellation state. Native capture sends accepted reading requests; speech runs on a dedicated worker; the audio callback consumes bounded PCM; GPUI observes status and sends commands. History persistence and model downloads run outside the UI/audio paths.

## First performance gate

Claim to test: release-build Kokoro first-chunk generation delivers valid, non-silent PCM within the available first-speech budget on this M4. This measures a component of the eventual user path, not shortcut-to-audible latency or M1 certification. The harness must count failures, record the actual model/thread/voice/speed/input configuration and separate first inference from warm trials.

## Pinned choices

- Rust 1.98.1, matching the inspected upstream GPUI toolchain.
- GPUI/platform revision 33156f0d81f3a90854626f7286869a7edc6234a0, with upstream runtime_shaders. This avoids requiring the unavailable offline Metal compiler; system runtime Metal compilation is supported upstream. Measure first UI launch separately.
- sherpa-onnx 2.0.1 arm64 shared runtime. The release archive is verified against its published SHA256 before extraction.
- Speech payload manifest: 180,621,105 downloaded bytes, below 200,000,000. Includes pinned model/voices/tokens and all four explicitly licensed pronunciation dictionaries. Lexicons are deterministically regenerated locally.

No model performance, M1 compatibility, audible end-to-end latency or signing gate is marked passed by these setup checks.


## Native playback and model iteration

Kokoro v1.1 int8 failed the first warm synthesis budget on the M4 at the initial 68-character chunk. The approved fallback evaluation moved to the pinned Apache-2.0 Kitten nano v0.1 artifact. Profiling the latter identified `ConvInteger` / `Im2col` in the CPU runtime as a limiter. The reproducible CPU conversion changes 74 integer convolutions to float32 arithmetic over the same integer-valued inputs/weights, preserving activation quantization, zero points and output scales. All remaining graph operators are retained. Setup performs this transformation in Rust; Python is used only by independent verification tooling.

The source download is 36,494,560 bytes including all pronunciation dictionaries. The Rust-derived active model is 40,116,494 bytes, SHA256 `264b48c18257cc172960c9321b233b249e215879b5d295766baeace33f5eb5b1`. Active CPU model, voice, token and generated US lexicon hashes are checked before loading. Exact seeded PCM digests matched in all 27 native C-API cases spanning three voices, three speeds and three fixed English inputs. The bundled ONNX Runtime reports version 1.28.2; the Python wheel at that version was unavailable, so the independent native harness was used in addition to the Python 1.24.4 numerical checks. See `performance/native-float-equivalence.json`.

Five interleaved rounds of original versus converted Kitten produced 25 warm replacement trials per configuration without errors. Request-to-nonzero-render p95 was 617.89 ms original and 260.27 ms converted on the M4, Bluetooth output. These are software callback results, excluding selection capture, GPUI and physical-device latency. No timings from seeded checksum tests or profiler runs are used as latency results.

A subsequent built-in-speaker software matrix used the Rust-derived model and 100 warm trials in each short, typical, 100,000-character long and unpunctuated bucket, plus 20 unloaded-model trials. All processes and trials succeeded. The newer harness additionally records the first sample exceeding -50 dBFS; it still does not prove acoustic onset. Raw runs/configuration and summaries are in `performance/builtin-*.json`. Final whole-app and M1 certification remain open.

## Concurrency and failure findings

- PCM queues belong to request objects. Old consumers cannot discard a new request's samples. The controller retains retired requests until callback/producer references are gone, keeping PCM deallocation off the audio callback.
- Synthesis has a bounded latest-request mailbox; stopped requests cannot build an unbounded job queue.
- Shutdown joins synthesis, control and device workers. An early asynchronous-exit prototype crashed after completed timing rows; that experiment was treated as failed. C++ exceptions are contained at the Rust FFI boundary and return content-free errors.
- PCM is bounded to two source-audio seconds per live request. Pause retains position and the meter; stop invalidates the run and silences subsequent render frames. Route changes rebuild output without accepting stale completion events.

## Native UI progress

A real GPUI settings window and nonactivating compact player are implemented. Native integration provides menu-bar actions, recorded global shortcuts, modifier-held selection state, AX capture with messaging timeouts, explicit clipboard reading, source-display placement, lock/sleep pausing and reduced-motion detection. The user approved enabling Accessibility and Input Monitoring for testing; the actual grants and acceptance matrix are still pending.

The first visual run found invisible text despite visible controls. `cargo run --release --features visual-test --bin render-check` reproduced the exact symptom: 0 white text pixels with 576 red control pixels. GPUI had been built without its platform `font-kit` feature. That feature is now structurally included in `desktop`; the same real offscreen Metal test passed with 3,486 text pixels. The check now also covers embedded Lucide SVG rendering. The latest UI refinement follows Handy v0.9.8 source and captures, rather than a guessed typeface; see `handy-reference.md`.

The development `.app` is built with `scripts/package_app.py`. `--ui-preview` uses ad-hoc signing; the main path supports a configured identity or the existing local development identity. Developer ID signing/notarization, public updater distribution, M1/macOS14 certification, physical audible/visible latency, full compatibility, listening/pronunciation checks and the remaining spec features are not marked passed.

## 2026-10-30 follow-up

The current catalog has eight Kitten aliases plus authentic Kokoro Heart. Screening covered 21 voices / 294 full recordings, including experimental Kitten 0.8 and Paradee 1.2 Python pipelines. The native frontend has a confirmed unsupported-token omission defect; no candidate is promoted and no human listening/M1 certification is claimed. See `performance/voice-evaluation/REPORT.md`. Resumable evaluation now checks immutable stage inputs and waveform hashes. The retained original run is explicitly marked as a verified post-run import, because historical code fingerprints were not captured at execution.

The user narrowed activation to ⌘B and Esc cancellation. Hold monitoring, configurable shortcut recording and extra playback shortcuts are removed. Legacy settings preserve speech preferences and ignore old bindings. The popup stays button-free and click-through. A macOS permission switch can remain enabled for an older ad-hoc build; native trust and real selected-text capture must be checked for the exact packaged binary.

The permission mismatch was reproduced in the running app: macOS showed the switch on while `AXIsProcessTrusted()` was false. Toggling and re-adding the existing entry did not resolve it. Removing the old Accessibility entry, adding the exact final bundle, and restarting changed the native General page from **Allow access** to **Open settings**. The app also refreshes this status on activation. The user then physically tested in Zen and confirmed that ⌘B reads the selected text and Esc stops it. Targeted automated ⌘B input activated TextEdit’s Bold command and is not used as global-key validation. Current evidence is in `native-verification.json`.


## Current refinement: modifier activation and screenshot UI

The latest source replaces the temporary Command-B binding with one shared tap/hold key. Auto chooses Fn/Globe only when the system preference explicitly reports no Fn action; otherwise it chooses right Option. Users can choose either key explicitly. System preference detection is best-effort and does not detect all third-party conflicts. No system settings are modified. Native local/global NSEvent observers pass only key codes and reduced modifier flags to a pure Rust classifier; letters and modifier combinations cancel the gesture. A quick release reads the selection; a hold of at least 350 ms reads only a changed selection on release. Escape cancels playback and the gesture. Existing audio survives invalid hold gestures.

The shortcut section now shares the surrounding Settings row frame, 13.125 px labels, compact neutral controls, solid dividers and group colors. The read-key choice comes first, followed by tap, hold and cancel rows. Oversized keyboard tiles and the redundant tap description are removed; the hold row retains “Release to read”. Both activation rows display the resolved key. The playback pill remains button-free and click-through, with a compact lime waveform, amber selection-failure state and a preparing spinner. Actual production components are shared with the offscreen GPUI render check.

Sentence/paragraph pauses, tone and volume are now settings. The current defaults add no silence; source sentence prosody and short edge fades remain. Five newer models have experimental local adapters and measured reports in `performance/five-models/REPORT.md`; none is promoted based on a single short passage or certified on M1.

Permission fixing is paused at the user's request. The previously approved main bundle at `dist/InfoTransfer.app` retains its original Command-B/Esc code identity and is not overwritten by this work. The current ad-hoc preview has one fixed path (`.cache/ui-preview/InfoTransfer.app`) and bundle identifier (`dev.infotransfer.reader.preview`); it does not inherit the main app's grants. The persistent local signing certificate exists, but Keychain access and permission-preserving replacement remain unverified. This is separate from paid Apple Developer membership: local development can be free, while Developer ID/notarization require program membership.

## 2026-10-31: developer reading and one running app

The native omission was reproduced using the production export: raw `5` yielded zero samples and raw `PR #5` logged skipped OOV tokens. The shared Rust speech frontend now prepares numbers, dates, units, versions, common programming terms and contractions before choosing a backend. Native Kitten/Heart inference receives explicit phoneme codepoints through the sherpa 2.x generation API rather than relying on its raw-text lexicon frontend. The duplicate native lexicon load is removed. Heart uses its pinned Misaki lexicon and an offline new-word fallback. Kitten uses full eSpeak IPA for bounded clauses, preserving affricates, vowel lengths and phrase context; applying Heart-style lexical phones to it produced systematic transcription errors for “check”, “JSON” and “CPU”. A persistent, separate eSpeak NG process performs pronunciation only. The model token alphabet is validated, with word-level/letter-spelling fallback for unsupported phones. No eSpeak audio is generated.

Dictionary settings provide add, edit, remove, Match case and preview controls. Replacements are literal, whole-word, longest-first and nonrecursive; custom entries take precedence over built-in expansion. Both curly and straight apostrophes match. Original selected text is preserved. The native editor validates through a pure Rust callback before closing its sheet, then sends the accepted edit to GPUI; queuing validation behind the modal loop had prevented completion. Match case is serialized as a JSON boolean. Live UI checks verified duplicate errors, saving both case values, editing, persistence after restart and removal of the disposable entry.

Syntax punctuation is skipped without inserted pauses or punctuation names. Numeric meaning, such as `PR #5`, `90%`, decimal points and comparisons, remains spoken. Real sentence marks remain model prosody cues. Default sentence and paragraph padding are zero; a versioned migration removes only the previous automatic 300/650 ms pair and preserves custom settings. Tone remains Natural at unity gain by default. EQ is optional and does not stand in for pronunciation or prosody correction.

The GUI acquires an OS file lock before starting speech workers. A second current GUI exits and activates an existing InfoTransfer bundle, including older preview identities. CLI exports and benchmarks are unaffected. The packager refuses to replace a bundle while any InfoTransfer GUI is running and no longer creates tagged preview variants. Five inactive tagged previews were archived under `.cache/retired-previews` with binary/archive hashes and ZIP integrity checks; the main approved bundle is untouched. The lock is released by the OS on exit or crash.

The pronunciation helper adds about 19.8 MB, including its full GPL-3.0-or-later license and corresponding source. Rebuild with `python3 scripts/prepare_pronunciation.py`; the original application remains Apache-2.0. Component notices and distribution review remain required for a public release. Current correctness evidence and limits are in [developer-reading/correctness-summary.json](performance/developer-reading/correctness-summary.json). Older quality/startup ratings are withdrawn from the app because they describe the superseded frontend.

## 2026-11-03: usability pass

Local signing now uses a self-signed identity in a dedicated keychain, so builds cannot hang on a hidden Keychain prompt and the Accessibility requirement stays stable across rebuilds. The app no longer holds audio output open while idle: output opens on key press or reading and closes after 60 idle seconds, which measured about 70 ms for the first open in a process and 5–11 ms after that. Notices expire after four seconds, so the popup and its Escape shortcut cannot stay stuck. Chunks with nothing to speak are skipped rather than ending the reading, and a long opening sentence starts at a clause break when one exists. A long opening sentence now ends at punctuation, then a clause word, then a word break within 100 characters, and engines run one silent inference after loading so the first reading after launch is not slower. M4 built-in speakers, Kitten Arthur, small trial: first signal 360–389 ms for a 73-character sentence and 456–470 ms for a long sentence with no comma. Heart took 2.5–3.3 s for the same 73-character sentence and misses the fast-model target. The pronunciation helper restarts after a crash or a five-second hang. Voices are saved by source ID.

Driven with synthetic key events against TextEdit from an already-trusted host process: tap, hold, Esc, empty selection and the moving meter all worked with right Option. Open at login toggled on and off. Zen exposes page selections only as text markers, so plain AXSelectedText failed there; selection capture now falls back to AXSelectedTextMarkerRange. With triple-click selections, tap reading worked in Zen and Preview, and an empty Zen selection showed "No text selected". Fn was not verified because another app on the test Mac responds to it. Physical key presses, Notes and VS Code (not installed) still need a manual pass. The pinned-output developer-reading timing rerun was not completed; `timing-20261031T094904Z-61f687` remains excluded from acceptance.
