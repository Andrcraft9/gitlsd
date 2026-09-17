---
title: 'Preview mode'
type: 'feature'
created: '2026-09-05'
status: 'done'
baseline_commit: '0fe196b46d2e7eaaec6e31d114056ea63257982a'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The log only shows one-line commit rows, forcing users to leave gitlsd to inspect the selected commit. Users need an optional, keyboard-driven preview that retains the fast log-navigation workflow.

**Approach:** Show the selected commit through a configurable Git command in an automatically oriented split beside the log. Keep preview visible by default, toggle it with a configurable `p` binding, and let Enter focus the preview for familiar scrolling and searching until Escape returns focus to the log.

## Boundaries & Constraints

**Always:** Execute Git directly without a shell; append the selected commit ID to the configured preview argv; retain safe Git SGR styling while neutralizing other controls; refresh preview when log selection changes; choose side-by-side layout when the available content area is wider than tall and stacked layout otherwise; keep log navigation active while the preview is visible but unfocused; scope scrolling and search to the focused view; expose all effective preview configuration and bindings in help/debug behavior; preserve the intent and output of existing tests by explicitly disabling preview in their setup, then cover the new default behavior in dedicated preview-enabled tests.

**Never:** Reimplement commit/diff formatting in Rust, allow preview navigation to alter log selection, paginate preview output, or change the existing configured log command semantics.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Default preview | Non-empty log after startup | Selected commit preview is visible using `git show --stat --patch`; selection changes reload it at offset zero | A failed preview leaves log usable and reports the Git error in status |
| Toggle | `p` while preview is visible/hidden | Preview is hidden/shown; showing reloads the selected commit | No selected commit yields an empty preview without launching Git |
| Focus and navigation | Enter from log, then scroll keys, then Escape | Preview becomes active, scroll keys change only preview offset, Escape restores log focus | Offset is clamped to available content |
| Preview search | `/query`, `n`, or `N` while preview is focused | Case-insensitive matching scrolls to matching preview lines and wraps | Empty/no-match status mirrors log search behavior |
| Custom command | `set preview git show ...` | Exact argv is executed with the selected commit ID appended | Invalid command shape is rejected with file and line context |
| Existing test scenarios | Legacy unit, renderer, and debug tests | Their setup disables preview, preserving existing assertions and snapshots | Preview-specific expectations live in new tests with preview enabled |

</frozen-after-approval>

## Code Map

- `src/config.rs` -- add the preview command setting and toggle action/default `p` binding; reuse tokenization, argv quoting, action parsing, and effective-help conventions. Reserve Enter as the fixed focus transition.
- `src/git.rs` -- extend the repository boundary with preview loading by commit ID; reuse direct process execution and `safe_text`, while preserving multiline output.
- `src/app.rs` -- own preview visibility, focus, content, scroll position, preview-search state, selection-triggered reloads, and recoverable errors in the shared state machine.
- `src/ui.rs` -- render focused/unfocused preview and log panes, dynamically selecting horizontal or vertical `Layout`; reuse ANSI-to-ratatui styling for multiline preview lines.
- `src/debug.rs` -- expose preview visibility, focus, offset, and plain preview lines in deterministic snapshots.
- `src/main.rs` -- continue composing one Git-backed source that serves both history and preview operations.
- `tests/debug_mode.rs` -- explicitly disable preview for existing scenarios so their behavior remains stable; add separate enabled-preview scenarios exercising the real Git boundary, default/custom commands, toggling, focus, scrolling, searching, and failure behavior.
- `README.md` -- document controls, `set preview`, action vocabulary, default behavior, and debug scripts.
- `ARCHITECTURE.md` and `PLAN.md` -- record preview data flow/state ownership and mark the planned feature complete.

## Tasks & Acceptance

**Execution:**
- [ ] `src/config.rs` -- define, parse, validate, and display preview configuration and the `toggle-preview` action.
- [ ] `src/git.rs`, `src/app.rs`, `src/main.rs` -- add preview retrieval and model all visibility/focus/scroll/search transitions in the core.
- [ ] `src/ui.rs`, `src/debug.rs` -- render both orientations and expose deterministic preview state.
- [ ] `tests/debug_mode.rs` and module tests -- disable preview in existing test setup without changing existing assertions; add preview-enabled coverage for configuration, state transitions, process integration, sanitization, layout, and edge cases from the matrix.
- [ ] `README.md`, `ARCHITECTURE.md`, `PLAN.md` -- keep user and architecture documentation aligned.

**Acceptance Criteria:**
- Given defaults in a repository with commits, when gitlsd starts, then the log and selected commit's `git show --stat --patch` output appear in an aspect-ratio-selected split.
- Given a custom binding for `toggle-preview`, when that key is pressed, then preview visibility changes and the effective configuration reports the binding.
- Given preview focus, when standard movement, page movement, `/`, `n`, or `N` actions are used, then only preview position/search changes; when Escape is pressed, log focus resumes.
- Given a selected commit changes while preview is enabled, when navigation completes, then content reloads for its stable full commit ID and begins at the top.
- Given unsafe terminal controls in Git output, when preview renders or snapshots, then only SGR affects interactive styling and debug output contains no escape sequences.
- Given the pre-preview test suite, when it is adapted for the new default, then preview is explicitly disabled and its existing behavioral assertions remain unchanged; given new preview tests, preview is explicitly enabled or left at its enabled default and the new behavior is asserted independently.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| Finding | Verdict | Evidence and route |
| --- | --- | --- |
| Preview focus swallows Quit | medium | Verified in `App::dispatch`: the focused branch handles no `Quit`, so `q` does nothing. Patch. |
| Preview focus swallows TogglePreview | medium | Verified in `App::dispatch`: `p` cannot hide a focused preview. Patch. |
| Enter can focus preview from Help | low | Verified: Enter sets focus without checking `Screen::Log`; Escape then returns to a focused preview. Patch. |
| Help-screen Enter focus persists | low | Carried duplicate of the independently reported Help-screen Enter finding; verified at the same branch. Patch. |
| Preview ID after `--` becomes a pathspec | medium | Verified in `GitHistory::load_preview`; `git show options -- <id>` interprets the ID as a path. Patch. |
| PageDown reloads every intermediate preview | low | Verified: its loop calls `move_down`, which reloads after each selection. Patch. |
| Split renderer lacks preview-enabled tests | low | Verified by the existing UI tests explicitly hiding preview; both orientation branches are untested. Patch. |
| Preview sanitization lacks direct coverage | low | Verified: existing sanitization tests cover log text only. Patch. |
| Non-HEAD preview command argv is unverified | low | Verified by the coverage review; current real-Git custom test inspects only HEAD. Patch. |
| Split orientation lacks active renderer verification | low | Carried duplicate of the independently reported renderer-coverage finding; verified at the same branch. Patch. |
| Invalid preview configuration lacks test coverage | low | Verified: configuration tests do not exercise `set preview` rejection. Patch. |

## Design Notes

The existing `HistorySource` is the correct inward-facing seam; broaden it to a repository source rather than letting `App` or `ui` spawn processes. Keep visibility separate from focus: the log remains the active view on startup even though preview is shown. Maintain a separate last preview query so log and preview searches do not overwrite one another.

## Verification

**Commands:**
- `cargo fmt --check` -- formatting is clean.
- `cargo clippy --all-targets --all-features -- -D warnings` -- no lint warnings.
- `cargo test --all-targets --all-features` -- unit, renderer, and debug integration tests pass.
