---
title: 'Horizontal scrolling'
type: 'feature'
created: '2026-09-05'
status: 'done'
baseline_commit: 'bb13dd751361cdf64ecd4571046499ca9bd5dea0'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Long log, preview, and help lines are clipped at the terminal edge with no way to reveal their right-hand content.

**Approach:** Add configurable horizontal navigation so the active textual pane can be panned left and right while preserving existing vertical navigation and focus behavior.

**Decision:** Left and Right arrow keys are the default horizontal-scroll controls.

## Boundaries & Constraints

**Always:** Keep input frontend-independent through `Key` and `Action`; retain independent horizontal offsets per applicable view; preserve existing vertical navigation, searches, preview focus, and deterministic debug output.

**Never:** Alter Git output, wrap lines merely to expose clipped text, or change the log selection when scrolling a focused preview or help.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Log navigation | Horizontal-scroll action while log is active | The log rows pan without changing selected commit | Left movement clamps at column zero |
| Preview navigation | Horizontal-scroll action while preview is focused | Only preview content pans; its vertical offset and log selection remain unchanged | Left movement clamps at column zero |
| Help navigation | Horizontal-scroll action on Help | Help text pans without changing the screen or log selection | Left movement clamps at column zero |
| Configuration/debug | Custom binding and scripted key | Binding parses, appears in effective help, and drives the same state machine | Unknown bindings retain existing configuration errors |

</frozen-after-approval>

## Code Map

- `src/config.rs` -- `Key`, `Action`, default bindings, parser, and effective-help generation; extend the shared vocabulary rather than handling keys only in the terminal UI.
- `src/ui.rs` -- `translate_key` maps Crossterm keys, and `Paragraph::scroll` currently receives a zero horizontal component; apply view-specific horizontal offsets and retain `ListState` behavior.
- `src/app.rs` -- owns help and preview vertical offsets plus focus/screen dispatch; add state transitions for horizontal offsets without coupling to Ratatui.
- `src/debug.rs` -- deterministic snapshots expose state, so include any new offsets for scriptable regression coverage.
- `README.md` -- controls and supported configured key names/action vocabulary must describe the new navigation.
- `ARCHITECTURE.md` -- documents core ownership of scrolling state; update if horizontal offsets become part of that contract.

## Tasks & Acceptance

**Execution:**

- [x] `src/config.rs` -- add horizontal key/action vocabulary, defaults, parsing, and configuration-help coverage.
- [x] `src/app.rs` -- model per-view horizontal scrolling and test focus/screen routing and lower-bound clamping.
- [x] `src/ui.rs` -- translate the chosen terminal keys, render all applicable panes at their horizontal offsets, and test visual output.
- [x] `src/debug.rs` -- expose horizontal state and test scripted navigation.
- [x] `README.md`, `ARCHITECTURE.md` -- document the controls and state responsibility.

**Acceptance Criteria:**

- Given a line wider than its pane, when the user scrolls right and then left, then the hidden right-hand content becomes visible and the view returns to column zero without affecting vertical state.
- Given a configured horizontal-scroll binding, when it is pressed, then it has the same effect as the default binding and appears in effective help.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| Finding | Verdict | Evidence and route |
| --- | --- | --- |
| Log slicing counts Unicode scalars instead of terminal columns | medium | Verified in `scrolled_line`: a width-two grapheme is removed by one right-scroll step. This makes common non-ASCII log text pan farther than preview/help. Patch with grapheme- and display-width-aware slicing. |
| Log slicing loses tab-stop context | low | The stated alignment issue is real for tab-containing custom log formats, but it is unlikely in everyday one-line Git log use and needs non-trivial renderer logic. Rejected. |
| Horizontal offsets are unbounded | low | Offsets can grow indefinitely, but recovery is always available with Left and a correct viewport-aware bound would add new layout/state coupling. Rejected as an unlikely everyday defect. |
| Preview horizontal offset survives selected-commit reload | medium | Verified in `reload_preview`: vertical offset resets while horizontal offset does not, so a newly selected short preview can be blank. Patch by resetting both preview offsets on reload. |
| Help horizontal offset survives reopening Help | medium | Verified in `Action::Help`: only `help_offset` resets. Patch by resetting the matching horizontal offset. |
| Paragraph offsets saturate at `u16::MAX` | low | The mismatch is real only after more than 65,535 individual scroll actions, which is not an everyday scenario. Rejected. |
| Edge-case review: wide/combining Unicode before a log offset | medium | Verified duplicate of scalar-counting issue: wide/combining text cannot follow the display-column semantics of Paragraph scrolling. Patch with the same renderer correction. |
| No display-column/grapheme regression coverage | low | The gap is verified: only ASCII log scrolling is tested. Patch with the renderer correction’s Unicode regression tests. |
| No regression test combines log scrolling, Git styling, and selection highlight | low | The gap is verified: existing styling and selection tests use offset zero. Patch with a focused renderer assertion. |

## Design Notes

The log renderer uses `List`, which does not have the `Paragraph` horizontal-scroll API used by preview and help. The implementation must choose a presentation-level solution that preserves ANSI styling and selection rendering rather than preprocessing Git output in the application core.

## Verification

**Commands:**

- `cargo fmt --check` -- formatting is clean.
- `cargo clippy --all-targets --all-features -- -D warnings` -- no lint warnings.
- `cargo test --all-targets --all-features` -- state, renderer, and debug behavior pass.
