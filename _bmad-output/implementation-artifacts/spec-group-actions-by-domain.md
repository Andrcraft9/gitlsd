---
title: 'Group actions by domain'
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

**Problem:** The flat `Action` enum makes every dispatcher accept every action and
forces `App::dispatch` to encode precedence through global-action checks, a chain of
conditional screen handlers, boolean consumption results, and unreachable fallback
arms. The action vocabulary therefore obscures ownership even after the first action
routing refactor.

**Approach:** Replace the flat action vocabulary with a top-level enum containing
focused action enums, including preview and show groups, and route those groups with
one exhaustive pattern match in `App::dispatch`. Preserve the existing configured
action names and all context-sensitive behavior while moving state-specific decisions
into focused handlers.

</frozen-after-approval>

## Implementation Notes

- Replaced the flat runtime action variants with `NavigationAction`,
  `SearchAction`, `GlobalAction`, `PreviewAction`, and `ShowAction` groups;
  configuration parsing, default bindings, effective help, and all existing
  action names remain unchanged.
- Changed `App::dispatch` to one exhaustive match over the grouped action
  enum. A shared active-pane resolver sends navigation and search to the
  focused log, preview, show, or help handler while preserving the Help-to-log
  search fallback and global transition behavior.
- Updated direct dispatch callers and tests, added action-name round-trip and
  Help search-fallback coverage, and made the scripted semicolon test exercise
  configuration parsing.
- Updated `ARCHITECTURE.md` and module-level documentation to describe action
  group ownership and context routing.
- Validation passed: `cargo fmt --all`, `cargo test --all-features`, strict
  Clippy for all targets/features, and `git diff --check`.

## Review Triage Log

- `patch` — Finalized this spec as `done` and recorded implementation notes and
  verification results.
- `false` — This is an approved `oneshot` spec whose workflow requires the
  frozen intent, implementation notes, and final status; a full planning Code
  Map/task/acceptance artifact is not required for this route, and behavior is
  covered by the existing and added tests.
- `patch` — Named all five action groups and their context/global ownership in
  `ARCHITECTURE.md`.
- `low (rejected)` — `Action::ALL` is complete and covered for all current
  variants; replacing the small explicit inventory with a macro or generated
  source would add maintenance complexity for a future-only omission risk.
- `patch` — Added an equal-length assertion before the action-name round-trip
  test's `zip` so a mismatched inventory cannot be silently truncated.
- `low (rejected)` — The child enums are internal runtime vocabulary types in a
  pre-1.0 crate, and adding per-enum rustdoc would not affect normal use or
  behavior; existing module documentation describes the public boundary.
- `patch` — Centralized preview/show/help/log precedence in `active_pane`, so
  navigation and search share one context resolver.
- `patch` — Documented and tested the intentional Help-to-log search fallback,
  preserving the pre-refactor behavior.
- `false` — Configured key-level coverage already exists in the debug frontend
  tests and the Ctrl-quit path remains covered by the application unit test;
  direct grouped dispatch is appropriate for focused application tests.
- `patch` — Changed the semicolon debug test to parse `bind semicolon quit`
  before running the scripted key.
