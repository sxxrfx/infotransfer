# Discovery decision tree

Status: Q1–Q30 answered. Product discovery is complete; [SPEC.md](SPEC.md) is consolidated and ready for final review. Q31 is approved; native implementation and continued performance iteration are authorized. Installed Handy screenshots and matching styling/flow are included.

Interview delivery preference: the user requests the T3 questions API for questions.

## Settled by the initial request

- macOS application.
- Deliberate keyboard-activated selected-text reading.
- Local inference with open-source models and multiple voices.
- Startup responsiveness is the highest priority; model size matters.
- Visual playback feedback and reading accessibility matter.
- Detailed discovery/specification before app implementation.
- Handy-inspired settings and multiple playback mockups.
- Learn through the build, then complete implementation in one sustained pass after agreement.

## Confirmed round 1 answers

| ID | Decision | User's answer | State |
| --- | --- | --- | --- |
| Q1 | Initial audience/release intent | Public free/open-source release | Confirmed |
| Q2 | Activation methods | Both select-then-shortcut and hold-while-selecting in v1 | Confirmed |
| Q3 | Processor family | Apple Silicon only | Confirmed; Q7 settled the minimum baseline |
| Q4 | Required languages | English first | Confirmed |
| Q5 | UI framework | GPUI required | Confirmed; supersedes the Tauri recommendation |
| Q6 | Reading history | Opt-in history | Confirmed; Q12 settled the policy and Q16 permits automatic update checks |

The interface offers prototype preferences but does not record them as spec approval.

## Confirmed round 2 answers

| ID | Decision | User's answer | State |
| --- | --- | --- | --- |
| Q7 | Concrete baseline | M1, 8 GB RAM, macOS 14+ | Confirmed |
| Q8 | First certified apps | Zen browser | Confirmed; Q17 settles the complete matrix |
| Q9 | Hold-gesture commit | Speak once on chord release; no speech during selection or without a new valid selection | Confirmed |
| Q10 | Repeated activation/interruption | Replace current reading, clear queued output, restart repeated selection, separate pause/resume and stop, no queue | Confirmed |
| Q11 | Unsupported selections | Explicit Read clipboard fallback; no automatic copy-key simulation or OCR | Confirmed |
| Q12 | History policy | Off by default; encrypted local text/timestamp/voice/speed; no audio/source metadata; regenerate on replay; 100 entries/30 days; delete/clear; disabling stops new saves | Confirmed |
| Q13 | Playback surface | Only compact UI with audio visualizer | Confirmed; expanded text/sentence view superseded |
| Q14 | Readiness | Selected model stays loaded by default; optional memory-saving mode | Confirmed |
| Q15 | Distribution | Signed/notarized DMG, public source, build instructions | Confirmed; release credentials remain a later dependency |
| Q16 | Network | Allow automatic update checks | Confirmed; does not authorize automatic install or telemetry |

The macOS 14 floor is an accepted product support target, not a claim that GPUI itself requires macOS 14. Exact model/runtime and frontend licenses still need verification before redistribution.

## Confirmed round 3 answers

Answers were collected through structured question cards. Runtime internals and native adapters remain engineering responsibilities.

| ID | Decision | User's answer | State |
| --- | --- | --- | --- |
| Q17 | App list | Zen primary; Preview selectable PDFs, Notes and VS Code secondary; Safari/Chrome leave required list | Confirmed |
| Q18 | Responsiveness | p95 feedback ≤100 ms, warm speech ≤500 ms, stop ≤100 ms, cold speech ≤3 s on baseline M1/built-in audio; Bluetooth reported separately | Confirmed targets, not measured results |
| Q19 | Resources | Speech assets ≤200 MB; installed ≤500 MB; ready RAM ≤500 MB; peak ≤1 GB; history/temp installer/update staging excluded | Confirmed targets, not measured results |
| Q20 | Voice/speed | Three curated English voices, local previews, 0.75×–2×; changes at next sentence/chunk boundary | Confirmed |
| Q21 | Compact placement | Bottom center of source display above Dock; no focus stealing; hide on completion/stop; menu bar settings/history; no text expansion | Confirmed |
| Q22 | Interruptions | Continue through audio-device changes | Confirmed change; can become speaker playback. Lock/sleep pause is explicitly included in the final-review defaults |
| Q23 | Updates | Automatic launch/daily checks; no reading data/stable ID/profiling; notify; manual download/install; opt-out switch | Confirmed |
| Q24 | Bindings | Configure bindings at setup; read and hold gestures plus pause/stop; conflict feedback where detectable; actual Zen tests | Confirmed |
| Q25 | License | Apache-2.0 original code; preserve compatible third-party terms/notices | Confirmed |

Targets are not measured results. Missed targets must be reported, not silently relaxed. Cold startup is measured with an unloaded speech model and a ready application; initial process launch and model downloads are separate measurements. Final model selection depends on accepted budgets and a fully documented redistributable frontend/runtime/artifact chain.

## Additional confirmed instruction — Handy styling and flow

