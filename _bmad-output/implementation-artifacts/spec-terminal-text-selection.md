---
title: 'Support terminal-native text selection'
type: 'feature'
created: '2026-09-06'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: 'cba13b4aeb1e978f208bfff8f15495cde43b16be'
context: ['{project-root}/README.md', '{project-root}/ARCHITECTURE.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The interactive frontend captures mouse input, so users cannot normally drag-select and copy text rendered in the log, preview, help, or status/input areas.

**Approach:** Give mouse ownership back to the terminal and render the interface as terminal text so every visible view can be selected and copied with ordinary terminal gestures.

## Boundaries & Constraints

**Always:** Preserve keyboard navigation, search, commands, ANSI-derived styling, split-view behavior, resizing, terminal cleanup on success and error, and the deterministic debug frontend. Make log, preview, help, and status/input text selectable through the terminal without requiring an application-specific selection mode.

**Never:** Implement an internal clipboard, selection state machine, or platform-specific copy command; send terminal drag events to the application; change the application state model or Git output semantics; promise selection of content that is not currently rendered.

**Decision:** Keep alternate-screen rendering for the first implementation and remove application mouse capture. If its selection or copy behavior is unsatisfactory in actual use, normal-buffer/inline rendering remains the fallback for a follow-up iteration.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Select view text | User drags across visible log, preview, help, or status/input text | The terminal selects the exact rendered text for its normal copy operation | Application state does not change |
| Navigate without mouse capture | User uses configured keys in any view | Existing focus, scrolling, search, command, and quit behavior remains intact | Terminal state is restored on failure or exit |
| Resize | Terminal dimensions change while the interface is active | Layout redraws at the new size without stale text corrupting selectable output | Continue using the latest valid terminal dimensions |

</frozen-after-approval>

## Code Map

- `src/ui.rs::run` -- interactive event loop; stop dispatching captured mouse events while preserving keyboard dispatch and resize redraws.
- `src/ui.rs::TerminalSession` -- owns raw mode, alternate-screen selection, mouse capture, terminal construction, and cleanup; this is the behavior boundary that must return selection to the host terminal.
- `src/ui.rs::render`, `styled_line`, `scrolled_line` -- existing Ratatui text rendering and ANSI-to-style conversion should be reused; do not move presentation behavior into `App`.
- `src/ui.rs::tests` -- TestBackend coverage already verifies textual log, preview, help, status, styling, and offsets; replace obsolete mouse-translation coverage and add lifecycle/output-mode assertions where feasible.
- `README.md` -- replace application mouse-wheel claims with native selection/copy behavior and document any wheel/scrollback consequence.
- `ARCHITECTURE.md` -- update terminal-edge responsibilities if the frontend moves from alternate-screen to inline output; keep the core/frontend boundary unchanged.
- `PLAN.md` -- mark the improvement complete after implementation and verification.

## Tasks & Acceptance

**Execution:**
- [x] `src/ui.rs` -- release mouse ownership to the terminal and implement the approved text-output lifetime while retaining keyboard interaction, rendering, resize behavior, and reliable cleanup.
- [x] `src/ui.rs` tests -- cover the selected terminal lifecycle/output mode and retain textual rendering regressions across all views.
- [x] `README.md`, `ARCHITECTURE.md`, `PLAN.md` -- document the resulting interaction model, architecture impact, and completed plan item.

**Acceptance Criteria:**
- Given any interactive view with visible text, when the user drag-selects and invokes the terminal's copy operation, then the copied value matches the rendered text and the application selection/focus does not move.
- Given mouse capture is disabled, when the user operates log, preview, help, search, and command flows with the keyboard, then existing behavior remains available.
- Given startup succeeds or fails partway, when the application exits, then raw mode, cursor visibility, and any selected screen/viewport mode are restored correctly.
- Given the debug interface is run, when scripted interactions execute, then its stable plain-text snapshots remain unchanged except for intentional documentation-independent effects.

## Implementation Notes

## Spec Change Log

## Review Triage Log

- medium — `TerminalSession::start` did not clear mouse capture that a prior program might have left enabled, so terminal selection could remain unavailable; route: patch.
- medium — removing the mouse-event unit test left no executable regression test for interactive terminal lifecycle output; route: patch.
- low — real-terminal drag selection cannot run in this environment; the spec retains its manual smoke check, so no code action is possible here; rejected as environment-limited verification.
- low — the README did not state that alternate-screen sessions normally do not provide scrollback for terminal-owned wheel input; route: patch.
- medium — verification-gap review independently confirmed there was no executable assertion of mouse-capture-free setup/cleanup output; grouped with the terminal lifecycle test patch.

## Design Notes

Crossterm mouse capture cannot coexist with ordinary unmodified terminal drag-selection: capture redirects the gesture to the application. The feature therefore removes that protocol and its wheel-event translation. Keyboard scrolling remains authoritative; uncaptured wheel behavior belongs to the terminal emulator and must not be treated as a portable application control.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: no formatting diff.
- `cargo clippy --all-targets --all-features -- -D warnings` -- expected: no warnings.
- `cargo test --all-targets --all-features` -- expected: all unit and fixture-backed debug tests pass.

**Manual checks (if no CLI):**
- In a real terminal, drag-select and copy text from log, preview, help, and status/input, resize once, then quit; selection must not alter app state and terminal state must be clean afterward.
