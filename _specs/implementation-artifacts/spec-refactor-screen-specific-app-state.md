---
title: 'Refactor screen-specific application state'
type: 'refactor'
created: '2026-09-09'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: 'f90e85552f5adfc33ad8a777f96702f07ab14cc4'
context:
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `App` stores log, preview, help, and show fields side by side, so inactive-screen state can form meaningless combinations such as show-diff focus while the log screen is active. This weakens the state machine and makes screen transitions harder to reason about.

**Approach:** Group related fields into `LogState`, `HelpState`, and `ShowState`. Keep persistent log/preview state on `App`, and represent only the active secondary screen with enum payloads so help and show state exist only while their screen is active.

## Boundaries & Constraints

**Always:** Preserve all current navigation, focus, pagination, preview, show, search, horizontal scrolling, status, debug snapshot, and rendering behavior. Returning from help or show must restore the existing log selection and preview state. A newly opened help or show screen must begin with the same reset state as today.

**Never:** Do not combine this work with the separate findings about field visibility, dispatch routing, viewport ownership, layout calculation, patch parsing, search behavior, or explorer-offset semantics. Do not change configured commands, key bindings, user-facing text, or debug snapshot format.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open and leave help | Populated log/preview state, then Help and Back | Help owns fresh offsets; Back restores unchanged log/preview state | N/A |
| Open and leave show | Selected log commit, then Show, focus changes, and staged Back | Show owns its focus/content/offsets; final Back restores unchanged log selection | Existing load failures stay on log and retain their status message |
| Reopen secondary screen | Help or show was previously closed | A fresh `HelpState` or `ShowState` is created with reset offsets, focus, selection, and search state | Empty commits and missing patches retain current recoverable behavior |

</frozen-after-approval>

## Code Map

- `src/app.rs` -- Defines the flattened `App`, `Screen`, and `ShowFocus`; owns all transitions and state mutation. Introduce cohesive state types here and keep persistent pagination/search fields with their owning log/preview state. Do not refactor `dispatch` beyond the changes needed to access enum payloads.
- `src/ui.rs` -- Reads every screen-specific field for active-pane width and rendering; update matches to consume the active screen payload while preserving layouts and Ratatui `ListState` behavior.
- `src/debug.rs` -- Serializes the same state into the stable debug contract; adapt reads without changing snapshot keys or conditional sections.
- `tests/debug_mode.rs` -- End-to-end scripted behavior and snapshot assertions; use unchanged expectations as the external regression boundary.
- `ARCHITECTURE.md` -- Describes `app` as the screen state machine; record the new ownership boundary because the module architecture changes internally.
- `REVIEW.md` -- Source finding only; do not rewrite or remove findings as part of the implementation.

## Tasks & Acceptance

**Execution:**
- [x] `src/app.rs` -- Add `LogState`, `HelpState`, `ShowState`, and a payload-bearing active-screen enum; migrate state reads, mutations, initialization, and transitions while preserving behavior and keeping the later privacy/routing findings out of scope.
- [x] `src/ui.rs`, `src/debug.rs` -- Render and snapshot state through the new grouped representation without changing visible or deterministic output.
- [x] `src/app.rs`, `src/ui.rs`, `src/debug.rs`, `tests/debug_mode.rs` -- Adapt unit fixtures/assertions and retain regression coverage for initialization, help/show entry and exit, focus, scrolling, search, failures, and reopening reset behavior.
- [x] `ARCHITECTURE.md` -- Document that persistent log state is separate from enum-owned active help/show state.

**Acceptance Criteria:**
- Given an `App` on the log screen, when its state is inspected structurally, then no help state or show focus/content/selection/offset state exists.
- Given an `App` on help or show, when Back returns to log, then the previous log selection and preview state remain intact.
- Given existing scripted inputs and render scenarios, when the full validation suite runs, then user-visible output and debug snapshots remain unchanged.
- Given the refactor diff, when reviewed against `REVIEW.md`, then it resolves finding 1 without preemptively implementing findings 2 through 14.

## Implementation Notes

