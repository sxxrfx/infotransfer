# Handy reference — captured from the installed app

Captured 2026-10-29 from Handy v0.9.8 using native app screenshots:

- [General](../assets/references/handy-general.png)
- [Models](../assets/references/handy-models.png)
- [Advanced](../assets/references/handy-advanced.png)

The inspected flow was the configured settings application: General, Models and Advanced. No settings were changed, no model was downloaded, and no microphone recording or private history was opened. First-run onboarding and the live recording overlay were not captured from this session.

## User instruction

“take screen shot of the handy app. copy its styling and flow.”

This supersedes the earlier light/blue visual study. It changes the application styling/navigation, while the confirmed GPUI implementation, local TTS, compact-only player, privacy, and performance requirements remain in force.

## Reproduce

- Charcoal window and control surfaces, fine gray dividers, light text, muted secondary text, pink active/selected states.
- Left sidebar, native-looking title bar, grouped setting rows with right-aligned controls, rounded bordered groups.
- Persistent footer with the current speech choice on the left and update access on the right.
- Voice cards with an explicit selected state, adapted from Handy's active model card.
- Settings navigation: General → History → Voices → Advanced → About.
- A compact dark playback pill with pink output bars and state only. Esc cancels.

## Adapt to the agreed reader

Transcription shortcuts become read-selection and hold-selection bindings. The Models destination becomes Voices because the first release offers curated voices rather than an engine marketplace. Playback and system-default output replace microphone settings. History is encrypted text-only and opt-in. Post-processing, input-device selection, auto-paste and recording-specific controls are outside scope.

Keep InfoTransfer's identity. The General hand glyph is copied from Handy under its preserved MIT notice; other action icons use the matching Lucide release. Omit redundant descriptions, ubiquitous info icons, unsupported model-performance ratings, and decorative copy. Preserve the structure and interaction patterns with functional labels suited to speech playback.

## Artifact

[handy-reader.html](../prototypes/handy-reader.html) displays the three actual screenshots and the adapted interactive prototype. Sample voice/model/playback data is explicitly labeled; nothing is installed, recorded, uploaded or persisted. The HTML is a specification artifact, not a webview implementation.

## Native typography and icon pass

The native implementation now uses the v0.9.8 source at commit `14f6f0d31cb22a4268acfbddba3052820fd0a06e` as its reference, alongside the captured screens. Relevant upstream files:

- [Sidebar.tsx](https://github.com/cjpais/Handy/blob/v0.9.8/src/components/Sidebar.tsx): 150 px sidebar at the 15 px root size, 24 px icons, medium navigation labels and solid pink active rows.
- [App.css](https://github.com/cjpais/Handy/blob/v0.9.8/src/App.css): system sans typography, 15 px root, weight 400, antialiasing. Tailwind `text-sm` resolves to 13.125 px.
- [SettingsGroup.tsx](https://github.com/cjpais/Handy/blob/v0.9.8/src/components/ui/SettingsGroup.tsx) and [SettingContainer.tsx](https://github.com/cjpais/Handy/blob/v0.9.8/src/components/ui/SettingContainer.tsx): outlined groups, inner dividers, inset overline headings and aligned controls.
- [Button.tsx](https://github.com/cjpais/Handy/blob/v0.9.8/src/components/ui/Button.tsx): medium labels, neutral control surfaces and compact padding.
- [RecordingOverlay.css](https://github.com/cjpais/Handy/blob/v0.9.8/src/overlay/RecordingOverlay.css): compact charcoal surface, system font, pink signal bars and small controls.

InfoTransfer uses the system font at a 15 px root, 150 px sidebar, 39 px navigation rows with 24 px icons, 45 px setting rows, and 13.125 px setting/control text. Selected navigation uses Handy's translucent pink surface and light labels. Voice cards follow Handy's two-level model-card layout, 2 px outlines, 11.25 px corner radius, 60×5.625 px indicators, separate footer actions and whole-page scrolling. The 174×42 compact playback pill expands to 218 px during preparation while retaining 24 px corners. Its spinner occupies the left slot; the state label is independently centered.

Source motion values are implemented for controls (150 ms), card hover/press (200 ms, 1.01/0.99 scale), progress fill (300 ms), preparing progress (2 s pulse), and compact preparation width (460 ms). Reduced motion bypasses transitions and loops. These source values do not by themselves certify animation smoothness or pixel parity. Latest native interaction, download-state and overlay captures remain required.

The matching Lucide 0.542.0 assets and Handy hand notice are in `assets/icons/`. Icons are embedded vectors. Popup redraw stops while hidden, and audio-meter updates are capped around 30 Hz.

The latest user refinement makes the floating popup visual-only. Escape cancels the reading and hides it. The earlier interactive HTML mockup predates this refinement.
