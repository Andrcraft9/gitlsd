---
title: 'Make q mirror Escape'
type: 'feature'
created: '2026-09-09'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The default `q` binding quits immediately from every view, while Escape
backs out through focused preview and show panes and does not quit from the focused
log. This makes two expected exit keys behave inconsistently.

**Approach:** Make `q` use the same default back action as Escape, and make that action
quit only when the log screen itself has focus. Preserve Ctrl-C and configurable quit
bindings as unconditional quit actions.

</frozen-after-approval>

## Implementation Notes

- Changed the default `q` binding from `quit` to `back`, and made `back` quit only
  when the log pane itself is active. Explicit `quit` bindings, including Ctrl-C,
  remain unconditional.
- Added application and debug-frontend coverage for matching `q`/Escape transitions
  through preview focus, show diff, show explorer, and the focused log.
- Updated the controls documentation and marked the PLAN.md item complete.
- Made every explicitly configured `quit` binding win during text entry, while the
  default `q` remains available as text because it is now bound to `back`.
- Validation passed: `cargo fmt --all -- --check`, `cargo test --all-features`,
  strict Clippy for all targets/features, and `git diff --check`.

## Review Triage Log

- `false` — Ordinary `q` intentionally remains text while a search or command input
  has focus; README.md distinguishes this from Escape, and a unit test protects the
  ability to search for `q`.
- `patch` — Generalized the text-entry fast path so every explicitly configured
  `quit` key, not only Ctrl-based keys, remains unconditional as specified.
- `patch` — Added debug coverage proving `q` returns from Help to the log without
  quitting.
- `false` — The `in-progress` spec state was the required state during implementation
  and review; finalization now marks it `done` alongside PLAN.md.