- Added `LogState` for persistent history/preview data, plus defaultable `HelpState` and `ShowState` payloads owned by `Screen`.
- Updated application transitions, UI rendering, deterministic snapshots, and test fixtures to work with active-screen payloads. Existing debug snapshot keys and user-visible behavior remain unchanged.
- Verified all matrix scenarios through the passing help navigation/reset, show navigation/failure/empty-commit, and show reopening tests.
- Review patches removed post-MSRV let-chain syntax and avoided an eager full-diff search scan, documented the new public state types, and strengthened transition/reset coverage.
- Validation passed: `cargo fmt --check`, strict Clippy for all targets/features, and 93 tests across unit and integration suites.

## Spec Change Log

## Review Triage Log

- `high` -- Blind Hunter: the let-chain in `submit_search` requires Rust 1.88 while `Cargo.toml` declares Rust 1.85; patched to nested conditionals so the declared MSRV remains viable.
- `low` -- Blind Hunter: debug output after leaving horizontally scrolled help now reports zero instead of stale inactive help state. The difference is real but represents the intended removal of inactive help state; retaining compatibility would reintroduce the invalid state, so it is rejected.
- `false` -- Blind Hunter: `show_search_query()` returns `None` outside show instead of exposing a stale query. Show search now belongs to the active `ShowState`, and no supported UI/debug consumer reads it outside show; reopening behavior remains unchanged.
- `false` -- Blind Hunter: tuple variants and grouped public fields are source-incompatible with the old flattened public representation, but that structural change is the explicit purpose of finding 1. Public visibility remains available for the later privacy finding; compatibility was not promised for this pre-1.0 internal application API.
- `low` -- Blind Hunter: `Deref<Target = LogState>` keeps existing log-field syntax and permits the same replacement already possible through public `App::log`; the readability concern is real but uncommon, and removing it creates broad churn that finding 2's accessor boundary will supersede, so it is rejected.
- `medium` -- Blind Hunter: help-transition tests compared only log selection and could miss other persistent log/preview mutations; patched with a complete `LogState` before/after equality assertion.
- `medium` -- Blind Hunter: show-transition tests compared only fragments of persistent log state; patched with a complete `LogState` before/after equality assertion across show navigation and final Back.
- `medium` -- Blind Hunter: `search_show_diff` eagerly rescanned every diff even after finding a result; patched so the boundary scan runs only for a non-wrapping miss.
- `low` -- Blind Hunter: no debug regression test covers the changed stale help-offset value after Back. That value is intentionally zero because help state no longer exists, so preserving the stale value is not a valid regression target and the finding is rejected.
- `low` -- Blind Hunter: the new public state types and payload accessors lacked ownership documentation; patched with concise API documentation.
- `low` -- Edge Case Hunter: a help-scroll/Back snapshot changes its inactive help offset from stale nonzero to zero. This duplicates the verified intentional semantic change above; retaining stale state would violate the refactor, so it is rejected.
- `low` -- Edge Case Hunter: existing scripted inputs that end after leaving scrolled help can observe zero rather than stale state. The debug format and active-screen values are preserved, while the removed inactive value is intentionally no longer representable; rejected.
- `medium` -- Verification Gap: command-driven help reopening lacked a test proving both offsets reset; patched with `help_command_replaces_scrolled_help_with_fresh_state`.

## Design Notes

`LogState` remains present across screens because log selection, loaded history, preview content, and pagination must survive temporary help/show screens. The active-screen enum therefore owns only ephemeral alternatives:

```rust
pub struct App { pub log: LogState, pub screen: Screen, /* global state */ }
pub enum Screen { Log, Help(HelpState), Show(ShowState) }
```

This makes invalid help/show combinations unrepresentable without forcing reconstruction of persistent repository history. Public visibility can remain compatible for now; finding 2 will define the accessor boundary separately.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: all Rust sources are formatted.
- `cargo clippy --all-targets --all-features -- -D warnings` -- expected: no warnings.
- `cargo test --all-features` -- expected: all unit and integration tests pass with unchanged behavioral assertions.
