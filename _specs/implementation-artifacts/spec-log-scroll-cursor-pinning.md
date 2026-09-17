---
title: 'Keep log cursor pinned while reversing scroll direction'
type: 'bugfix'
created: '2026-09-05'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After navigating down until the log cursor is at the bottom of the viewport, moving up scrolls the list while leaving the cursor at the bottom.

**Approach:** Retain the interactive list viewport state between redraws so reverse navigation moves the cursor upward through visible rows and only scrolls once it reaches the top.

</frozen-after-approval>

## Implementation Notes

- Retained one `ListState` for the lifetime of the interactive frontend and pass it into every log render.
- Added a regression test that dispatches real navigation actions and covers the default preview layout in both orientations.
- Verified with `cargo fmt --check` and `cargo test`.

## Review Triage Log

- low — the initial regression test changed selection directly; replaced it with `App::dispatch` navigation through an in-memory history source.
- low — the initial regression test hid the preview; expanded it to cover the default preview layout in horizontal and vertical orientations.
- low — the workflow artifact was unfinished; recorded implementation and verification details and marked it done.
