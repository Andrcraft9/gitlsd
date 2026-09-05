---
title: 'Git-backed configurable log browser MVP'
type: 'feature'
created: '2026-09-05'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '39cfc2d320b86b0c1441166259ea88695987f21a'
context: ['{project-root}/README.md', '{project-root}/CONTRIBUTING.md', '{project-root}/ARCHITECTURE.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** gitlsd has only a placeholder executable and needs a usable, fast MVP whose structure can support later status and diff modes without changing the project fundamentals.

**Approach:** Build a Ratatui log browser whose repository data always comes from configurable Git CLI commands, with configurable bindings, incremental history loading, less-style search, command/help screens, and a debug-only deterministic interaction interface used by integration tests.

## Boundaries & Constraints

**Always:** Treat Git CLI output as the source of truth; execute commands as argv without a shell; disable Git paging; keep UI state/event handling independently testable from terminal I/O; load history in configurable batches; include every action binding in configuration and show the effective configuration in help; restore the terminal on every interactive exit path; use `tests/fixtures/mapbox-gl-js` for end-to-end behavior.

**Never:** Reimplement Git object/history traversal; add status or diff modes; make Enter on a log row perform an action; expose scripted debug interaction in release builds; commit, push, or mutate the fixture repository.

## Architecture Documentation Contract

`ARCHITECTURE.md` must describe the stable, top-level shape that future features build on rather than mirror current source files. It must reflect:

- **Purpose and principles:** gitlsd is a terminal application and orchestration layer over the installed Git CLI. Git owns repository semantics; gitlsd owns configuration, interaction state, and presentation.
- **System boundaries:** configuration translates user-authored `set` and `bind` commands into effective settings/actions; the application core coordinates screens, selection, search, commands, and incremental loading; a Git boundary executes configured commands and returns displayable records; frontends translate either terminal events or debug scripts into the same actions and render the resulting state.
- **Dependency direction:** terminal, filesystem, environment, and child-process concerns stay at the edges. Core application behavior depends on abstractions/data rather than Ratatui, terminal input, or concrete process execution, so it can be tested deterministically.
- **Primary flows:** startup/configuration resolution, initial log loading, lazy pagination, input-to-action dispatch, search/command handling, rendering, and clean shutdown/error restoration.
- **Configuration as a first-class interface:** Git commands, batch size, and bindings are policy supplied by defaults plus user configuration, not scattered constants. Effective configuration is inspectable through help.
- **Git integration invariant:** all repository history comes from CLI Git output; pagination augments the configured log command and never introduces an internal Git implementation.
- **Test architecture:** debug mode and interactive mode drive the same application state machine; debug mode substitutes scripted input and stable text output at the outer boundary and exists only in debug builds.
- **Evolution model:** log is the first mode; later status/diff modes should attach through the same configuration, command-execution, action, and presentation boundaries without restructuring the core.

It must intentionally omit module inventories, concrete type names, parsing algorithms, exact rendering layout, and other implementation details likely to change during normal development.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Initial log | Run inside a repository | First configurable batch is shown and first row selected | Git failures appear as actionable errors and terminal is restored |
| Pagination | Selection reaches loaded tail with more history | Next batch is fetched and appended without duplicates | Empty batch marks end; failed batch preserves existing rows |
| Search | `/`, query, Enter; then next/previous match | Matching row is selected with wraparound | Empty/no-match query leaves selection and reports status |
| Commands | `:help`/`:h` or `:quit`/`:q` | Help shows effective settings/bindings, or app exits | Unknown command reports status without exiting |
| Debug run | Debug binary receives semicolon-separated keys | Same state machine runs without a TTY and prints stable final state to stdout | Invalid scripted keys return a nonzero error |
| Configuration | Optional config contains `set`/`bind` commands | Defaults are overridden, including log command and batch size | File/line and reason are reported for invalid directives |

</frozen-after-approval>

## Code Map

- `Cargo.toml` / `Cargo.lock` -- existing Clap and Ratatui baseline; add only direct terminal/argv parsing support needed by the MVP.
- `src/main.rs` -- replace placeholder with thin CLI/bootstrap entry point.
- `src/lib.rs` and `src/{cli,config,git,app,ui,debug}.rs` -- establish boundaries between configuration, Git process adapter, application state/actions, rendering, and debug harness.
- `tests/debug_mode.rs` -- exercise the compiled binary in the 13,686-commit Mapbox fixture for navigation, paging, search, help, and quit.
- `tests/fixtures/mapbox-gl-js` -- initialized read-only Git submodule; do not edit or traverse its large working tree.
- `ARCHITECTURE.md` -- replace placeholder with the documented architecture contract: principles, boundaries, dependency direction, runtime flows, Git/configuration invariants, testing seam, and extension model; omit source-level design.
- `README.md` -- document usage, configuration grammar/default location, controls, and debug-build testing contract.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/cli.rs` -- establish executable/library boundary and normal versus debug-build CLI paths.
- [x] `src/config.rs` -- implement defaults, XDG/home or explicit config discovery, tigrc-like `set log`, `set batch-size`, and `bind` directives with validation.
- [x] `src/git.rs` -- run the configured log argv via Git-compatible pagination arguments, parse one record per commit, and surface process errors.
- [x] `src/app.rs` -- own screen/input state, selection, lazy fetch, search navigation, commands, and configurable action dispatch.
- [x] `src/ui.rs`, `src/debug.rs` -- provide interactive Ratatui rendering and a stable plain-text snapshot of the same state machine in debug builds.
- [x] `tests/debug_mode.rs` -- cover primary interactions and configuration against the production-history fixture.
- [x] `README.md`, `ARCHITECTURE.md` -- document behavior and the complete architecture contract without coupling it to current modules or types.

**Acceptance Criteria:**
- Given a debug build in the fixture repository, when scripted down/up keys run, then stdout identifies the expected selected commit and visible log state.
- Given a batch size smaller than the scripted downward movement, when selection crosses a batch boundary, then additional distinct commits are loaded from Git.
- Given a searchable commit subject, when scripted search and repeat-search actions run, then selection moves according to less-style forward/backward wraparound.
- Given help and quit commands, when entered interactively or by script, then help reflects effective bindings/settings and quit exits cleanly.
- Given a release build, when help is inspected, then no `--debug` option is available.
- Given a future contributor reads `ARCHITECTURE.md`, when locating ownership and extension points, then the Git, configuration, application, frontend, and test boundaries and their dependency direction are unambiguous without requiring source-level implementation details.
- Given the repository checks, when format, lint, and tests run, then all complete successfully without fixture changes.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| ID | Verdict | Evidence and route |
| --- | --- | --- |
| BH-01 | medium | Confirmed that Ctrl-C was consumed by text-entry modes; patched by resolving configured control-key quit before mode input and covered by an app test. |
| BH-02 | medium | Confirmed that quoted empty arguments disappeared; patched with explicit token-start tracking and covered by configuration parsing tests. |
| BH-03 | medium | Confirmed that all standalone `=` tokens were removed; patched so only the optional grammar separator is removed, with literal-key and argv coverage. |
| BH-04 | medium | Confirmed that empty or relative XDG homes produced an invalid config location; patched to require a non-empty absolute path and fall back to HOME. |
| BH-05 | medium | Confirmed that `Path::exists` hid non-not-found filesystem failures; patched to read directly and suppress only `NotFound`, with regression coverage. |
| BH-06 | medium | Confirmed that output-expanding Git options broke record parsing; patched at the Git boundary to enforce machine output and covered against configured graph/patch/stat options. |
| BH-07 | low | Confirmed that the unit separator could collide with repository text; patched with NUL-framed fields and records. |
| BH-08 | high | Confirmed that repository control bytes reached both renderers; patched with lossy decoding and control-character neutralization, with focused tests. |
| BH-09 | medium | Confirmed that effective help was clipped without navigation; patched with independent help scrolling and small-terminal rendering coverage. |
| BH-10 | medium | Confirmed that joined help text lost argv boundaries; patched with reversible per-argument quoting and an exact help assertion. |
| BH-11 | medium | Confirmed that the debug grammar could not express a semicolon key; patched with the named `semicolon` key and end-to-end dispatch coverage. |
| BH-12 | low | The integration suite is debug-gated, but the prescribed release build/help check directly verified that `--debug` is absent; rejected because a nested release build test would add disproportionate complexity. |
| BH-13 | low | Confirmed that the fixture error assertion depended on Git's locale; patched by forcing the C locale for spawned integration processes. |
| EC-01 | medium | Same reachable Ctrl-C text-entry defect as BH-01; patched and verified by the same app regression test. |
| EC-02 | low | Confirmed that Alt-modified characters could trigger ordinary bindings; patched by rejecting unsupported Alt combinations and covered in the key-translation table. |
| EC-03 | medium | Same literal-equals defect as BH-03; patched and verified by parser and binding tests. |
| EC-04 | medium | Same quoted-empty-argument defect as BH-02; patched and verified by parser tests. |
| EC-05 | medium | Same XDG resolution defect as BH-04; patched and verified by unit and process-level discovery tests. |
| EC-06 | medium | Same semicolon debug-input defect as BH-11; patched and verified end to end. |
| EC-07 | medium | Same configured output-option defect as BH-06; patched and verified against the real fixture. |
| EC-08 | low | Same delimiter collision as BH-07; patched with NUL framing and parser coverage. |
| EC-09 | low | Confirmed that strict UTF-8 decoding rejected legacy metadata; patched with lossy sanitized decoding and invalid-byte coverage. |
| EC-10 | maybe-false | Ref movement could destabilize offset pagination, but expected live-session snapshot semantics are unspecified and no reproduction was established; deferred pending a mutation scenario and product decision. |
| EC-11 | false | The shipped Git source returns a short page only when the configured history is exhausted; no concrete nonterminal short-page path was found. |
| EC-12 | low | Public state could be corrupted by a hypothetical library consumer, but the shipped frontends preserve the invariants and no public library API is promised; rejected as unlikely and accessor conversion would add nonessential surface. |
| EC-13 | medium | Confirmed that alternate-screen entry failure did not attempt complete cleanup; patched with best-effort raw-mode, screen, and cursor restoration. |
| VG-01 | medium | Duplicate suppression had no overlapping-page test; patched with a source that repeats a commit across adjacent pages. |
| VG-02 | medium | Custom bindings were parsed and displayed but not dispatched in tests; patched with fixture-backed literal and named-key behavior checks. |
| VG-03 | medium | Configured Git argv was not observed at the process boundary; patched with a fixture filter that changes the selected history. |
| VG-04 | medium | Default configuration discovery had no process-level coverage; patched with an isolated absolute XDG home and no explicit config argument. |
| VG-05 | medium | Physical-key translation covered only Ctrl-C and Enter; patched with a table covering every supported key and unsupported modifiers/codes. |
| VG-06 | medium | Ratatui output was not verified; patched with TestBackend assertions for selected log rows, status, help, and scrolling. |
| VG-07 | false | A real PTY launch/quit smoke check ran during review and exited zero while emitting alternate-screen leave and cursor-show restoration sequences. |
| VG-08 | low | Same release-assertion concern as BH-12; rejected because the explicit release help verification ran and confirmed omission of `--debug`. |

## Design Notes

Configuration is line-oriented and command-like. The configured log command remains user-authored while gitlsd adds batch pagination arguments. A single action vocabulary connects physical keys, scripted keys, and application transitions, so tests exercise production behavior rather than a parallel test model.

## Verification

**Commands:**
- `cargo fmt --check` -- expected: no formatting diff.
- `cargo clippy --all-targets --all-features -- -D warnings` -- expected: no warnings.
- `cargo test --all-targets --all-features` -- expected: unit and fixture-backed debug tests pass.
- `cargo build --release && target/release/gitlsd --help` -- expected: release help omits `--debug`.
