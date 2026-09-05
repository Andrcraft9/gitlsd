---
title: 'Highlight search matches'
type: 'feature'
created: '2026-09-06'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Search navigation finds matching log records and preview lines, but the matching text is not visually distinguished, making results slow to locate within dense content.

**Approach:** Highlight every case-insensitive occurrence of the active pane's most recently submitted search query in the interactive log or preview, while preserving existing Git ANSI styling, selection, scrolling, search navigation, and plain-text debug output.

</frozen-after-approval>

## Implementation Notes

- Keep log and preview queries independent by exposing their existing saved search state through read-only application accessors.
- Apply a conspicuous match style in the Ratatui rendering path after ANSI parsing and before horizontal clipping so highlights compose with Git colors and remain correct across styled spans, Unicode text, multiple occurrences, and scrolling.
- Cover rendering behavior with focused UI tests; update user-facing documentation and mark the PLAN item complete after verification.
- Added read-only log and preview query accessors in `src/app.rs` and composed match highlighting with ANSI parsing and horizontal clipping in `src/ui.rs`.
- Chose bold black-on-yellow highlighting so matches stay visible under the list's selected-row reversal and across Git-provided colors.
- Updated `README.md` and `PLAN.md` to describe and record the completed behavior.
- Review hardening includes overlapping occurrences, whole-grapheme highlighting, and removal of inherited hidden/reversed Git modifiers from matched text.

## Review Triage Log

- medium: Overlapping occurrences were not all highlighted; patched by checking every folded character boundary and merging overlapping source ranges.
- medium: Match boundaries could split extended grapheme clusters and undermine display-width scrolling; patched by expanding highlights to whole graphemes.
- medium: Git-provided hidden or reversed modifiers could obscure a match; patched by removing those modifiers from highlighted spans.
- low, rejected: Recomputing highlights during redraw may become costly for unusually large loaded histories, but no performance regression is demonstrated and caching or viewport-aware rendering would add disproportionate state and complexity.
- false: The in-progress spec status was correct during review and is finalized to done only after review triage, as required by the workflow.
