# Audit and roadmap — 2026-11-03

Sources: [SPEC.md](SPEC.md) (Q1–Q31), [decision-tree.md](decision-tree.md), [IMPLEMENTATION.md](IMPLEMENTATION.md), the requests made while building the app, and a read-only audit of `src/`, `native/`, `scripts/` and `site/` at `44c943f` (v0.2.0). `cargo test --no-default-features --lib --bins` passes (49 tests). Nothing below was measured on an M1 or with VoiceOver.

## Where the app stands against the spec

| Spec area | State |
| --- | --- |
| §2 Tap/hold activation, Esc, AX capture with text-marker fallback (Zen) | Done. Fn mode misses a tap after navigation keys (A7). Esc is captured whenever any notice shows (A6). |
| §3 One request, replace/stop/pause, no stale resurrection | Done and holds up under review. Output-loss recovery has gaps (A9). |
| §4 100,000-character limit, bounded chunks, developer reading | Limit enforced. Some chunks are silently dropped (A1). Several common forms are misread (B1–B8). |
| §5 Settings UI, compact player, reduced motion | Done. The meter redraws at display rate, and Preparing spins although the spec says static (A10). No VoiceOver pass, and text size is fixed. |
| §6 Encrypted opt-in history | **Not started.** Only an icon and an unused `Origin::History`. |
| §7 First launch, permissions, recovery | Permissions are rechecked. Errors are 4-second notices rather than lasting states with Retry or Open settings. Voice download cannot be cancelled while stalled and does not swap files atomically (A8). |
| §8 Updates | Signed manifest and signer check are good. No warning or cancel before relaunch (A3). No DMG or notarization, because there is no paid developer account. |
| §9 Targets | Measured on an M4 only: Paradee first PCM about 96 ms, Stop 16–20 ms, peak about 380 MB. M1/8 GB certification is open. |

The spec itself is out of date. SPEC.md still says eight voices, Kokoro/sherpa-onnx, Carbon key events and the slate palette, and §1 still excludes word highlighting while the README roadmap and the website demo show it. Update SPEC.md before the next feature pass, so the work below has a current contract.

## Audit findings

### A. Correctness, safety and reliability

