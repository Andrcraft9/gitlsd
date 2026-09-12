---
title: 'Encapsulate application state'
type: 'refactor'
created: '2026-09-09'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context:
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Most of `App` and its screen-state data is publicly mutable, so
frontends can depend on representation details and bypass the application
state machine's invariants.

**Approach:** Make application and grouped screen-state fields private, then
expose intent methods and read-only view accessors for the interactive and
deterministic frontends. Preserve all current behavior and debug output while
removing direct mutation from those consumers.

</frozen-after-approval>

## Implementation Notes

- Made `App`, `LogState`, `HelpState`, and `ShowState` fields private and
  removed the `Deref`-based flat state escape hatch.
- Added read-only accessors for frontend rendering/debug snapshots and kept
  mutations inside the existing `App` intent methods and state transitions.
- Updated UI/debug fixtures to initialize state through `HistorySource` and
  dispatch actions, preserving the existing rendered and snapshot contracts.
- Updated module and architecture documentation to record the frontend
  ownership boundary.
- Review hardening made `InputMode` private and replaced the concrete
  `Config` accessor with the narrower `help_lines` view needed by frontends.
- Verification passed with `cargo fmt --check`, strict Clippy, 68 unit tests,
  25 debug integration tests, and `git diff --check`.

## Review Triage Log

- `patch` -- `InputMode` was still public despite being internal-only state;
  it is now private.
- `patch` -- The public `config()` accessor exposed the concrete configuration
  representation; frontends now receive only effective help lines.
- `low / rejected` -- The accessor set mirrors the state needed to render and
  debug, but every returned value is immutable and the finding explicitly
  requires read-only views. Purpose-built wrapper types would add API and
  churn without changing reachable mutation.
- `false` -- The viewport-width concern is not reproducible: the UI computes
  width immediately before each key, and a transition action does not also
  scroll a pane in the same dispatch.
- `defer` -- `explorer_offset` remains a pre-existing ownership mismatch with
  Ratatui `ListState`; it is recorded in `deferred-work.md` as REVIEW finding
  8 rather than expanding this refactor.
- `low / rejected` -- Unit tests in the `app` module can intentionally seed
  private state because they are testing internal transitions; this is not a
  public consumer or a runtime invariant bypass.
- `low / rejected` -- Existing integration tests use stable substring
  assertions and the complete test suite passed; adding exact snapshots is a
  separate test-hardening improvement, not a regression caused here.
