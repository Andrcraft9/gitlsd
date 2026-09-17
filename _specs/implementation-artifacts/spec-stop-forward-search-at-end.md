---
title: 'Stop repeated search at boundaries'
type: 'bugfix'
created: '2026-09-06'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Repeating a search past its final or first match loops to the opposite boundary, so the user cannot tell that they exhausted searchable content in that direction.

**Approach:** Keep the selection at the boundary match and display `(END)` or `(TOP)` when repeated forward or backward search exhausts the log or focused preview. Preserve initial search discovery, no-match reporting, and incremental log loading.

</frozen-after-approval>

## Implementation Notes

- Treat the requested bottom behavior as forward repetition with `n`; backward `N` behavior remains unchanged because no top-boundary behavior was requested.
- Split initial search from repeated search internally so `/query` still discovers matches anywhere, while repeated `n` searches only below the current match.
- Applied the same end behavior to log and focused-preview searches, retaining incremental history loading, true no-match messages, and backward wrapping.
- Updated unit, debug-mode integration, and control documentation expectations.
- Review added direct state-machine coverage for incremental log loading, end retention, true no-match handling, and preserved backward wrapping.
- Supersedes the earlier asymmetric notes: backward `N` now stops at `(TOP)` without wrapping, matching forward `n` at `(END)`.
- Backward repetition does not fetch later history batches because they are below the current selection and cannot satisfy a search toward the top.

## Review Triage Log

- medium: Log end behavior lacked focused unit coverage; patched with a multi-batch state-machine test covering progression, `(END)`, selection retention, missing queries, and backward wrapping.
- low: The debug integration test asserts the final boundary snapshot rather than each intermediate step; rejected because existing integration assertions cover forward and backward progression and the new focused unit test covers the complete sequence.
- false: The in-progress spec status was correct during review and is finalized to done only after review triage, as required by the workflow.
- low: README no longer stated the retained backward wraparound behavior explicitly; patched with concise asymmetric `n`/`N` documentation.
- User subsequently revised the requirement to symmetric boundaries; implementation and tests now stop backward repetition at `(TOP)` instead of wrapping.
- Reused the existing wrap flag only for initial search discovery; both repeated directions now disable wrapping and report their respective boundary marker.
- medium: Backward repetition unnecessarily loaded all later history before reporting `(TOP)`; patched by limiting incremental loading to forward searches.
- medium: Boundary coverage did not prove backward repetition avoids loading later batches; patched by asserting the loaded record count remains unchanged while `has_more` is true.
- false: Earlier notes and triage entries accurately record the prior approved implementation; the later superseding notes capture the user's symmetric-boundary revision.
- false: The in-progress status was correct during re-review and is finalized to done only after triage, as required by the workflow.