| # | Finding | Where | Severity |
| --- | --- | --- | --- |
| A1 | A chunk whose text fails `prepare()` is skipped with no message. `speakable()` treats any error as "nothing to say". Affected: `CO₂`, `m²`, `½`, full-width digits, and replacements over 8,192 characters. Breaks §4 "never silently discard". | `spoken.rs:470`, `spoken.rs:742`, `reader.rs:746` | High |
| A2 | Signed without hardened runtime, so `DYLD_INSERT_LIBRARIES` can run code with InfoTransfer's Accessibility grant. Add `--options runtime`. The self-signed identity has no Team ID, so loading `libonnxruntime.dylib` may also need `disable-library-validation`; check that before shipping. | `scripts/package_app.py:110-117` | High |
| A3 | "Install and restart" downloads, installs and quits in one step, so active speech dies mid-sentence. There is no warning, no cancel, and the download cannot be cancelled. Breaks §8. | `ui.rs:726-787` | High |
| A4 | The update-signing key defaults to the same folder as the code-signing keychain and its plaintext password. One stolen backup holds both, which defeats the two-key defence in IMPLEMENTATION.md. | `scripts/release.py:12`, `scripts/local_signing.py:13-37` | Medium-high |
| A5 | One invalid dictionary or sound value fails `from_json`. All settings (voice, speed, key, dictionary) then reset to defaults and are overwritten on the next save. | `settings.rs:54-55`, `ui.rs:289-293` | Medium |
| A6 | Global Esc is registered for every notice, including "update available", and while a reading is paused, including the automatic lock/sleep pause. After waking the Mac, Esc in every app is swallowed. | `ui/player.rs:41-45`, `macos.m:228` | Medium |
| A7 | In Fn mode, arrows, Home/End, PgUp/PgDn and F-keys carry the Fn flag. The next Fn tap is ignored. | `macos.m:120`, `activation.rs:97-99` | Medium |
| A8 | Voice download: no per-read timeout, so Cancel can hang for up to an hour. Activation is two renames, not atomic, and `.previous` is never restored. A cleanup error after success is reported as a failure. No free-space check, and downloads cannot resume. | `assets.rs:210-290` | Medium |
| A9 | Output recovery stops retrying after 60 s while paused. Resuming without a device shows "Reading" with no sound. The output error expires after 4 s. | `reader.rs:518-530`, `reader.rs:882-919` | Medium |
| A10 | The speech thread polls every 2 ms while the 10 s queue is full, which is all the time, including overnight pauses. The 10 s queue was sized for Kokoro. With Paradee it makes speed, volume and EQ changes take up to 10 s to be heard, and allocates about 3.8 MB per reading. | `audio.rs:15`, `audio.rs:157-172` | Medium |
| A11 | Meter bars animate every frame (60–120 Hz) instead of about 30 Hz, and Preparing uses a spinner. | `ui/motion.rs:179`, `ui/player.rs:163-190` | Low-medium |
| A12 | `install.sh` trusts a checksum from the same release, accepts any signer, uses `rm -rf` then `ditto` (not atomic), and isn't wrapped against a truncated `curl \| sh` download. | `scripts/install.sh` | Medium |
| A13 | The update is verified in `/Applications/.InfoTransfer.update` and then swapped without a recheck. `is_dir()` follows symlinks. | `update.rs:214-256` | Medium (plausible) |
| A14 | Smaller issues: a failed chunk stops the whole reading and drops queued good audio. The pronunciation helper start has no timeout. A U+0000 in a selection truncates it. A password-field subrole lookup that times out falls through to reading. `speech-bench` uses a 100-character first chunk, the app 180. `release.py` builds from a dirty tree and `--clobber`s releases. | various | Low |

### B. Developer-reading misreadings (`spoken.rs`)

Primary use is reading prompt output and developer text, so these matter more than their individual size suggests.

| # | Input → current output |
| --- | --- |
| B1 | Hyphen next to a digit is read as "minus": `3-5`, `GPT-4` ("…tee minus four"), `SHA-256`, `COVID-19`, `x86-64`, `555-1234`. |
| B2 | All-caps words of 2–5 letters are spelled out: `DO NOT USE` → "dee oh en oh tee…", `HELLO`. |
| B3 | Plural acronyms are split wrongly: `APIs` → "eigh pee is", `URLs`, `PRs`, `CPUs`, `IDs`. |
| B4 | Years and decades: `1999` → "one thousand nine hundred…", `1990s` → "…ninety ess". |
| B5 | `11/03/2026` → "eleven over three over…", `24/7` → "twenty four over seven". |
| B6 | Compiler locations read as times: `main.rs:12:30` → "…twelve thirty". |
| B7 | Lowercase `10am` / `10 a.m.` read as "um" / "uh em". `Vitamin A is` → "vitamin uh is". |
| B8 | Case-insensitive dictionary matching is ASCII-only (`école` ≠ `ÉCOLE`). Built-in aliases rewrite user replacement text (`my nan` → "not a number"). |
| B9 | Smaller: `?` inside URLs gives question intonation, a spaced `2 ** 10` loses the operator, `5€` drops the currency, CRLF and U+2029 aren't paragraph breaks, and a hard 180-character cut can split `3.5 | GB`. |

### C. Test gaps

The reading controller has 3 tests and is hard-wired to the real engine. Replacement, stop/pause ordering, output loss and memory-saver timing are untested. Also untested: `assets::prepare` failure and rollback, the updater beyond manifest verification, the activation classifier's Fn edge cases, every script, the 510-token split, settings with one invalid field, and B1–B9.

## Plan

