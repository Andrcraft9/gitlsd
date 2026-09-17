---
title: 'Horizontal navigation extensions'
type: 'feature'
created: '2026-09-05'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Horizontal scrolling requires repeated arrow presses and cannot be driven with a horizontal mouse wheel or touchpad gesture.

**Approach:** Add Home and End as horizontal beginning/end navigation for the active pane, and route Crossterm horizontal mouse-scroll events through the existing horizontal scroll actions. End shows the final visible content rather than a blank offset.

</frozen-after-approval>

## Implementation Notes

- Added configurable Home/End bindings and bounded horizontal offsets based on the active pane's content width and viewport width.
- Enabled terminal mouse capture and routed horizontal mouse-wheel or touchpad events through the shared scroll actions only while normal navigation is active.
- Kept the existing horizontal state model; the interactive frontend supplies transient pane width before dispatching input.
- Verified with `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`.

## Review Triage Log

- medium — End could leave final content clipped when its offset fell inside a width-two grapheme; fixed by skipping an unsplittable grapheme and testing the case.
- medium — Log End did not reserve the selected-row marker width; fixed by including it in the interactive content-width calculation.
- medium — mouse scrolling changed a pane while search or command input was active; fixed by accepting mouse events only in normal input mode.
- low — repeatedly measuring all loaded content and reconstructing all scrolled log rows can be expensive; deferred because it predates this extension and needs a broader rendering design.
- low — the implementation artifact initially lacked notes and status; completed here.
