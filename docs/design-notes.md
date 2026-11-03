> Native implementation now supersedes this study. The popup contains only a meter/state, with no buttons. Escape cancels the reading and hides it. The HTML artifacts below preserve the earlier spec exploration.

# UI study — captured Handy styling and flow

This is a visual prototype, not the macOS application. Playback, downloads, permissions, and settings are simulated. No speech model, shortcut listener, or text capture is installed.

The native implementation framework is GPUI. HTML remains a visual/interaction study rather than a proposed webview implementation. The user explicitly requested screenshots of installed Handy and a copy of its styling/flow. The current reference is `../prototypes/handy-reader.html`. Compact-only playback, the output visualizer and separate settings/history remain confirmed.

## Reference

[Handy](https://github.com/cjpais/Handy) provides the settings/navigation reference. Actual General, Models and Advanced screens from installed v0.9.8 are saved in `../assets/references/`; see `handy-reference.md`. The upstream overlay screenshot informed the earlier compact shape, but a live recording overlay was not captured in this session. The TTS player represents speech output rather than microphone activity.

## Design plan

- Palette, matched to captured Handy: window #2c2b29, controls #363533, divider #44433f, primary text #f3f2f1, muted text #aaa8a5, selected pink #c17c9f, output accent #ee88bb.
- Type: macOS system UI font, compact 12–14px settings labels and controls, quiet group headings, heavier app name. No special font is presented as a treatment for dyslexia.
- Layout: left sidebar, grouped rows with right-aligned controls, selected voice cards and persistent voice/update footer. General → History → Voices → Advanced → About. A separate compact playback pill has an output meter, short state label and playback controls.
- Purpose: the output meter shows that playback is active without displaying selected text. Status and controls remain understandable when motion is reduced or audio is quiet.
- Motion: no shortcut entrance/exit animation. The native output meter will sample playback PCM, update at a capped rate, freeze on pause, and become static for reduced motion. The HTML uses explicitly labeled synthetic data because no speech backend is connected.

## Review against the brief

The structure, surfaces, selected states and navigation follow Handy. Labels/control groups are adapted to the reader: voices instead of transcription models, read/hold bindings instead of recording, output playback instead of microphone settings, and opt-in text-only history. Handy's logo, Post Process destination, redundant descriptions, ubiquitous info icons and unmeasured accuracy/speed ratings are omitted. The player has no guessed total duration. Screenshot-gallery, preview-state and reduced-motion controls are review tools outside the product UI. The mockup neither claims actual audio output nor measures latency.

## Open design choices

Rounds 1–4 settle the product and recovery policies. Exact validated model/voice artifacts follow the accepted evaluation rule. The current styling direction is captured Handy; older light/blue studies are superseded. The complete implementation spec is in `SPEC.md`, awaiting final shared-understanding confirmation through Q31.

## How to inspect

Open `../prototypes/handy-reader.html` directly. Switch the captured reference screens, navigate the application sidebar, record example shortcuts within the page, choose voices, change speed and inspect prospective history opt-in. Playback/recovery controls are simulated and do not access the microphone, clipboard, OS permissions or global shortcuts. No data or preferences persist. Older `compact-player.html` and `reader-ui.html` remain historical studies.

## Validation

The Handy-style study renders at 728px and 360px without console errors. Checks pass for reference gallery switching, sidebar/footer routing, voice/speed synchronization, recording shortcuts and committing a held chord on release, player/redraw states, prospective history opt-in, disabling without deleting existing history, clear-all confirmation, explicit clipboard simulation, reduced motion, unique IDs and no narrow-width overflow. Meter scheduling uses a deterministic frame driver against the actual control code. These checks cover the mockup, not native capture, actual audio, model performance, or macOS accessibility behavior.