The user requested screenshots of installed Handy and instructed us to copy its styling and flow. Actual General, Models and Advanced screens from v0.9.8 are saved in `assets/references/`. `prototypes/handy-reader.html` is now the current design reference; earlier blue/light studies are superseded. Adapt the sidebar to General/History/Voices/Advanced/About and retain the compact-only output visualizer. No native app implementation is authorized before final spec agreement.

## Confirmed round 4 answers

The user accepted all five recommendations through structured question cards.

| ID | Decision | User's answer | State |
| --- | --- | --- | --- |
| Q26 | Model-selection policy | Pinned Kokoro/sherpa plus verified licensed pronunciation data first; three voices by correctness/listening; smaller compatible alternative under identical requirements; report misses | Confirmed |
| Q27 | Long text | 100,000-character limit; no extra prompt; bounded incremental synthesis; request smaller selection above limit; preserve content without rewriting/translation | Confirmed |
| Q28 | Memory-saving behavior | Five idle minutes before unload when enabled; active/paused readings stay loaded; normal mode stays ready | Confirmed |
| Q29 | History failure/recovery | Async save when speech begins; continue on Keychain/storage failure; report unsaved history/retry; no plaintext fallback; confirm clear-all | Confirmed |
| Q30 | Setup/runtime recovery | Guided assets/access/bindings; settings close keeps service; Quit stops speech; login launch off; actionable local recovery without silent fallback | Confirmed |

## Final confirmation — Q31

The complete [implementation specification](SPEC.md) is now the reviewable result. No further product questionnaire is planned unless the user requests changes or a new fact invalidates a settled requirement.

| ID | Decision | Recommended answer | State |
| --- | --- | --- | --- |
| Q31 | Confirm shared understanding and start implementation | Approve SPEC.md, including the explicit final-review defaults and external release gates, and authorize the native build/validation pass | Confirmed — approve and build |

The final review explicitly includes lock/sleep pause versus continuing device handoff; invalid held-gesture cancellation; voice preview replacing active playback without saving history; history replay without duplicates/age reset; setup completion after at least one activation binding; and actual M1 validation/signing/feed dependencies. These defaults are visible in the spec and approval card, not silently attributed to earlier answers.

## Resolved design tree and implementation validation gates

```text
Initial requirements
├─ Public free/open-source release (confirmed Q1)
│  ├─ direct distribution (confirmed Q15) + Apache-2.0 (confirmed Q25) → exact notices / release credentials
│  └─ Zen + Preview/Notes/VS Code (confirmed Q17) → capture acceptance matrix for both gestures
├─ Both activation methods (confirmed Q2)
│  ├─ hold release (confirmed Q9) + setup bindings (confirmed Q24) → Zen modifier conflicts / permission flow
│  ├─ explicit clipboard fallback (confirmed Q11) → failure/recovery UI
│  └─ replace/no queue (confirmed Q10) + device continuation (confirmed Q22) + long text (confirmed Q27) → behavioral validation
├─ Apple Silicon + English (confirmed Q3/Q4)
│  ├─ M1/8GB/macOS14+ + accepted budgets (confirmed Q18/Q19) → actual baseline validation
│  └─ Apache-2.0 + three English voices (confirmed Q20/Q25) + model rule/memory mode (confirmed Q26/Q28) → model evaluation
├─ GPUI required (confirmed Q5)
│  ├─ pinned accessible GPUI revision / native menu bar adapter / runtime packaging
│  ├─ compact + output visualizer + placement (confirmed Q13/Q21) → captured Handy styling / accessible states
│  └─ build workflow / learning handover / VoiceOver validation
├─ Opt-in history (confirmed Q6)
│  ├─ encrypted text-only policy (confirmed Q12) + failure recovery (confirmed Q29) → persistence/replay tests
│  └─ automatic daily checks/manual install (confirmed Q23) → verified native updater / release feed
└─ Finalized capabilities
   ├─ compact controls / output-meter semantics / reduced motion
   ├─ placement, focus, displays and full-screen behavior
   ├─ voices, speed and exact pause/resume behavior
   ├─ sleep/lock/output-device behavior and useful errors
   ├─ acceptance matrix and complete implementation scope
   └─ Q31 final shared-understanding confirmation → implement and validate
```

Exact model/backend/artifact selection is delegated through the accepted Q26 validation rule. Prototype sample voices do not prove quality/performance or freeze an unvalidated artifact. Hardware certification, native permissions, signing and updater configuration remain real validation/release dependencies, not unanswered product preferences.

## Likely rounds

1. Foundations: completed; six answers recorded above.
2. Workflow and operating policy: completed; ten answers recorded above.
3. Budgets and product details: completed; nine answers recorded above.
4. Model/long-text/recovery policies: completed; five answers recorded above.
5. Final review: complete spec available; Q31 approved. Implement and continue iterating toward the accepted performance goal.

These rounds are a guide, not a fixed questionnaire. Recompute the frontier after each response; do not treat silence as agreement.
