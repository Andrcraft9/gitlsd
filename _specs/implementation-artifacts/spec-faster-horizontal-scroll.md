---
title: 'Faster horizontal scrolling'
type: 'feature'
created: '2026-09-08'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Left and Right currently move the active pane by only one terminal column, making long lines slow to inspect.

**Approach:** Match tig's default behavior by moving half of the active pane's visible width per horizontal-scroll action, while retaining the existing bounds and active-pane routing.

</frozen-after-approval>

## Implementation Notes

- Changed shared horizontal navigation in `src/app.rs` to step by 50% of the active viewport width, with a one-column minimum and the existing content bound.
- Added application-state regression coverage and documented the faster Left/Right behavior in `README.md`.
- Gave the deterministic debug frontend a stable 80-column viewport so scripted horizontal navigation exercises the same proportional behavior as the interactive frontend.

## Review Triage Log

- medium — Debug mode retained its one-column default because it had no viewport width; patched with a stable 80-column debug viewport and regression expectations.
- low — Preview and help assertions used a three-column viewport that could not distinguish proportional scrolling from the old behavior; patched to assert two-column steps with a four-column viewport.