Sizes: S ≈ half a day, M ≈ 1–2 days, L ≈ a week. Each phase ends with `cargo test`, clippy, a packaged build and a short manual pass in Zen and Preview.

### Phase 0 — Fix what is broken (about 1 week)

**Status, 2026-11-03: done in code, except the Preparing spinner.** The spinner was kept because it is the Handy-style design you chose; reduced motion already makes it static. Automated tests, clippy and the offscreen render check pass. Hardened runtime was checked by signing `speech-bench` the same way: without the library-validation entitlement ONNX Runtime fails to load ("different Team IDs"); with it, it loads and `DYLD_INSERT_LIBRARIES` is ignored. Still to do by hand in the packaged app: Fn after arrow keys, Escape after lock/sleep, unplugging the output during a reading, the update flow against a real release, and the installer. `install.sh` now checks the signer, but install.sh, the zip and its checksum all still come from the same release.

Fix these before any new feature. Most are small and each one removes a way the app silently misbehaves.

1. **No silent drops (A1, A14).** Make `prepare()` read superscripts, vulgar fractions and full-width digits. Split "nothing to say" from "could not prepare", and on failure speak what can be spoken and show a lasting notice. Let one failed chunk skip forward instead of stopping the reading. S
2. **Update safety (A3, A13).** Split into Download → Ready → Install. If a reading is active, ask "Playback will stop" with a Cancel option, and make the download cancellable. Re-verify after the swap, or verify in place under a lock, and reject symlinks. M
3. **Signing hardening (A2, A4, A12).** Use hardened runtime and test that ONNX Runtime and the helper still load. Move the update key out of `signing/` (Keychain or an explicit path, with no default next to the keychain password). Wrap `install.sh` in `main()`, swap atomically, and check the expected signer. S–M
4. **Settings resilience (A5).** Validate field by field, drop only the bad dictionary entry or sound value, and keep the rest. Tell the user which item was reset and keep the `.bak`. S
5. **Escape and activation (A6, A7).** Register Esc only while a reading is playing or preparing. Notices don't take Esc, and a paused reading releases it. Clear the Fn block on any non-Fn key-down and ignore the Fn flag on navigation keys. S
6. **Audio pipeline (A9, A10, A11).** Shrink the queue to about 3 s, measure it with 180-character chunks, and keep the no-underrun result. Replace the 2 ms poll with a condvar or park/unpark. Keep retrying output while a reading is waiting for it, with a lasting "No audio output" state. Animate the meter at 30 Hz and make Preparing static. M
7. **Voice download (A8).** Add a read timeout, swap with a single rename of a version folder or a symlink switch, restore `.previous` at startup, check free space first, and resume downloads with HTTP range requests. M

### Phase 1 — Developer reading quality (about 1 week)

8. **Done 2026-11-03.** Bare four-digit numbers from 1100 to 2099 read as years unless a word like `port`, `PID` or `line` comes just before them. Fix B1–B9 with explicit rules: hyphen between letters and digits joins them; digit–digit ranges are read as "to" (but not phone numbers or ISO dates); all-caps words are spelled out only when the lexicon doesn't know them; plural acronyms; four-digit years and decades; slash dates and fractions (read in the locale's order, US by default); `file:line:col`; `am`/`pm` after a number; URL handling. M
9. **A golden corpus test**: about 300 inputs of developer prose (commit messages, stack traces, Markdown, CLI output and LLM answers) with expected spoken text, checked in. Add a property test that `prepare` + chunking never panics, keeps every alphanumeric character in order, and stays within the limit. M
10. **Markdown-aware reading for prompt output**: optional "Skip code blocks" (announce "code block, 12 lines"), read list items and headings with pauses, and skip table rules. Default off; it changes content, so it is a setting rather than a rule. M

### Phase 2 — Finish the v1 spec (2–3 weeks)

