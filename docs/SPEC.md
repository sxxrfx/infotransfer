# InfoTransfer — implementation specification

Status: approved through Q31. Native implementation and validation are authorized, including the explicit final-review defaults. The user subsequently requested continued iteration toward the performance goal. Decisions are recorded in [decision-tree.md](decision-tree.md); implementation evidence is in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## 1. Product and scope

InfoTransfer is a public, free/open-source macOS app that reads deliberately selected text aloud using local open-source speech models. It reduces the effort between selecting text and hearing it, including for readers with dyslexia. Responsiveness is the highest priority. No font, animation, or speech configuration is presented as a clinical treatment.

- Native UI: GPUI with a Rust application core. GPUI is required.
- Supported processor family: Apple Silicon. Baseline: M1, 8 GB RAM, macOS 14+.
- Primary acceptance app: Zen browser. Secondary apps: Preview with selectable-text PDFs, Apple Notes, and VS Code.
- English reading, eight voices, local previews, and 0.75×–2× speed.
- Select text, then tap Fn/Globe (right Option when Fn is assigned). Esc cancels playback and dismisses the visual-only popup.
- One compact playback overlay with an audio visualizer; separate settings/history windows and menu bar access.
- Encrypted, opt-in local reading history.
- Automatic update checks; user-initiated download and installation.
- Apache-2.0 original application code, exact third-party notices, public source/build instructions, and signed/notarized DMG distribution.

Excluded from v1: OCR/scanned-image reading, document import/library management, cloud speech, accounts/sync, analytics/crash uploads, AI rewriting/translation, voice cloning, audio export, expanded reading panels, word highlighting, sentence navigation, Intel support, other operating systems, and Mac App Store distribution. Safari/Chrome and other apps can work but are outside the required acceptance matrix.

## 2. Reading and activation

### Select, then shortcut

The user selects text normally and taps the active key: Fn/Globe by preference, with right Option as the fallback. Capture the foreground application and intended selection before presenting anything that could alter focus. An accepted request replaces the current reading. Repeating a read of the same selection restarts it.

### Shortcuts and fallback

The latest refinement replaces temporary ⌘B with a modifier-only activation key shared by tap and hold. **Auto** prefers Fn/Globe when the read-only system preference explicitly reports no Fn action, otherwise uses right Option. Settings offer Auto, Fn and Right Option and display the resolved key in both rows. System Fn detection is best-effort (an undocumented preference); unknown values fall back conservatively, and third-party conflicts cannot all be detected. Never change macOS keyboard settings automatically. Explicit Fn selection warns when it may conflict with a system action.

Tap (release within 350 ms) reads the current selection. Hold (350 ms or longer) reads on release only after the selection changes. A failed/unknown baseline, empty or unchanged selection, source-app change, Escape, lock/sleep or a superseding gesture cancels the held gesture without stopping existing speech. Selected text plus an available selected range identify a new selection; range availability alone does not. Hold waits safely for an in-flight baseline capture. Modifier combinations and ordinary Fn/Option typing cancel the gesture until release; left Option does not activate. Observers do not suppress or modify other apps' input. Escape cancels audio and dismisses the button-free, click-through popup. Pause/resume and stop remain menu-bar actions. ⌘B is no longer registered by the new build.

Permission refusal leaves settings, local voice previews and deliberately invoked clipboard reading usable once speech assets are ready; selection activation remains unavailable until its required access is granted.

Use macOS Accessibility selection first. Empty, inaccessible, or protected fields produce a clear state/recovery action. Never simulate a copy shortcut, read stale clipboard content automatically, or capture password fields. Offer an explicit **Read clipboard** action after the user copies text. Clipboard reading works only by deliberate invocation; it does not modify the clipboard.

## 3. Playback contract

There is one active playback request. Selected-text reading, explicit clipboard reading, history replay, and voice preview share the same controller; audio never overlaps and there is no queue.

