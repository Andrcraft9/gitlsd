---
title: 'Optional diff filter with delta example'
type: 'feature'
created: '2026-09-12'
status: 'done'
baseline_commit: '029fe504e05e3c10a7185a7d8557b30d230ed530'
route: 'dispatch'
review_loop_iteration: 0
context: ['{project-root}/AGENTS.md', '{project-root}/README.md', '{project-root}/CONTRIBUTING.md', '{project-root}/ARCHITECTURE.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Preview, show, and status display Git output without a configurable presentation filter. Users want delta's syntax coloring and within-line highlighting in every diff view.

**Approach:** Add one shared optional `diff-filter` command, disabled by default. Run configured filters on diff presentation output before terminal sanitization. An empty setting (`set diff-filter =`) disables filtering and preserves today's Git output. Add `set diff-filter = delta --paging never` to the example `gitlsd.conf` so it matches normal piped delta output.

**Failure policy:** A configured filter that cannot launch or exits unsuccessfully reports an error for the affected view. There is no fallback. Delta is only required when explicitly configured.

## Boundaries & Constraints

**Always:** Apply the same setting to the full preview, show diff, staged diff, and unstaged diff including untracked files. Preserve safe ANSI colors, search, scrolling, file navigation, and deterministic plain-text debug output. Execute commands directly with argument vectors using the existing configuration quoting syntax. Keep Git discovery, file identity, and staging semantics intact.

**Never:** Filter log rows, show metadata, file lists, porcelain status, or mutation commands. Introduce shell pipelines, install delta automatically, change the user's Git configuration, or add unrelated UI features. Stage, commit, or push changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default | No explicit setting | Existing unfiltered output without requiring delta | No filter process launched |
| Delta example | Example configuration and delta available | Normal delta presentation in all three modes; delta file headers support file jumps | Configured filter failures report an error |
| Disabled | `set diff-filter =` | Existing unfiltered output | No filter process launched |
| Custom command | Executable plus quoted arguments | Git bytes supplied on stdin; stdout becomes displayed content | Filter diagnostics identify executable and affected operation |
| Filter failure | Missing executable or unsuccessful exit | Error for the affected view, without fallback | Identify command and sanitized launch or exit diagnostic |
| Unsafe controls | Filter emits ANSI colors and terminal controls | Preserve supported SGR; strip unsafe controls; debug strips colors | Sanitize diagnostics too |
| Large diff | Input and output exceed pipe capacity | Complete output without pipe deadlock | Reap child and report I/O failures |
| Empty output | Git produces an empty diff | Empty diff stays empty without decorative filter output | Skip filter for empty input |
| Custom structural changes | Filter removes recognized Git and delta file headers | Existing graceful missing-patch behavior | Document header preservation for file navigation |

</frozen-after-approval>

## Code Map

- `src/config.rs`: `Config`, defaults, `set` parsing, command quoting, and `help_lines` own runtime policy. Existing Git command validators require Git prefixes and must not be reused for arbitrary filter commands.
- `src/main.rs`: constructs `GitHistory` from effective configuration; pass the optional filter here.
- `src/git.rs`: `GitHistory::load_preview`, `load_show`, `load_status`, and `load_untracked_diffs` collect presentation bytes. `run`, `run_show`, and `run_status_command` enforce Git process semantics. Keep filter execution separate from discovery helpers. `safe_text` retains allowed SGR; `patch_offset` and status equivalents rely on literal `diff --git` headers after ANSI removal.
- `src/app.rs`: show/status file jumps and scroll synchronization use patch offsets. Existing graceful behavior handles custom output without matching headers; the example delta configuration must retain navigation.
- `src/ui.rs`: `highlighted_line` already renders ANSI styling and search overlays. Reuse it; no new renderer is needed.
- `src/debug.rs`: snapshots remove styles from displayed lines. Preserve this behavior.
- `tests/debug_mode.rs`: reusable temporary configuration and Git fixtures cover preview/show/status. Existing tests retain unfiltered defaults; new tests explicitly configure deterministic filter commands.

## Tasks & Acceptance

**Execution:**
- [x] `src/config.rs` — add optional filter arguments, disabled default, empty-value disable syntax, and effective help output; test defaults, disabling, custom quoted arguments, malformed values, and help round trips.
- [x] `src/main.rs`, `src/git.rs` — pass configuration into a shared subprocess filter helper; feed raw Git bytes, collect stdout/stderr concurrently with stdin writing, close stdin, reap children, and implement the selected failure policy. Sanitize only after filtering.
- [x] `src/git.rs` — connect preview/show/status presentation paths, including untracked diffs; preserve empty diff behavior and Git exit-code handling. Update module documentation and constructor call sites.
- [x] `src/git.rs`, `tests/debug_mode.rs` — test the matrix with deterministic helper commands, covering transformation in every mode, failures, large bidirectional output, terminal sanitization, and default-compatible file jumps. Keep routine tests independent of an installed delta.
- [x] `gitlsd.conf`, `README.md`, `ARCHITECTURE.md`, `PLAN.md` — add the delta filter to the example configuration; document the disabled default, configuration, disabling, executable requirements, failure behavior, and header-preserving custom filters; record the presentation-filter responsibility and mark the planned feature complete after validation.

**Acceptance Criteria:**
- Given a configured filter, when preview, show, or status loads, then each diff displays the filter's output and effective help identifies the command.
- Given the example delta filter and multiple changed files, when selecting files and navigating diffs, then show/status jumps and selection synchronization continue to work.
- Given filter-produced color sequences, when searching or scrolling the displayed text, then offsets and matches use visible text and debug output contains no escape sequences.
- Given a temporary repository with staged and untracked changes, when opening status and toggling a file's stage state, then repository mutations remain correct and refreshed diffs use the filter.

## Implementation Notes

Implemented shared optional filtering before sanitization, with concurrent pipe handling and sanitized view-specific errors. After human smoke-test feedback, the example uses normal delta presentation and navigation recognizes delta's standard file headers. CSI controls such as delta's `ESC[0K` are removed as complete sequences. Config, process, sanitizer, navigation, and debug integration tests pass (88 unit, 45 integration). Formatting and clippy pass. Local delta smoke passed on a temporary multi-file repository: normal presentation, no leaked CSI text, show/status file jumps, scrolling, search synchronization, and untracked navigation.

## Spec Change Log

- Human testing found that the original `--color-only` example retained Git's raw `+`/`-` presentation and leaked `ESC[0K` as visible replacement text. The example now matches `git --no-pager diff | delta --paging never`; delta header parsing preserves navigation, and the sanitizer removes non-SGR terminal escape sequences atomically.

## Review Triage Log

- `medium` — `GitHistory::new(..., Some(Vec::new()))` could reach `command[1..]` and panic. Patched by destructuring with `split_first` and returning a view-specific filter error; regression-tested.
- `low` — the metadata-bypass assertion used a token the filter did not transform. Patched so the filter transforms that token in diff input and the test proves show metadata remains unchanged.
- `low` — the horizontal-navigation test reset its offset with `home` before inspection. Patched with separate `right` and `end` snapshots.
- `low` — navigation coverage did not exercise a header-preserving filter that inserts lines. Patched with show and status file-jump coverage using inserted lines.
- `low` — missing-filter integration coverage omitted the unstaged operation. Patched with an unstaged-only repository state and diagnostic assertion.
- `low` — raw-byte ordering was inferred rather than directly tested. Patched with invalid UTF-8 and control bytes verified by the filter before sanitization.
- `false` — the large-pipe regression test has no internal deadline. The test completes in milliseconds and the test harness does not provide a portable per-test timeout; adding a subprocess supervisor would test the test runner rather than product behavior. The concurrent pipe behavior is directly exercised with data above pipe capacity.
- `review limitation` — the edge-case and verification-gap review agents could not return findings because their service quota was exhausted. An inline review checked configuration parsing, process lifecycle and errors, all four presentation paths, sanitization order, navigation, staging semantics, documentation, and the acceptance matrix; no further defects were found.

## Design Notes

Investigation confirmed the code map. Collect tracked and untracked unstaged bytes before filtering once. Keep Git execution helpers unchanged; a stdin writer runs concurrently with stdout/stderr collection, with exit diagnostics taking priority over broken-pipe errors. The user explicitly requested implementation of this spec; no intent gaps or irreversible actions remain.

The change has one user-facing goal but crosses configuration, process handling, startup wiring, tests, and documentation. It introduces no irreversible action and has no remaining intent gaps.

The initial `--color-only` example preserved Git's raw `+`/`-` presentation and did not match normal delta output. Human testing changed the intended example to `delta --paging never`. File navigation recognizes delta's standard modified, added, removed, and renamed headers as well as Git patch headers. Custom commands remain user-controlled and must preserve a recognized header form to support file navigation.

## Verification

**Commands:**
- `cargo fmt --check` — formatting passes.
- `cargo test` — unit and debug integration tests pass with deterministic filter configuration.
- `cargo clippy --all-targets -- -D warnings` — no warnings.
- Local delta smoke check on a small temporary Git fixture — colors survive capture and patch headers remain navigable with the example arguments.