11. **Encrypted opt-in history (§6)**, the largest open spec item. Use AES-GCM or ChaCha20-Poly1305 with the key in Keychain. Save asynchronously when speech starts. Keep 100 entries / 30 days. Support delete, confirmed clear-all, and replay without duplicating or re-ageing entries. On Keychain failure show "not saved" with Retry, and never write plaintext. Add a History page in the sidebar. Test against a fake Keychain. L
12. **Lasting error states with actions (§7)**: Model ("Retry", "Download voices"), Permission ("Open settings"), Output ("No audio output"). These replace the 4-second notices for anything the user must act on. M
13. **A testable reading controller**: an engine trait with a fake, plus tests for replacement, stop, pause, late messages, output loss versus manual pause, and memory-saver timing. Do this before or alongside 11, since history replay goes through the controller. M
14. **First-launch guide (§7)**: one window in three steps (download voice, allow Accessibility, try a key), using the existing state checks. M
15. **Accessibility pass (§5)**: VoiceOver walk-through of every page, accessible names for custom controls, Dynamic Type or a text-size setting, a window that grows with text, and keyboard paths. Record the results the way `native-verification.json` does. M

### Phase 3 — New features (pick after Phase 2)

Spec §1 excludes some of these from v1. Each needs your decision before it is built.

| Feature | Why | Notes | Size |
| --- | --- | --- | --- |
| Follow-along highlighting (sentence first, then word) | README roadmap; the core dyslexia benefit | Excluded by spec §1. Sentence-level highlighting is feasible from chunk boundaries. Word-level needs the duration output from Paradee's predictor, and painting a highlight inside another app needs AX bounds for ranges, which Zen may not expose. Start with an optional caption line in the popup showing the current sentence. | L |
| Skip back / forward a sentence, and "read again" | Lost your place | Excluded by spec §1 ("sentence navigation"). Needs menu-bar actions or keys while reading. | M |
| More Heart-quality voices | You asked for alternatives, and only Heart passed | The voice catalog, picker and filters already exist. Each candidate needs the same Misaki-match and listening check. | M per candidate |
| Read clipboard by key | Apps that hide their selection | A separate key or double-tap, still deliberate (no automatic copy). | S |
| Per-app or per-content speed (prose vs code) | Developer use | Small once settings are field-validated. | S |
| Import/export dictionary | Share fixes for project terms | JSON file via a save panel. | S |
| Notarized DMG | No security warning | Blocked on a paid Apple Developer account. Until then keep the zip and installer. | S once unblocked |

### Phase 4 — Certification and release gates

16. M1/8 GB/macOS 14 run of `reading-bench` against the §9 targets (100 warm trials per bucket, 20 cold). Report misses rather than relaxing targets.
17. Full acceptance matrix (§9): Notes and VS Code, multiple displays, Spaces and full screen, lock/sleep, Bluetooth handoff.
18. Release hygiene: `release.py` refuses a dirty tree or an untagged commit and won't `--clobber`. A CI job runs test, clippy and fmt on PRs. Bump the version per release.

## Docs and site fixes

- Bring SPEC.md up to date: one Paradee voice, ONNX Runtime C API, NSEvent observers, teal palette, zip plus self-signed distribution, and which v1 exclusions are being reconsidered.
- The website hero demo shows word-by-word highlighting, which the app does not do. Change it until the feature ships.
- `~/.cargo/config.toml` sets `target-dir = "~/.cargo/target"`. Cargo does not expand `~`, so builds go to `/Users/sxxrfx/~/.cargo/target` (5.8 GB). Use an absolute path.

## Decisions needed

1. Should follow-along highlighting and sentence navigation move into scope? Spec §1 excludes them, and the README roadmap promises highlighting.
2. History: build it as specified (encrypted, 100 entries / 30 days), or drop it from the spec?
3. Slash dates: US order (`11/03` = November 3) by default, or follow the macOS region setting?
4. Is a paid Apple Developer account planned? It decides whether DMG and notarization stay in the plan.