- A new accepted read cancels old generation/output and clears buffered audio.
- Pause preserves the playback position. Resume continues that reading; it does not start over.
- Stop invalidates the request, clears queued output, and hides the player. No stale callback may restart it.
- Final-review preview rule: a voice preview replaces the current reading, uses a short local sample, and is never saved to history. Preview controls identify the action clearly.
- Speed/voice changes update preferences immediately and apply to active generation at the next sentence/chunk boundary. Bound lookahead so old settings cannot remain audible for an unbounded buffer.
- Continue across normal foreground-app switches.
- Follow the system-default audio output and continue through device changes, retaining playback position. A brief handoff gap is measured separately. Headphone removal may continue speech through speakers.
- Final-review lock/sleep rule: pause on screen lock or sleep and require explicit resume. This pause and user pause/stop take precedence over automatic device recovery.
- If no usable output exists, retain position and show an actionable output error. Recover only the still-current request; never resurrect a stopped/replaced reading.
- Quit stops playback and the background service. Closing settings keeps the menu bar service running.

The request state model is `idle → capturing → preparing → reading ⇄ paused → idle`, with actionable capture/model/output errors. Every asynchronous result carries the owning request identity. Replacement and cancellation must remain correct when capture, synthesis, persistence, and UI updates complete out of order.

## 4. Text and local speech

Accept up to **100,000 Unicode characters**, counted as Unicode scalar values, without an additional confirmation. Reject a larger selection with a clear request to select less; do not silently truncate. Preserve the selected content/order. Normalize whitespace and text formatting only as needed for speech; do not rewrite or translate it.

Start with a bounded initial chunk and generate/play incrementally. Do not normalize, phonemize, or synthesize the entire long selection before first output. Bound chunk length and queued PCM, including for unpunctuated text. Keep capture/inference/persistence off the UI thread and keep expensive or blocking work out of the audio callback.

Handle paragraphs, abbreviations, numbers, dates, URLs, code-like fragments, punctuation, Unicode, and out-of-vocabulary English words explicitly through the chosen text frontend. English-only scope does not guarantee foreign-language pronunciation. Do not silently discard words; report unsupported-input or correctness limitations. Reject invalid/non-finite PCM and surface synthesis failure honestly.

Latest developer-reading refinements: apply one shared text preparation policy to every voice. Speak numerical meaning (for example, `PR #5` as “pull request number five”), versions, units and common programming terms. Automatically expand common English contractions, including curly apostrophes and `I'am`. Provide editable whole-word replacements with optional case matching, previews, add/edit/remove and persistent local storage; these affect spoken output only. Custom rules take precedence and are not recursively reapplied. No fixed dictionary can guarantee every future identifier or proper name.

Skip syntax punctuation such as colons, filename dots and brackets without naming it or adding quiet. Preserve decimal/version meaning and ordinary sentence prosody. Default added sentence and paragraph silence is zero; optional user pause settings remain available. Use neural prosody with neutral EQ by default. EQ cannot guarantee natural delivery or correct a model's pronunciation errors.

### Model selection

Start with a pinned Kokoro/sherpa-onnx English configuration. Validate all eight offered voices using listening and correctness checks. A smaller compatible open-source alternative is authorized if needed, under the same language/voice, licensing, latency, size and memory requirements. Report unmet targets; never silently relax them or switch to cloud/system speech.

The documented first candidate uses:

- sherpa-onnx v2.0.1, commit `af2d8d81dbfa61c4cea4c568da78f3982edca005`.
- Corrected Kokoro int8 conversion at artifact revision `1355334be009ccce397060ee639a874137a128fa`, with its compatible voice table and tokens.
- US/GB pronunciation lexicons regenerated using the pinned sherpa generator from the explicitly Apache-2.0 hexgrad/Misaki dataset, revision `7a6093fd11124188077f2dc16397cc465466d59a`.

See [research.md](research.md) for source links, model/voice hashes, license provenance and the older short-text int8 correctness issue. Verify downloaded bytes. Do not substitute the uncleared generic lexicon release asset. Model-code, weights, converted artifacts, voice data, dictionaries, runtime binaries and notices each need a record. Converted-file sizes are not installed-app or RAM measurements.

The candidate is documented, not benchmarked in this project. Pin the exact validated dependency/artifact combination in the implementation, record any justified candidate change, and preserve required notices.

A materially different distribution license is a scope change, not an automatic consequence of choosing a smaller model. Select compatible artifacts/frontend/runtime components within the accepted release plan.

