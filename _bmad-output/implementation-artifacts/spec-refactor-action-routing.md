---
title: 'Refactor action routing'
type: 'refactor'
created: '2026-09-09'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context:
  - '{project-root}/ARCHITECTURE.md'
  - '{project-root}/REVIEW.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `App::dispatch` mixes preview, show, help, and log action handling
through nested matches and deliberate fall-through, making the routing rules
difficult to follow and easy to change incorrectly.

**Approach:** Split screen-specific action handling into focused dispatch
functions that report whether they consumed an action. Keep global actions and
existing routing precedence explicit, while preserving all current behavior,
state transitions, status messages, and frontend output.

</frozen-after-approval>

## Implementation Notes

- Split `App::dispatch` into preview, show, help, and log/global routing stages;
  screen handlers return whether they consumed the action.
- Routed the global `Quit`, `TogglePreview`, and `ShowMode` actions once before
  screen-specific dispatch, preserving their existing fall-through behavior.
- Enumerated all screen-local actions, extracted shared show-diff movement, and
  added regression coverage for global actions from focused screens.
- Updated the application module documentation to describe the routing order.
- Validation passed: `cargo fmt --check`, `cargo test --all-features`, strict
  Clippy for all targets/features, and `git diff --check`.

## Review Triage Log

- `patch` -- Screen handlers no longer use wildcard arms that report unrelated
  actions as consumed; local and intentionally ignored actions are explicit.
- `patch` -- Global action policy is centralized in `is_global_action` and
  handled before screen-specific routing, avoiding inconsistent fall-through.
- `patch` -- Added a unit test covering global actions from preview, show, and
  help contexts.
- `patch` -- Extracted repeated show-diff offset updates and synchronization
  into `move_show_diff`.
- `false` -- The minimal one-shot route intentionally omits Code Map and
  acceptance sections; implementation notes are now populated and the spec is
  finalized as done.
