---
title: 'Configurable editor'
type: 'feature'
created: '2026-09-14'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Files visible in show and status mode cannot be opened directly in an editor at the source location represented by the focused diff line.

**Approach:** Add a configurable editor command, defaulting to micro, and a configurable open-editor action usable from both the file explorer and diff panes. Expand `file` and `line` placeholders so commands such as `code -g --goto file:line` work; use line 1 when the current pane has no source-line mapping.

</frozen-after-approval>

## Implementation Notes

- Added the global `open-editor` action on `e` and the `editor` command setting, defaulting to `micro +line file`; configured arguments expand `file` and `line` without a shell.
- Added worktree-path selection and unified-diff new-line mapping for show/status explorer and diff focus. Explorer selections and unmappable diff rows use line 1; files absent from the worktree report an actionable status.
- Added an editor request boundary so the interactive frontend restores the terminal before launch and resumes it afterward, while debug mode exposes requests deterministically.
- Updated README, architecture, plan status, configuration/unit coverage, and debug integration coverage. `cargo test`, `cargo clippy --all-targets -- -D warnings`, formatting, and `git diff --check` pass.
- Review fixes made placeholder expansion single-pass and OS-string-safe, retained raw show paths without changing their Git-authored display, rejected incomplete commands, checked worktree existence before launch, and refreshed status after successful edits.
- Final verification passes with 110 unit tests and 49 debug integration tests.
- Updated the checked-in `gitlsd.conf` example and smoke-tested it through debug mode; it resolves the default editor request to `micro +1 .gitignore` in the repository root.
- Follow-up cleanup changed show-mode `ChangedFile` paths from raw byte vectors to `PathBuf`, preserving exact filesystem identity while removing conversion from editor request construction; status paths remain byte-based for exact Git mutations.

## Review Triage Log

- medium — Confirmed that sequential placeholder replacement could mutate `file` text inserted from a path; patched with single-pass structured expansion and a regression test.
- medium — Confirmed that unrestricted substring replacement corrupted ordinary options such as `--profile`; patched by limiting bare forms to `file`, `line`, `+line`, and `file:line`, with braced placeholders for other embedding.
- medium — Confirmed that combined placeholders used lossy UTF-8; patched by composing `OsString` fragments and testing a non-UTF-8 path on Unix.
- medium — Confirmed that show file discovery discarded raw path bytes; patched by pairing the unchanged Git-authored display list with a NUL-delimited raw path list.
- medium — Confirmed that an empty editor executable passed parsing; patched with first-argument validation.
- medium — Confirmed that commands without a selected-file placeholder could launch without opening the file; patched by requiring both file and line placeholders.
- false — Delta and arbitrary structural filters do not retain a reliable per-row new-side line mapping; falling back to line 1 is the explicitly required behavior when a line is unavailable.
- medium — Confirmed that historical show paths could be absent from the current worktree and cause an editor to create a file; patched with a worktree existence check.
- medium — Confirmed that status data stayed stale after editing; patched by refreshing status on successful editor return while preserving the active focus behavior.
- low — Coverage was thin around editor edge cases; patched with four-pane request, missing-file, status-refresh, launch-failure, placeholder-safety, raw-path, and debug integration coverage. Terminal transitions reuse the existing tested restoration primitives.
- false — `in-progress` was the workflow-required state during review; this finalization changes it to `done` after review and verification completed.