### Readiness

Normal mode keeps the selected model loaded while the app runs. Optional memory-saving mode unloads it after **five idle minutes**. Active/paused readings keep it loaded. Downloads, permission setup and update checks do not block playback readiness or UI feedback.

## 5. UI and accessibility

Use the installed Handy v0.9.8 screenshots as the visual/navigation reference: [General](../assets/references/handy-general.png), [Models](../assets/references/handy-models.png), [Advanced](../assets/references/handy-advanced.png). The [interactive study](../prototypes/handy-reader.html) is the current design artifact. It contains simulated data, not a working native speech app.

Reproduce the charcoal surfaces, fine dividers, light text, pink selected states, left sidebar, grouped rows, right-aligned controls, selected voice cards and persistent speech-choice/update footer. Keep InfoTransfer branding. Current implemented navigation: **General → Voices → Dictionary → Advanced → About**; opt-in History remains a required feature pending implementation. Adapt recording/input controls to reading/playback. Omit transcription-only settings, unnecessary descriptions/info icons, decorative copy and invented quality/speed ratings. See [handy-reference.md](handy-reference.md).

Run one GUI instance across development/preview bundle variants. Reopening brings forward the current app. Development packaging uses one fixed preview location and refuses replacement while the GUI is running.

### Compact player

- Position: bottom center of the source display, above the Dock; handle multiple displays, Spaces and full-screen apps without stealing source focus.
- States: preparing, reading, paused, error. Hide on completion/stop; retain idle menu bar access.
- The playback popup contains no buttons or interactive controls: only the output meter and state.
- Escape cancels the reading, invalidates queued audio and dismisses the popup. Start a new reading to speak again. Separate pause/resume remains available from the menu bar. Escape is reserved only while the popup is visible.
- Pause/resume, stop, voice/speed, settings and explicit clipboard recovery remain in the menu bar/settings.
- Visualizer: a small output energy/envelope display derived from PCM submitted for playback, not microphone input or synthesis progress. No microphone permission is needed.
- Cap visible meter updates around 30 Hz. Freeze on pause and cancel ongoing redraw scheduling when hidden/idle. Reduced motion uses a static indicator and clear state text.
- Preparing is static and truthful. No entrance/exit animation delays activation. No guessed word timings or unreliable total-duration estimate.

Use native semantics/accessible names, visible focus, complete keyboard paths, readable contrast and layouts that tolerate larger text. Pin a coherent GPUI revision with the required accessibility integration and matching platform crates; commit the lockfile. Verify actual VoiceOver behavior, including custom control roles/actions and stable unique IDs. Passive playback must not seize VoiceOver focus or announce each visualizer update.

## 6. History and privacy

History is **off by default** and applies prospectively. Reading and voice previews work without it. When enabled, save the selected text, timestamp, voice and speed locally using authenticated encryption with a key held in macOS Keychain. Save no audio, source-app/window titles, URLs or identifying source metadata. Replay regenerates speech locally.

- Save asynchronously when speech begins, never before a failed capture and never on the latency-critical playback path.
- Keep at most **100 readings**; expire anything older than **30 days**. Apply both limits on saving and when opening/pruning history.
- Provide individual delete and clear-all. Clearing all requires confirmation. Explain consequential deletion only where needed.
- Disabling saving stops future saves and retains existing entries until explicitly cleared. Do not save earlier readings retrospectively.
- Keychain/storage failure does not block reading. Show that the reading was not saved and allow retry while its content is still available. Never write an unencrypted fallback, plaintext journal, or plaintext content-bearing temporary file.
- Final-review replay rule: replaying an existing entry does not create a duplicate history entry or reset its original age.
- Do not log/upload selected text or audio. History opt-in does not authorize diagnostics or network sharing. Deletion removes application-managed records; do not promise physical erasure from backups.

Settings such as bindings, voice and speed are stored separately from reading content. They do not enable history.

## 7. First launch, permissions and recovery

Guide first launch through explicit speech-asset download, required selection access and the Fn/right Option and Esc interaction. Show progress, cancellation and useful retry/recovery actions. Keep already working assets until replacements are fully downloaded and verified; activate assets atomically. Handle interrupted downloads, low disk space, invalid checksums and runtime load failure.

