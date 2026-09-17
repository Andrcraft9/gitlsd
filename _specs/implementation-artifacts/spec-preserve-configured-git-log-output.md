---
title: 'Preserve configured Git log output'
type: 'feature'
created: '2026-09-05'
status: 'done'
baseline_commit: '497cf50ea9e8e8d2194a34eb22228214bcb8149c'
route: 'dispatch'
review_loop_iteration: 0
context: ['{project-root}/README.md', '{project-root}/CONTRIBUTING.md', '{project-root}/ARCHITECTURE.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Log mode currently strips or overrides Git presentation options and rebuilds each row from a hard-coded record format. As a result, `set log` controls history selection but cannot make the browser look like the configured `git log` command.

**Approach:** Make Git-produced output the displayed log content, require configuration in the form `set log git log <options>`, and retain navigation, pagination, search, help, and deterministic debug behavior.

## Boundaries & Constraints

**Always:** Execute Git directly as argv without a shell; disable external paging; preserve configured presentation and history selection; add only pagination arguments; render Git-produced text rather than rebuilding fields in Rust; neutralize unsafe terminal controls; keep interactive and debug behavior shared; show the effective command accurately in help.

**Never:** Silently remove or override configured presentation options; reimplement Git history traversal; add other modes; mutate the fixture repository.

**Decision:** Keep commit-based selection with a stable commit ID. Git-produced one-line pretty formats are supported exactly; multiline formats and graph/stat/patch-style output are rejected with an actionable configuration error because they cannot map reliably to one selectable commit.

**Default:** When no configuration overrides it, use `git log --oneline --decorate` so the built-in presentation is Git-native and one line per commit.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Custom/default format | Custom one-line `--format`, or no override | Rows match Git's configured output; default is `git log --oneline --decorate` | Git failures surface stderr and exit status |
| Command form | `set log git log ...` | The full command executes and help shows its argv unambiguously | Any form not beginning with `git log` reports file, line, and the required form |
| Pagination/pathspec | Two-commit batch, optionally with `-- <path>` | Pagination precedes the pathspec and appends distinct Git-produced rows | Failed page preserves loaded rows and reports status |
| Search | Query exists only in configured display text | Existing forward/backward wraparound selects the matching row | Empty/no-match behavior is unchanged |
| Color | Git emits ANSI SGR color | Interactive styling is preserved; debug output is stable plain text | Other controls are neutralized |
| Unsupported output | Multiline format, graph, stat, patch, notes, or signature output | Configuration is rejected instead of rewritten | Error identifies file, line, and incompatible option or format |

</frozen-after-approval>

## Code Map

- `src/config.rs` (`Config::log_command`, `parse_set`, `help_lines`) -- require and preserve the full `git log` command; retain reversible help output.
- `src/git.rs` (`CommitRecord`, `HistorySource`, `GitHistory::load`) -- separate stable commit identity from Git-produced one-line display text; retain direct execution, pagination-before-pathspec, errors, and control safety.
- `src/app.rs`, `src/ui.rs`, `src/debug.rs` -- search and render the display text through the existing shared state machine.
- `tests/debug_mode.rs` and module tests -- replace forced-format expectations and cover the matrix against the pinned fixture.
- `README.md`, `ARCHITECTURE.md`, `gitlsd.conf` -- document and demonstrate Git-owned log presentation.

## Tasks & Acceptance

**Execution:**
- [x] `src/config.rs` -- require `set log git log <options>`, validate one-line output, and use the selected default.
- [x] `src/git.rs` -- return stable IDs plus Git-produced display text while retaining pagination, pager policy, errors, and safe ANSI handling.
- [x] `src/app.rs`, `src/ui.rs`, `src/debug.rs` -- adapt state, search, and both renderers to the selected output/selection model.
- [x] `tests/debug_mode.rs` and unit tests -- cover every matrix scenario and prevent regression to Rust-owned log formatting.
- [x] `README.md`, `ARCHITECTURE.md`, `gitlsd.conf` -- describe and demonstrate configurable Git-owned log presentation.

**Acceptance Criteria:**
- Given the existing navigation, search, command, help, and quit scripts, when run against configured Git output, then their behavior remains deterministic and passes without fixture changes.
- Given repository validation, when formatting, strict linting, all tests, and release help checks run, then all pass.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Design Notes

Use the configured command for display and a companion ID-only Git query with the same selection, ordering, and pagination arguments. One-line validation makes their results pairable while leaving Git in control of visible text. Pagination and pager suppression are wrapper policy; presentation is user policy.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: no formatting diff.
- `cargo clippy --all-targets --all-features -- -D warnings` -- expected: no warnings.
- `cargo test --all-targets --all-features` -- expected: unit and fixture-backed debug tests pass.
- `cargo build --release && target/release/gitlsd --help` -- expected: release help omits `--debug`.
- `git diff --check && git -C tests/fixtures/mapbox-gl-js status --short` -- expected: no whitespace errors and no fixture changes.
