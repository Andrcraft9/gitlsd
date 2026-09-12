---
title: 'Show mode'
type: 'feature'
created: '2026-09-06'
status: 'ready-for-dev'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The selected-commit preview is useful for quick inspection, but it does not provide a focused way to browse a commit by changed file and move directly to that file's patch.

**Approach:** Add a full-screen show tab for the selected commit. It contains an automatically oriented explorer/diff split: the explorer shows configurable commit metadata above selectable name-status rows, while the searchable diff initially contains all changes and jumps to the selected file when Enter focuses it.

## Boundaries & Constraints

**Always:** Open show mode through a configurable `show-mode` action, bound to `d` by default like tig; preserve the selected log commit while the tab is open; use the existing `width > height` orientation rule and the full content area rather than nesting inside the log/preview split; begin with the file explorer focused; make file rows vertically selectable and scrollable; execute Git directly without a shell; append the stable full commit ID to configurable `set show-commit git show ...` and `set show git show ...` commands; obtain the file list from the fixed `git show --name-status --format=` operation; preserve safe Git SGR styling while neutralizing other controls; keep show-diff search and horizontal/vertical offsets independent from log, preview, and help state; make Enter focus the diff at the selected file's patch header; make Escape return from diff to explorer and from explorer to the log.

**Never:** Replace the log/preview screen with persistent show-mode state, reimplement Git's commit or patch presentation, filter the diff down to one file, mutate repository state, invoke a shell, or let show navigation change the selected log commit.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Open tab | Selected log commit and `show-mode` | Full-screen explorer/diff tab loads that commit and focuses its first file row | No selected commit leaves the log active with an actionable status |
| File jump | Select a modified, added, deleted, copied, or renamed row and press Enter | Diff gains focus with that file's `diff --git` header at the top and still contains every file | If a custom command omits recognizable patch headers, focus diff at its top and report that the file location was unavailable |
| Diff navigation | Focused diff receives movement, page, horizontal, `/`, `n`, or `N` actions | Only show-diff position/search changes; search uses existing case-insensitive boundary behavior | Empty/no-match statuses mirror preview search |
| Git failure | Metadata, names, or diff command fails | Existing log remains recoverable and show mode is not entered with partial data | Status identifies which show load failed |
| Empty commit | Commit has no name-status rows | Metadata and diff render; explorer has no selection | Enter is a no-op; Escape returns to log |

</frozen-after-approval>

## Code Map

- `src/config.rs` -- extend `Action`, defaults, parsing, and effective help with `show-mode`, `show-commit`, and `show`; reuse reversible argv quoting and `git show` prefix validation.
- `src/git.rs` -- extend `HistorySource`/`GitHistory` with cohesive show data; reuse direct no-pager execution and safe-line conversion, but parse tab-delimited name-status data before sanitization and retain old/new paths for rename/copy mapping.
- `src/app.rs` -- add `Screen::Show`, explicit explorer/diff focus, show content/selection/offset/search state, load/error transitions, file navigation, patch jumps, and two-stage Escape behavior.
- `src/ui.rs` -- extract the shared aspect-ratio split helper; render the show tab across the full content area, nesting commit metadata and a stateful file list in the explorer and reusing styled/search-highlighted paragraph rendering for diff.
- `src/debug.rs` -- expose deterministic show screen, focus, selection, offsets, metadata, file rows, and diff content.
- `src/main.rs` -- pass the two configured show commands into the Git-backed source.
- `tests/debug_mode.rs` -- exercise show mode against the pinned repository, including selected non-HEAD commits, custom commands, file jumps, focus/back transitions, fixed name-status output, and recoverable failures.
- `README.md`, `ARCHITECTURE.md`, `PLAN.md` -- document controls/configuration, update state/data-flow ownership, and mark show mode complete.

## Tasks & Acceptance

**Execution:**
- [ ] `src/config.rs`, `src/main.rs` -- define and wire the show action and commands, including validation/help tests.
- [ ] `src/git.rs` -- load and sanitize show metadata, structured changed-file rows, and full diff; map ordinary and rename/copy paths to patch starts with adapter tests.
- [ ] `src/app.rs` -- implement show screen state and transitions with unit coverage for navigation, search, jumps, resets, empty data, and failures.
- [ ] `src/ui.rs`, `src/debug.rs` -- render and snapshot show state, covering both split orientations, active-pane widths, list visibility, styling, and focus.
- [ ] `tests/debug_mode.rs` -- add real-Git end-to-end coverage for the edge-case matrix.
- [ ] `README.md`, `ARCHITECTURE.md`, `PLAN.md` -- keep user and architecture documentation aligned.

**Acceptance Criteria:**
- Given show mode is open, when the terminal changes between wide and tall shapes, then explorer/diff switches between side-by-side and stacked using the same rule as log/preview without showing the log pane.
- Given a file is selected, when Enter is pressed, then the diff is focused at that file's section and Escape returns focus to the unchanged file selection.
- Given show mode has custom metadata and diff commands, when it opens for a non-HEAD selection, then both receive that selection's full commit ID while the fixed name-status list remains selectable.
- Given show mode closes and reopens for another selected commit, when loading completes, then its selection, offsets, and search reset without disturbing log or preview state.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Design Notes

`ShowData` should cross the repository boundary as one cohesive result so the application never renders mismatched partial commands. Changed-file records should preserve the exact sanitized name-status display plus parsed path candidates. Patch offsets are derived from unambiguous Git patch headers, including rename/copy old/new paths; customized output that removes those headers degrades to offset zero rather than filtering or rerunning the diff.

## Verification

**Commands:**
- `cargo fmt --check` -- formatting is clean.
- `cargo clippy --all-targets --all-features -- -D warnings` -- no lint warnings.
- `cargo test --all-targets --all-features` -- unit, renderer, debug integration, and real-Git fixture tests pass.
