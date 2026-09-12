---
title: 'Status mode'
type: 'feature'
created: '2026-09-10'
status: 'ready-for-dev'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
  - '{project-root}/CONTRIBUTING.md'
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** gitlsd can inspect committed changes but cannot browse and stage working-tree changes.

**Approach:** Add a full-screen status tab with equally sized explorer and diff panes, using the existing automatic orientation. The explorer contains staged and unstaged lists; the complete diff follows the active list.

## Boundaries & Constraints

**Always:** Provide configurable actions `status-mode` (default `s`), `status-switch-group` (Tab), and `status-toggle-stage` (`u`). Start in the unstaged explorer. Up/Down and existing movement bindings select files; Enter focuses the diff at the selected file. Tab switches groups while the explorer is focused. Diff scrolling and search synchronize the active explorer selection. Escape/q backs out from diff to explorer, then to log. Repeating status-mode closes status; reopening reloads repository state. Preserve log selection. Split explorer/diff 1:1 using the same content-area aspect-ratio rule as log/show; stack staged above unstaged within the explorer, each receiving half its height.

Use fixed Git status discovery and fixed whole-file stage/unstage operations. Configure only the diff via `set status-diff git diff ...`, default `git diff`; add `--staged` for the staged group before any path separator. Keep complete group diffs rather than filtering to the selected file. Run commands directly without a shell or pager. Preserve exact paths for mutations and sanitize only presentation. Refresh both groups and diffs after mutations, retaining valid selection where possible. The same file may appear in both groups.

**Never:** Discard working-tree edits, commit changes, stage unrelated paths, introduce hunk staging, create an external terminal tab, or stage this implementation's changes during development.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected behavior | Error handling |
|----------|---------------|-------------------|----------------|
| File navigation | Active explorer selection; Enter | Focus complete group diff at file header | Missing header: focus top and report unavailable location |
| Partial staging | File has index and worktree changes | File appears in both groups; u acts on active group | Refresh safely and report command failure |
| Whole-file mutation | u in explorer or diff | Stage selected unstaged file, or unstage selected staged file; preserve working-tree bytes | Empty selection is a no-op; failed refresh must prevent mutations against stale data |
| Untracked file | Fixed status reports new path | Include under unstaged; allow staging | Ordinary git diff has no patch until staged; report unavailable location |
| Empty repository/group | No commits or changes | Status remains usable, with no invalid selection | Distinguish unborn HEAD from unrelated log failures |
| Special paths | Rename, deletion, spaces, controls, non-UTF8 paths, literal pathspec characters | Target exact raw paths, including both rename endpoints where required | Git failures remain recoverable |
| Custom diff | Command omits headers or filters output | Keep independent fixed lists and searchable output | Missing mappings never select an unrelated file |

</frozen-after-approval>

## Code Map

- `src/config.rs`: domain action groups, `Action::ALL`, Key parsing/display, Config defaults/validation/effective help.
- `src/git.rs`: `HistorySource`, `GitHistory`, `ShowData`, `ChangedFile`, `patch_offset`, `file_at_patch_offset`, `safe_text`. Existing ChangedFile paths are sanitized presentation and cannot safely identify mutation targets.
- `src/app.rs`: Screen/ActivePane dispatch, ShowState, Enter handling, show navigation/search/jump/synchronization, back transitions. Reuse behavior without coupling status lifetime to show or log state.
- `src/ui.rs`: `content_split_direction`, `active_content_width`, `render_show`, styled/highlighted rendering and persistent ListState scrolling; `translate_key` needs Tab.
- `src/debug.rs`: shared scripted input and stable snapshots.
- `src/main.rs`: configuration-to-adapter wiring.
- `tests/debug_mode.rs`: `run_in` and temporary Git fixtures; never mutate the pinned fixture for staging tests.

## Tasks & Acceptance

**Execution:**
- [ ] `src/config.rs`, `src/main.rs` — add status actions, Tab vocabulary, diff configuration, validation/help, and adapter wiring.
- [ ] `src/git.rs` — add cohesive status data and mutation contract; parse fixed porcelain v1 NUL output, retaining raw paths separately; load both diffs; perform literal-pathspec add/reset and handle unborn HEAD. Normalize repository-root operation for subdirectory invocation.
- [ ] `src/app.rs` — add status state, group selection, focus, navigation/search/synchronization, load/toggle/back transitions, mutation refresh and recoverable errors; allow status access in unborn repositories.
- [ ] `src/ui.rs`, `src/debug.rs` — render both lists and group diff; translate Tab; expose focus, group, selections, offsets and contents in snapshots.
- [ ] `tests/debug_mode.rs` and module tests — verify matrix cases in isolated repositories, including staging round trips and preservation of worktree contents.
- [ ] `README.md`, `gitlsd.conf`, `ARCHITECTURE.md`, `PLAN.md`, module docs — document controls, untracked behavior, configuration and data/mutation boundaries; mark completion after validation.

**Acceptance Criteria:**
- Given a wide or tall terminal, when status opens or resizes, then its equal explorer/diff split uses the existing orientation rule and both explorer groups remain rendered.
- Given custom status bindings and diff settings, when actions run, then those settings take effect while discovery and staging commands remain fixed.
- Given multiple patches, when Enter, scrolling or search changes diff position, then the corresponding active-list file is selected and visible.
- Given staged and unstaged edits, when u stages then unstages a selected file, then Git's index reflects each operation, both panes refresh, and working-tree contents remain unchanged.
- Given status is closed, when log resumes, then its selected commit and preview state are preserved.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Design Notes

Status data should cross the Git boundary cohesively. Keep presentation records separate from raw mutation paths. Fixed porcelain output distinguishes index and worktree states independently of custom diff presentation. Mutation methods must report success separately from a subsequent reload failure. This change spans seven source modules, integration tests and documentation; it adds repository mutation APIs but development validation mutates only disposable test repositories.

## Verification

- `cargo fmt --check` — clean formatting.
- `cargo clippy --all-targets --all-features -- -D warnings` — no warnings.
- `cargo test --all-targets --all-features` — existing and status tests pass, including real-Git index assertions and renderer checks for both orientations.
