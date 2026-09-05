---
title: 'Fix mouse scroll regression'
type: 'bugfix'
created: '2026-09-05'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** Commit `9e51ad81a42d8a39830b7d7fa47fd51cc9ed9e0b` broke mouse scrolling because interactive mouse handling now ignores vertical wheel events and only translates horizontal wheel events.

**Approach:** Restore vertical wheel translation to the existing move actions while keeping horizontal wheel translation mapped to horizontal scroll actions.

</frozen-after-approval>

## Implementation Notes

- `src/ui.rs::translate_mouse` is the interactive mouse-event boundary introduced by the regression commit.
- `src/app.rs::dispatch` already supports `MoveUp`, `MoveDown`, `ScrollLeft`, and `ScrollRight` for the active screen or pane, so the fix should reuse those existing actions.
- `MouseEventKind::ScrollUp` now maps to `Action::MoveUp`, and `MouseEventKind::ScrollDown` now maps to `Action::MoveDown`; log, preview, and help behavior then follows the existing active-pane dispatch rules.
- `MouseEventKind::ScrollLeft` and `MouseEventKind::ScrollRight` continue to map to horizontal scroll actions.
- Updated `README.md` so the documented mouse controls describe both vertical and horizontal wheel/touchpad events.
- Verified with `cargo test ui::tests::translates_scroll_mouse_events` and `cargo test`.

## Review Triage Log

- medium: README documented only horizontal mouse-wheel/touchpad events; updated it to include vertical events as well.
- low rejected: broader interactive-loop mouse coverage would require a larger harness change, while the regression is isolated to the private event translator and covered by `ui::tests::translates_scroll_mouse_events`.
- medium: spec status was still `in-progress`; set it to `done` after implementation and verification.
- low: spec intent did not spell out active-pane vertical behavior; implementation notes now state that vertical wheel events reuse existing active-pane `MoveUp`/`MoveDown` dispatch.
