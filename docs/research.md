# Feasibility notes

Research date: 2026-10-29. These are source-backed findings, not results from running an app or model in this repository. Upstream branches and model catalogs can change; freeze exact artifacts/versions before implementation.

## macOS selection is the first feasibility gate

[Apple's selected-text attribute](https://developer.apple.com/documentation/applicationservices/kaxselectedtextattribute) is required for editable text Accessibility objects. It does not establish universal capture of static web text, PDF viewers, custom UI, terminals or remote desktops. A named app/content compatibility matrix is necessary.

[AXIsProcessTrustedWithOptions](https://developer.apple.com/documentation/applicationservices/axisprocesstrustedwithoptions(_:)) returns before the user completes the permission interaction. The app must recheck trust and handle refusal or revocation.

Conventional registered shortcuts and observing global keyboard/mouse events have different requirements. [Apple Input Monitoring guidance](https://support.apple.com/guide/mac-help/control-access-to-input-monitoring-on-mac-mchl4cedafb6/mac) and [Quartz event services](https://developer.apple.com/documentation/coregraphics/quartz-event-services) support investigating the exact mechanism rather than requesting every permission in advance.

[Apple's App Sandbox restrictions](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox) explicitly identify assistive use of Accessibility APIs as incompatible with the sandbox. Direct download with signing/notarization is the current proposal. Mac App Store distribution is not established for this architecture.

## UI framework — GPUI required after round 1

The user's round 1 answer makes GPUI mandatory. The initial Tauri recommendation is superseded; its research is retained as context. UI GPU rendering remains separate from speech inference acceleration.

[Tauri's process model](https://v2.tauri.app/concept/process-model/) combines a Rust core with OS webviews; macOS uses WKWebView. [Its global shortcut plugin](https://v2.tauri.app/plugin/global-shortcut/) is one candidate for a conventional shortcut. It does not by itself establish hold-while-selecting semantics or text capture.

[GPUI](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) is a GPU-rendered Rust UI framework using Metal on macOS. Upstream describes pre-1.0 API churn. It is an alternative UI stack, not the speech model execution backend. Choose a GPU inference path only if it improves the agreed end-to-end measurements.

### GPUI follow-up findings

- [GPUI's manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml) and [macOS backend manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/Cargo.toml) declare Apache-2.0. The [Zed editor manifest](https://github.com/zed-industries/zed/blob/main/crates/zed/Cargo.toml) declares GPL-3.0-or-later. Audit the exact library/component graph used by this app; do not apply the editor's license to GPUI by assumption.
- [The inspected upstream build configuration](https://github.com/zed-industries/zed/blob/7f9a39a9c08373dcc62599906f8dc335d358d7c4/.cargo/config.toml) sets `MACOSX_DEPLOYMENT_TARGET` to 12.0. This is a build setting, not a support guarantee for our eventual model/runtime combination. The app's macOS floor remains a product decision and must be tested.
- Current upstream has [AccessKit integration guidance](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs) and [an accessible control example](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/a11y.rs). Custom elements need unique stable IDs, semantic roles, names/values, actions and keyboard focus behavior. A GPU-drawn control is not automatically an accessible control.
- The inspected [published `gpui-v0.2.2` manifest](https://github.com/zed-industries/zed/blob/gpui-v0.2.2/crates/gpui/Cargo.toml) lacks the AccessKit dependency in current upstream. Pin a coherent immutable revision for GPUI and platform crates; require actual VoiceOver validation.
- [The window API](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs) exposes window kinds and focus/display controls. In [the macOS implementation](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/window.rs), `PopUp` uses an `NSPanel` with a nonactivating style and Spaces/full-screen collection behavior. `Floating` is not the same nonactivating configuration. These source settings must be verified in our real overlay.
- [The macOS platform implementation](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/platform.rs) currently rejects `AnchoredPopup`; do not assume every cross-platform enum variant works on macOS. Multiple GPUI windows are shown in [an upstream example](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/move_entity_between_windows.rs).
- [The upstream tray-support PR](https://github.com/zed-industries/zed/pull/44047) was closed without merging. Menu bar ownership/callbacks may need an AppKit `NSStatusItem` adapter. GPUI's macOS Accessory activation policy is useful, but background lifecycle and settings-close behavior still need verification.

## Compact local models

| Candidate | Source facts | Caveats |
| --- | --- | --- |
| Kokoro | [Original 82M model](https://huggingface.co/hexgrad/Kokoro-82M), Apache-2.0 model weights. [ONNX artifact listing](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/tree/main/onnx): 326 MB fp32, 163 MB fp16, 86 MB q8f16, 92.4 MB quantized. | These files are converted artifacts; confirm the runtime supports the chosen variant. Download/bundle/RAM and text frontend are separate concerns. |
| Kitten lightweight | [Lightweight ONNX docs](https://github.com/KittenML/KittenTTS/blob/main/docs/onnx-models.md) document 15–80M variants, 25–80 MB, eight voices and CPU operation. [Mini 0.8 model card](https://huggingface.co/KittenML/kitten-tts-mini-0.8) lists Apache-2.0. | Evaluate the exact lightweight artifact. Do not conflate it with the larger Kitten-TTS 2 product. Quality/latency are unmeasured here. |
| Pocket TTS | [Official project](https://github.com/kyutai-labs/pocket-tts) offers actual audio streaming and currently lists smaller English variants. [Main model card](https://huggingface.co/kyutai/pocket-tts) describes CC-BY-4.0, access terms, and voice-specific licenses. | Publisher first-chunk claims are not app-level measurements. Verify each smaller artifact, runtime and license independently. |
| Piper | [Old repository](https://github.com/rhasspy/piper) is archived. [Current engine](https://github.com/OHF-Voice/piper1-gpl) is GPL-3.0. [Voice guidance](https://github.com/OHF-Voice/piper1-gpl/blob/main/docs/VOICES.md) requires reviewing per-voice model cards. | “Piper is MIT” is not an adequate description of the current engine and all its voices. Review exact distribution obligations. |

The engine, model weights, voice data, phonemizer, pronunciation resources, runtime binaries, and bundled notices each need a record. Model openness alone does not settle the whole application's dependency terms or voice provenance.

## Rust speech path

[sherpa-onnx Rust bindings](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/rust/sherpa-onnx/README.md) and the [TTS API](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/rust/sherpa-onnx/src/tts.rs) provide a viable direction. The API exposes sample callbacks/cancellation and sentence batching. Callback availability does not prove low-latency token/chunk streaming for every model.

Kokoro examples show [a lexicon path](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/kokoro_tts_en_lexicon.rs) and [a phonemizer path](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/kokoro_tts_en_phonemize.rs). The latter uses external phonemization/eSpeak resources. Review the actual selected dependency chain before calling the bundle permissively licensed.

Do not equate sentence-at-a-time generation with true incremental audio generation. Either may be fast enough for the product, but the first long sentence and punctuation-free text must be measured.

## Handy reference

[Handy](https://github.com/cjpais/Handy) is a local speech-to-text app using Tauri/Rust and a React/TypeScript UI. [Overlay settings](https://github.com/cjpais/Handy/blob/main/src/components/settings/ShowOverlay.tsx) and [overlay CSS](https://github.com/cjpais/Handy/blob/main/src/overlay/RecordingOverlay.css) show compact and expanded modes with top/bottom placement. [Actual upstream screenshot](https://raw.githubusercontent.com/cjpais/Handy/main/public/release-notes/0.9.0/overlay.png).

Useful inspiration: settings are separate from a quiet task overlay; the overlay reflects lifecycle state and can expand to show text. For TTS, use playback and output state rather than copying a microphone/transcription indicator.

## Unknowns requiring a real prototype after spec agreement

- Which source apps expose the intended selection through Accessibility.
- Cold versus warm first sound on the baseline Mac, including phonemization and audio-device overhead.
- Whether quantization produces acceptable reading quality for the target listener and speed.
- True first callback granularity and sustainable generation speed for each runtime/model pairing.
- Actual total download, installed size, peak/resident memory, and idle power impact.
- Cancellation of a busy inference call, stale callbacks, and clearing queued audio.
- Chunk boundaries, abbreviations, numbers, Unicode and long unpunctuated selections.
- Permissions, source-app focus, multiple displays, Spaces, full-screen apps, sleep and output-device changes.
- Whether sentence highlighting helps the target user; precise word following requires real timing/alignment data.

No benchmark, universal compatibility claim, or final model choice should be inferred from this research.

## Round 3 verification — exact artifacts and current runtime

These findings qualify the earlier catalog-level model sizes. An export's weights, voice data, frontend, and runtime must be considered together. They are not interchangeable configurations.

| Documented candidate | Asset facts | Remaining gate |
| --- | --- | --- |
| Kitten nano v0.1 fp16 + sherpa-onnx v2.0.1 lexicon example | Pinned ONNX 23,848,586 bytes; voice data 8,192 bytes; tokens 1,064 bytes. With the example's English lexicon/license, approximately 30.2 MB before runtime binaries. Eight voices are documented. | The model bundle carries Apache-2.0. The separate lexicon asset's exact provenance/license was not verified; resolve it before redistribution. No quality/latency measurements were performed. |
| Kitten nano v0.8 int8 | Pinned model 24,369,971 bytes, voice data 3,278,902 bytes, config 688 bytes; approximately 27.6 MB before frontend/runtime. Pinned model card says Apache-2.0. | Compatibility of this newer artifact with the documented sherpa Rust path was not established. |
| Kokoro sherpa v1.1 int8 | Model 114,203,914 bytes, voice data 53,790,720 bytes, US lexicon 5,956,885 bytes; approximately 174 MB including tokens/license, before runtime. | This is a different conversion from the smaller ONNX-community v1.0 files. Verify the full artifact/frontend license chain and actual performance. |

Sources: [pinned Kitten v0.1 files](https://huggingface.co/csukuangfj/kitten-nano-en-v0_1-fp16/tree/2f82cb5390e25056d71ef39f076e9dec727e3113), [its license](https://huggingface.co/csukuangfj/kitten-nano-en-v0_1-fp16/raw/2f82cb5390e25056d71ef39f076e9dec727e3113/LICENSE), [documented voices](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kitten.html), [Rust example](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/rust-api-examples/examples/kitten_tts_en_lexicon.rs), [runner and lexicon reference](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/rust-api-examples/run-kitten-tts-en-lexicon.sh), [pinned Kitten v0.8 files/card](https://huggingface.co/KittenML/kitten-tts-nano-0.8-int8/tree/84781d74e29ee25217551556398b42f80593a813), [pinned Kokoro files](https://huggingface.co/csukuangfj/kokoro-int8-multi-lang-v1_1/tree/1355334be009ccce397060ee639a874137a128fa). Byte counts were obtained from upstream file/release metadata without downloading models.

Current Kitten-TTS 2 uses [different Community License terms](https://kittenml.com/license/). Do not generalize a legacy artifact's Apache-2.0 status to the current product family.

sherpa-onnx removed built-in eSpeak/piper-phonemize in v2.0.0: [maintainer change](https://github.com/k2-fsa/sherpa-onnx/pull/4055). The current lexicon paths therefore differ from older archives/examples discussed above. Pin the actual release and packaged frontend, omit unused resources, and preserve verified notices. [sherpa v2.0.1 license](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/LICENSE); [ONNX Runtime license](https://github.com/microsoft/onnxruntime/blob/main/LICENSE). A permissive runtime license does not resolve an undocumented external lexicon license.

The [Kokoro implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/sherpa-onnx/csrc/offline-tts-kokoro-impl.h) calls back after generated batches. Document this as incremental chunk playback, not a measured end-to-end streaming latency claim. A safe model-selection rule must reject an artifact with unresolved redistribution terms or unsupported runtime compatibility before evaluating speed/quality.

## Zen selection and shortcut implications

[Zen's official README](https://github.com/zen-browser/desktop/blob/dev/README.md) identifies it as Firefox-based. Its [shortcut guide](https://docs.zen-browser.app/user-manual/shortcuts) notes that inherited Firefox shortcuts are not completely represented in shortcut settings. Therefore a shortcut recorder cannot guarantee the absence of browser conflicts.

Avoid conflicts with [VoiceOver's Control–Option commands](https://support.apple.com/guide/voiceover/control-your-mac-with-keyboard-commands-vo2681/mac) and [macOS Speak Selection's Option–Escape default](https://support.apple.com/en-gb/guide/mac-help/mh27448/27/mac/27). [Zen Glance](https://docs.zen-browser.app/user-manual/glance) uses configurable modifier-click gestures, which matter for held-chord selection. Validate ordinary page text, editable fields, links/dragging, PDF text, release ordering, and Accessibility permission behavior in the actual supported Zen version. Public docs do not prove selected-text capture reliability.

## Automatic update checks versus installation

[Sparkle configuration](https://sparkle-project.github.io/documentation/customization/) distinguishes automatic checks from automatic download/install. It supports daily checks, disabled automatic installation, and a user-visible opt-out. The [Objective-C controller APIs](https://sparkle-project.github.io/documentation/api-reference/Classes/SPUStandardUpdaterController.html) establish a candidate native integration, not a tested GPUI/Rust adapter.

The [request API](https://sparkle-project.github.io/documentation/api-reference/Classes/SPUUpdater.html) documents a default User-Agent containing app/version and Sparkle/version. [System profiling](https://sparkle-project.github.io/documentation/system-profiling/) is optional and should remain off. Add no stable identifier or reading data. Ordinary network/IP metadata is still visible to the hosting provider.

[Sparkle's signing/setup documentation](https://sparkle-project.github.io/documentation/) describes HTTPS, Developer ID/notarization and Ed25519 archive signatures. Signed feed/release-note verification is a separate opt-in facility in Sparkle 2.9; explicitly select and validate the metadata-verification policy. [Custom updater UI](https://sparkle-project.github.io/documentation/custom-user-interfaces/) has distinct check/download/install/relaunch responsibilities. Automatic checking must neither stall reading nor authorize installation.

## Final frontend provenance check

A concrete Kokoro frontend path resolves the earlier pronunciation-data question without assuming that the generic release lexicon is licensed:

- The publisher's [hexgrad/Misaki pronunciation dataset](https://huggingface.co/datasets/hexgrad/misaki/tree/7a6093fd11124188077f2dc16397cc465466d59a) explicitly declares Apache-2.0 for the dataset. Pin revision `7a6093fd11124188077f2dc16397cc465466d59a` and its US/GB gold/silver dictionaries.
- [sherpa's preparation script](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/scripts/kokoro/v1.0/run.sh) obtains these dictionaries from hexgrad/Misaki. Its [lexicon generator](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/scripts/kokoro/v1.0/generate_lexicon_en.py) merges them into US/GB lexicons; the v1.1 generator uses the same implementation. Regenerate from this pinned, explicitly licensed dataset and retain its license and provenance/modification notice. Do not substitute the uncleared generic `lexicon-en-us.txt` asset.
- [The upstream Kokoro v1.1 model card](https://huggingface.co/hexgrad/Kokoro-82M-v1.1-zh/blob/01e7505bd6a7a2ac4975463114c3a7650a9f7218/README.md) declares Apache-2.0 and identifies Maple, Sol and Vale. The [voice generator](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/scripts/kokoro/v1.1-zh/generate_voices_bin.py) maps those names to IDs 0, 1 and 2. English reading correctness and listener suitability remain runtime acceptance checks.
- Pin sherpa v2.0.1 at `af2d8d81dbfa61c4cea4c568da78f3982edca005`; the converted model/voice bundle at `1355334be009ccce397060ee639a874137a128fa`. Model SHA256: `05d793700b0b507f760c183c0e39bdd6a97b43d32fdc7bf7b54a5179f1b11222`. Voices SHA256: `e64a5a581d8c2a350d848f51c3121657cd83aa07ed6109172177345874a7244c`. Verify these against actual downloaded bytes during implementation.
- Published model, voice table, tokens and US/GB lexicons total 180,319,265 bytes before runtime binaries. Regenerated lexicon sizes must be measured. This is a payload estimate, not an installed-app/RAM measurement.

[Upstream issue #4086](https://github.com/k2-fsa/sherpa-onnx/issues/4086) reported short-text NaN audio with older int8 artifacts on macOS ARM64. The pinned [quantization script](https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/scripts/kokoro/v1.1-zh/dynamic_quantization.py) excludes the implicated STFT convolutions. Validate short selections, finite PCM output and all accepted speeds; the source change is not proof that this project works.

## Available development host

Read-only hardware facts: Mac16,12; Apple M4; 16 GiB RAM; macOS 27.2, build 26B118. No benchmark was run. This host alone cannot certify the agreed M1/8GB/macOS14 performance baseline. Test results must identify the real machine/OS; baseline certification needs a matching test environment. No M1 results may be extrapolated or invented.

## Installed Handy screenshots

The user subsequently requested screenshots of the installed app and a closer copy of its styling/flow. See [handy-reference.md](handy-reference.md) and the three captured PNGs. These actual screenshots supersede the earlier generic palette; they do not change the GPUI or compact-only product requirements.
