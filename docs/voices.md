# Voice catalog

The default eight presets come from the pinned Apache-2.0 Kitten nano v0.1 voice table. Their display names are InfoTransfer aliases, not upstream speaker identities. Language and male/female labels follow the model's metadata; accent or quality claims are not inferred from the preset IDs. Optional Heart retains the original Kokoro `af_heart` identity. The catalog is defined in `src/voices.rs`.

| Display name | Information | Source preset | Index |
| --- | --- | --- | --- |
| Arthur | English · Male | expr-voice-2-m | 0 |
| Clara | English · Female | expr-voice-2-f | 1 |
| Elliot | English · Male | expr-voice-3-m | 2 |
| Maya | English · Female | expr-voice-3-f | 3 |
| Theo | English · Male | expr-voice-4-m | 4 |
| Nora | English · Female | expr-voice-4-f | 5 |
| Owen | English · Male | expr-voice-5-m | 6 |
| Ruby | English · Female | expr-voice-5-f | 7 |
| Heart | American English · Female | af_heart, Kokoro v1.0 speaker 3 | 8 |

Every installed preset has a local preview. The same model and 8,192-byte style table serve the eight Kitten voices; switching between them does not download or reload weights. The native conversion check covers those eight at 0.75×, 1× and 2× over three synthetic inputs, comparing exact seeded PCM digests against the original graph. This is not a listening-quality score.

Heart is an explicit 155,030,000-byte optional download from a pinned Kokoro v1.0 conversion; the legacy v1.1-zh experiment does not contain it. Its graph, complete style table, tokens and pronunciation data are hash-verified. Only one engine is loaded at a time. Switching between Kitten and Heart can therefore require loading an engine. A distilled Heart-derived model is not substituted for the original voice.

Preview replaces active playback without changing the saved voice and creates no history. Escape cancels it and hides the button-free popup. Filtering and sorting preserve original catalog IDs, selected voice and active playback. Missing ratings sort last. Naturalness bars display explicitly labeled UTMOS predictions from 12 matched passages, rather than a claimed human listening score. The current native frontend skips some unsupported words; these ratings describe that imperfect app pipeline. See [the 21-voice screening report](performance/voice-evaluation/REPORT.md), including retained audition recordings, uncertainty and model/frontend provenance. Speed bars use per-voice M4 startup measurements when available, with full measurement scope available in their descriptions and hover details. Neither the bars nor this catalog certify M1 performance.