Request Accessibility access for cross-app selection. Tap/hold uses registered Carbon key-press/key-release events and does not request Input Monitoring. Check permission denial/revocation and recheck after the user returns from System Settings. Never assume that showing a permission prompt means access was granted. No Screen Recording or microphone access is part of the selectable-text feature.

Open at login defaults off. Once setup is complete, closing settings preserves the menu bar app; explicit Quit ends it. Local model/permission errors offer specific **Retry**, **Download voices**, or **Open System Settings** actions as appropriate. There is no silent cloud, system-voice, OCR or automatic-copy fallback.

## 8. Updates and distribution

Fetch a public HTTPS release feed after launch and no more than once per 24 hours. Keep checks off the reading/UI-critical path and provide a disable switch. Send no reading content, stable user ID or system profile. Ordinary request metadata can be visible to the hosting provider.

Notify that an update is available without interrupting speech. Users initiate download and installation; downloading, install authorization and relaunch are distinct states. Before an update relaunch, warn that active playback will stop and allow cancellation. Verify update artifacts and the chosen feed/metadata integrity policy; normal macOS signing/notarization still applies.

Sparkle is a candidate native updater, subject to a proven Rust/AppKit integration and exact notices. A native status-item adapter can support GPUI's menu bar presence. All app windows remain GPUI; the HTML prototype is not a proposed webview implementation.

Release original code under Apache-2.0 with compatible dependency/model/voice notices, reproducible build/setup instructions, and a signed/notarized Apple Silicon DMG. Mac App Store distribution is outside this architecture's v1 plan.

## 9. Acceptance targets and measurement

These are accepted targets, not measured results. A miss remains a reported issue rather than being silently converted into a looser requirement.

| Measure | Acceptance target | Definition |
| --- | --- | --- |
| Visible feedback | p95 ≤100 ms | Accepted activation completion to visible capture/preparing/reading state. |
| Warm first speech | p95 ≤500 ms | Activation completion to first audible non-silent output with the model loaded and output ready. |
| Stop | p95 ≤100 ms | Stop input to silence, including clearing queued output. |
| Cold first speech | p95 ≤3 s | App/UI ready, selected speech model unloaded. Initial process launch and downloads are separate measurements. |
| Initial speech assets | ≤200 MB | Required model, voice and pronunciation assets. |
| Installed app/assets | ≤500 MB | Full app/runtime plus required speech assets; excludes opt-in history and temporary installer/update staging. |
| Ready-state RAM | ≤500 MB | Resident memory with the model ready. |
| Peak active RAM | ≤1 GB | Peak resident memory during the accepted workload. |

Use decimal MB/GB for these resource limits and report raw bytes as well. Baseline is M1/8GB/macOS14+ using built-in audio; report Bluetooth and device-handoff behavior separately. A hold activation's clock begins at its successful release/commit event.

Record activation, capture completion, text preparation, first PCM and audio submission timestamps. An audio callback is a software proxy, not proof of physical audible latency; label the method and validate audible output with a suitable loopback/recorded measurement. Report software-only results as such.

Use release builds and record machine/OS, model/frontend hashes, voice, speed, output device and build configuration. Exercise short (including one-word), typical and long/unpunctuated inputs; include the maximum supported selection. Collect at least 100 warm trials per short/typical/long bucket and at least 20 unloaded-model trials per candidate configuration. Report p50/p95, failures and raw samples. Perform representative speed/voice quality and finite-PCM checks, especially for the corrected int8 artifact. Retain meaningful behavioral checks rather than tests that merely mirror implementation structure.

Measure initial launch, wake/resume, device handoff, idle CPU/power and meter overhead separately; no extra numerical battery/idle-CPU guarantee is claimed. Idle/hidden playback UI must not spin a redraw loop, and a ready model must not synthesize when unused.

### Functional acceptance matrix

