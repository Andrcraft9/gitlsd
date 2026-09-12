---
title: 'Migrate configuration to TOML'
type: 'feature'
created: '2026-09-08'
status: 'ready-for-dev'
route: 'dispatch'
review_loop_iteration: 0
context: ['{project-root}/README.md', '{project-root}/CONTRIBUTING.md', '{project-root}/ARCHITECTURE.md']
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** gitlsd maintains a custom line tokenizer and directive parser for configuration, creating avoidable parsing code and shell-like quoting rules for values that are ultimately executed as an argv list.

**Approach:** Replace the line-oriented format with typed TOML deserialization using `serde` and `toml`. Represent Git commands as string arrays and bindings as a table, while preserving runtime defaults, validation, direct process execution, and effective-configuration output.

## Boundaries & Constraints

**Always:** Parse a strict TOML schema; overlay supplied values and bindings onto `Config::default()`; preserve argv elements exactly; execute Git directly without a shell; retain command-prefix, positive-batch-size, reserved-key, action, and one-line-log validation; report the config path and useful source location for invalid input; keep missing-default and explicit-file I/O behavior; update the shipped sample and user documentation.

**Never:** Interpret command arrays as shell strings; silently accept unknown TOML fields; retain or translate the old directive syntax; change Git pagination, rendering, search, preview, key vocabulary, default values, or the public runtime shape of `Config`.

**Decision:** Use `$HOME/.config/gitlsd/config.toml` as the default path and rename repository-owned configuration samples and test files to `.toml`. Legacy default-path migration behavior is out of scope.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Empty or partial file | Empty TOML or only one setting/table entry | Unspecified settings and bindings retain defaults; supplied bindings override matching defaults | N/A |
| Exact argv | Arrays containing spaces, empty strings, `=`, `#`, quotes, backslashes, or `--` pathspec | Every array element reaches `Config` unchanged and help remains unambiguous | Wrong TOML value types are rejected with path and location |
| Custom bindings | `[bindings]` maps literal or named keys to action names | Valid entries overlay defaults | Unknown/reserved keys and unknown actions are rejected |
| Command policy | Valid `git log` and `git show` arrays | Existing direct execution and one-row-per-commit behavior continue | Wrong prefixes and incompatible multiline log options are actionable errors |
| Invalid schema | Malformed TOML, duplicate keys, unknown top-level field/table, or zero batch size | No configuration is applied | Error identifies the source file and useful line/column where available |

</frozen-after-approval>

## Code Map

- `Cargo.toml`, `Cargo.lock` -- add compatible `serde` derive and `toml` dependencies and lock them for Rust 1.85.
- `src/config.rs` (`Config::parse`, `ConfigError`, `Key`, `Action`, `validate_log`) -- deserialize a private strict TOML overlay, apply it to defaults, reuse existing vocabulary and domain checks, preserve source-aware diagnostics, and remove `tokenize`, directive parsing, and optional-separator handling.
- `src/config.rs` tests -- replace custom-syntax fixtures and cover defaults, exact arrays, strict schema/type handling, validation failures, binding overlay, reserved input keys, and diagnostic locations.
- `tests/debug_mode.rs` -- express all test configurations as TOML while retaining end-to-end coverage for loading, Git argv, pagination/pathspecs, help, bindings, preview, and errors; use a TOML-oriented temporary suffix.
- `README.md` -- document TOML arrays and `[bindings]`, the chosen default filename, strict validation, and direct non-shell execution.
- `gitlsd.conf` → `gitlsd.toml` -- rename and convert the shipped example to the selected format.
- `src/git.rs` (`pair_records`) and `src/config.rs` validation messages -- replace legacy `set log ...` guidance with TOML array examples without changing execution behavior.
- `src/cli.rs` (`Cli::config`) -- identify `--config` input as TOML; keep option semantics unchanged.
- `ARCHITECTURE.md`, `PLAN.md`, application/UI modules, and `tests/fixtures/mapbox-gl-js` -- no change expected because module boundaries, feature scope, and fixture state remain unchanged.

## Tasks & Acceptance

**Execution:**
- [ ] `Cargo.toml`, `Cargo.lock` -- add the deserialization dependencies with the smallest required feature set.
- [ ] `src/config.rs` -- implement the strict TOML overlay and source-aware validation; delete the custom tokenizer/parser; update focused unit tests.
- [ ] `tests/debug_mode.rs` -- migrate configuration fixtures and retain behavioral assertions.
- [ ] `README.md`, sample configuration, `src/cli.rs`, `src/git.rs` -- publish the new syntax and remove stale directive-form guidance.
- [ ] Repository validation -- format, lint, test all targets/features, verify release help, whitespace, and fixture cleanliness.

**Acceptance Criteria:**
- Given the current default, partial override, and custom Git-command scenarios, when equivalent TOML configurations are loaded, then effective runtime values and application behavior match the pre-migration behavior.
- Given command arrays with significant characters or empty elements, when configuration is parsed and displayed, then argument boundaries and values are preserved without shell evaluation.
- Given invalid syntax, schema, keys, actions, commands, or batch size, when loading configuration, then startup fails with an actionable source-aware error.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Design Notes

Deserialize into a private optional-field representation rather than deriving directly onto public `Config`: this supports partial files and binding overlays while keeping runtime types stable. Use deserialization-backed validation or spanned values where needed so domain errors retain TOML source locations. A representative file is:

```toml
log = ["git", "log", "--oneline", "--format=%h %an: %s"]
batch-size = 100

[bindings]
j = "move-down"
":" = "command"
```

## Verification

**Commands:**
- `cargo fmt --check` -- expected: no formatting diff.
- `cargo clippy --all-targets --all-features -- -D warnings` -- expected: no warnings.
- `cargo test --all-targets --all-features` -- expected: all unit and fixture-backed debug tests pass.
- `cargo build --release && target/release/gitlsd --help` -- expected: release build succeeds and help omits debug-only options.
- `git diff --check && git -C tests/fixtures/mapbox-gl-js status --short` -- expected: no whitespace errors and no fixture changes.
