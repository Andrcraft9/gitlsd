---
title: 'Always use the HOME configuration path'
type: 'refactor'
created: '2026-09-05'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** gitlsd's default configuration path must always be `$HOME/.config/gitlsd/config`.

**Approach:** Use only the HOME-relative default, retain explicit `--config PATH` behavior, and update tests and README documentation to describe and verify it.

</frozen-after-approval>

## Implementation Notes

- Simplified default discovery in `src/config.rs` to derive the path only from non-empty `HOME`.
- Replaced conditional-path coverage with an integration test that verifies HOME-based discovery.
- Updated `README.md` to state the default path directly.
- Verified with `cargo fmt --check`, `cargo test`, and `git diff --check`; local review found no additional issues.