- Both activation workflows in Zen, Preview selectable PDFs, Notes and VS Code; test editable/static content, empty/protected/inaccessible selection and actual supported selection modalities.
- Offline reading/previews/history replay after assets are installed.
- Stop, pause/resume, repeated activation, rapid replacement, key repeat, source changes, shortcut conflicts and stale callbacks.
- Short/long text, punctuation, abbreviations, numbers, names/OOV words, URLs, Unicode and 0.75×–2× speed; no crashes, invalid PCM or silent truncation.
- Permission refusal/revocation/recovery, interrupted/corrupt downloads, low disk, failed model load, unavailable Keychain and failed persistence.
- History opt-in/retention/delete/clear/replay, disabling without erasing existing entries, and content-free logs.
- Multiple displays, Spaces/full-screen focus behavior, lock/sleep pause, explicit resume and continuing device changes.
- Keyboard/VoiceOver/reduced-motion behavior, readable controls and honest player states.
- Update opt-out, background availability check, invalid update rejection, manual installation/relaunch and usable signing/notarization artifacts.

## 10. Implementation and learning handover

Use clear responsibility boundaries inside a small Rust codebase: macOS selection/input/permissions; reading controller; speech frontend/runtime; audio output; GPUI settings/player/history; encrypted storage; asset/update lifecycle. These are responsibilities, not a requirement for separate crates or a general plugin system.

The reading controller owns request identity and cancellation. Audio and inference do not depend on a webview or UI event loop. Bound work/buffers, concentrate OS/runtime validation at boundaries, and keep normal controller logic explicit and testable.

After approval, complete one sustained implementation pass with early verifiable gates:

1. Build the GPUI shell and prove source-preserving capture in the acceptance apps, including Zen's held-selection path.
2. Integrate pinned local speech, incremental output and real cancellation; measure before committing to a model/backend configuration.
3. Complete the compact player, binding/setup flow, encrypted history, lifecycle/recovery and updater integration.
4. Validate actual behavior, package reviewable builds, and record measured targets, limitations and outstanding release gates.

Provide a concise architecture/data-flow explanation, dependency/model rationale, setup/build/run instructions, validation commands/results, and a short guide to extending one feature. No marketing copy, unnecessary abstractions or implementation details in ordinary product controls.

## 11. External gates and completion

The available development host is Apple M4, 16 GiB RAM, macOS 27.2. It can establish behavior and measurements for that configuration; it cannot certify the M1/8GB/macOS14 baseline. Do not extrapolate M1 performance from M4 results. A matching environment remains necessary for baseline certification.

Signed/notarized distribution requires release credentials. A working updater requires the actual public feed and signing configuration. Native OS permissions require the user's normal setup/consent flow. Complete all authorized local implementation and produce concrete reviewable artifacts before requesting any missing credentials/access; do not ask for secrets in chat.

Distinguish a working local app, an unsigned development package, and a certified signed public release. Never mark a missing external gate as passed, claim unmeasured targets, silently reduce scope, or call an unsigned package the signed release. Report the precise remaining gate and retain the accepted requirements.

## Final confirmation

Q1–Q30 settle the feature decisions. The user approved this complete spec through Q31, including final-review defaults, and authorized the native implementation pass. The [grilling skill](/Users/sxxrfx/.codex/plugins/cache/mattpocock/mattpocock-skills/1.3.2/skills/productivity/grilling/SKILL.md) requires: “Do not act on it until the user confirms you have reached a shared understanding.”


## Post-approval playback refinement

The user's later instruction supersedes the original in-popup control arrangement: “do not show any button in the playback popup. esc button should pause and exit the audio/popup.” The user clarified “not pause but cancel.” Escape therefore stops the current reading and dismisses the popup, with no resumable canceled reading. The indicator remains visual-only and nonactivating.

The user also requested more voices after implementation began. The app now offers all eight presets already present in the pinned Kitten voice table, without another model download. Representative voice correctness and performance validation remains required.

## Higher-quality model tradeoff — 2026-10-30

The user permits longer speech-start latency for higher-quality models. The original 500 ms warm target remains the fast-model reference, rather than an automatic exclusion of every optional quality model. No replacement numerical latency budget has been chosen. Compare measured startup, sustained generation, text accuracy and listening quality separately; a larger parameter count does not establish better reading quality. Visual feedback and Esc cancellation retain their responsiveness targets. Download, disk and RAM budgets have not yet been relaxed by this instruction; identify any candidate that exceeds them explicitly.
