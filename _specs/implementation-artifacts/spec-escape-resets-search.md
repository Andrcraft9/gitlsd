---
title: 'Reset submitted search with Escape'
type: 'bugfix'
created: '2026-09-13'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A submitted search query remains active indefinitely, so its matches stay
highlighted after the user has finished searching. Escape does not provide a way to
return the active pane to its unsearched state.

**Approach:** Make the shared Escape/`q` back action clear the active pane's submitted
search first, removing its highlights and repeat-search state; preserve Escape's
existing text-input cancellation and existing pane-back/quit behavior when no search
is active.

</frozen-after-approval>

## Implementation Notes

- Added pane-aware clearing of submitted search state before the shared `back` action
  navigates or quits. This preserves the established equivalence of Escape and `q`.
- Preserved Escape's input-mode behavior: it cancels unfinished search text without
  clearing the previously submitted query.
- Added application-state coverage for log, preview, show, and status search reset,
  adjusted navigation tests for the additional reset step, and updated controls and
  the tracked PLAN item.
- Review correction: reset now consults the actual active pane, so a retained log
  search cannot consume back navigation from preview, Help, or show/status explorers.

## Review Triage Log

- patch — The Blind Hunter identified that a stale log query could consume Escape in
  preview, Help, and show/status explorer panes. `clear_active_search` now matches
  `active_pane()` exactly, and regression coverage verifies those panes still back
  out while preserving the inactive query.
