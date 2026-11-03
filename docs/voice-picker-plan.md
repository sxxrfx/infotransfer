# Voice picker refinement

The voice catalog will retain stable IDs while filters and sorting change only the displayed order. The existing eight Kitten presets remain available; authentic Heart adds a separately downloaded Kokoro v1.0 configuration, preset 3 (`af_heart`). The earlier Kokoro v1.1-zh experiment does not contain Heart.

## Visual reference

Handy v0.9.8, commit `14f6f0d31cb22a4268acfbddba3052820fd0a06e`, uses continuous pink pill tracks for its model indicators: [ModelCard.tsx](https://github.com/cjpais/Handy/blob/v0.9.8/src/components/onboarding/ModelCard.tsx#L199). Native dimensions at Handy's 15 px root are 60 × 5.625 px, label size 11.25 px, label gap 7.5 px and row separation 3.75 px. The track is mid-gray at 20% opacity and the dark-theme fill is `#f28cbb`.

Retain the existing charcoal surfaces, system font and pink selected state. Use Handy’s two-level cards: identity and two indicators above a divider; model and Preview below. The selection target includes the identity region; Preview remains separate.

The toolbar starts above the list; both scroll together, matching Handy. Use a gender filter, Installed toggle and a genuine sort menu. Handy has search/capability/language filters but no user sort menu in this view; sorting is an InfoTransfer adaptation. Do not add an English-only language filter.

## Evidence and behavior

- Naturalness uses **Predicted** labels from the retained 12-passage UTMOS evaluation. These are not human listening judgments. The native frontend currently skips unsupported words; see the evaluation report. Unknown scores remain Unrated.
- Speed indicators require comparable per-voice measurements. Existing typical-text latency covers Arthur only. Numerical equivalence across eight presets establishes neither speed nor speech quality.
- Exact speed details must identify startup latency, percentile, hardware and software-render scope. Keep this distinct from the reading-rate setting.
- Unknown values sort last, with name as the stable tie-breaker. Never represent unknown as a zero-quality voice.
- Filtering/sorting must preserve selection and active playback. Empty results retain the toolbar and provide **Clear filters**.
- Preview auditions a voice without changing the saved selection. It replaces current playback and saves no history. Escape cancels, hides the popup and leaves nothing resumable.
- Only one inference engine is loaded at a time. Model readiness, errors, repair and downloads belong to the relevant model, including after rapid changes or cancellation.

## Critic findings incorporated

Two independent read-only delegates reviewed the actual Handy code, current GPUI picker and benchmark evidence. Both rejected invented ratings and highlighted missing per-voice latency evidence. They requested compact rows, larger selection targets, an always-accessible toolbar, stable IDs, unknown-last sorting, temporary previews and engine-aware readiness. Validate these behaviors in the native app at the current 680 × 570 minimum size and larger windows.
