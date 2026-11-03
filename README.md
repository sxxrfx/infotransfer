<img src="assets/icon.svg" width="64" height="64" alt="">

# InfoTransfer

Created in [T3 Code](https://t3.codes).

A native macOS selected-text reader built with Rust and GPUI. Implementation is in progress against the approved specification.

The app reads selected text aloud with local speech: eight fast default voices plus optional Heart, Fn/Globe or right Option tap/hold activation, Esc to cancel, and a menu bar item for clipboard reading, pause and stop. Encrypted history, the updater and a public signed release are not built yet. See [the specification](docs/SPEC.md).

Build and install on Apple Silicon with macOS 14+. No Apple developer account is needed:

```sh
python3 scripts/prepare_pronunciation.py   # once
python3 scripts/local_signing.py --setup   # once
python3 scripts/package_app.py --install
```

Then open InfoTransfer and allow it under System Settings → Privacy & Security → Accessibility. If an older InfoTransfer entry is listed, remove it first. This is needed once: rebuilds keep the permission because they share one self-signed identity, stored in its own keychain under `~/Library/Application Support/InfoTransfer/signing`. The build runs only on Macs that trust it locally; sharing it publicly would need Developer ID notarization.

Check changes with:

```sh
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run --release --features visual-test --bin render-check
```

The packager expects the verified sherpa-onnx v2.0.1 arm64 runtime under `resources/runtime` and the prepared pronunciation utility under `resources/phonemizer`. Voice setup downloads pinned assets and prepares the CPU graph locally. The native Kitten/Heart voices do not require Python in the installed app; the optional `--experimental` voices use separately configured local Python runtimes. The pronunciation build uses Python, CMake and Ninja during development, and bundles a separate GPL-3.0-or-later utility with its license and corresponding source. See [speech notices](resources/SPEECH-NOTICES.txt).

`python3 scripts/package_app.py --ui-preview` builds an ad-hoc preview at `.cache/ui-preview/InfoTransfer.app`; it does not keep Accessibility access across rebuilds. Packaging refuses while any InfoTransfer bundle is running. Python model voices run a locally configured program, so they are left out unless you package with `--experimental`.

Dictionary settings support custom whole-word replacements, case matching and local previews. Common contractions expand automatically. Developer text is prepared before every voice backend: numbers and units are spoken, syntax punctuation is skipped, and sentence/paragraph padding defaults to zero. Model-generated prosody remains intact. [The correctness summary](docs/performance/developer-reading/correctness-summary.json) records the latest recording check; some experimental voices still mispronounce acronyms.

See [implementation and measurement notes](docs/IMPLEMENTATION.md) for exact pins, commands and limitations. Performance results currently apply to M4 software measurements and do not certify M1 or acoustic latency.

- [Implementation specification](docs/SPEC.md)
- [Decision record and final confirmation](docs/decision-tree.md)
- [Feasibility research](docs/research.md)
- [Handy styling and flow study](prototypes/handy-reader.html) — current direction, with screenshots of installed Handy and a simulated interactive prototype.
- [Handy reference notes](docs/handy-reference.md)
- [Earlier compact player study](prototypes/compact-player.html) — superseded styling.
- [Initial UI alternatives](prototypes/reader-ui.html) — superseded exploration.
- [UI design notes](docs/design-notes.md)
