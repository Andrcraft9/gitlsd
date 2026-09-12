---
title: 'Adjust automatic preview split'
type: 'bugfix'
created: '2026-09-08'
status: 'done'
baseline_commit: '6e4514815b3da70a8993ff697a67cb4f84b3d030'
route: 'oneshot'
review_loop_iteration: 0
context:
  - '{project-root}/README.md'
  - '{project-root}/CONTRIBUTING.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Automatic preview orientation compares terminal columns directly with rows, so a terminal occupying half of a 3820x2400 display can be treated as wide and rendered side by side even though its physical shape calls for a stacked split.

**Approach:** Account for the approximate 1:2 width-to-height aspect ratio of terminal cells when choosing the split. Use a shared, tested orientation helper so rendering and horizontal-scroll viewport calculations cannot disagree, and mark the plan item complete.

</frozen-after-approval>

## Implementation Notes

- Added one `preview_layout_direction` helper in `src/ui.rs` and reused it for rendering and active-pane width calculation. The side-by-side threshold is now wider than 2:1 in terminal cells, approximating square physical dimensions for cells that are twice as tall as they are wide.
- Added boundary coverage for the orientation rule and changed the stacked renderer case to a terminal that has more columns than rows, reproducing the old misclassification.
- Strengthened renderer coverage by asserting the preview title appears at the expected coordinates in both orientations.
- Updated `README.md` with the automatic-layout behavior and completed the corresponding `PLAN.md` item. No architecture boundary changed.

## Review Triage Log

| Verdict | Evidence and route |
| --- | --- |
| low / patched | The orientation threshold is defined over the drawable content area after reserving the status row. Renamed the helper argument and boundary test to make that scope explicit. |
| low / patched | The renderer test previously checked only visible text. It now asserts the preview title at the expected pane coordinates for both orientations. |
| false | `in-progress` was the required workflow status during review; finalization now sets it to `done`. |
| false | The legacy-config warning suggestion concerns an unrelated TOML migration draft that appeared in the worktree after this change started; this preview-split change neither created nor edits that draft. |
| false | The normalized-key collision suggestion concerns the same unrelated TOML migration draft, not code or specification changed by this work. |
| false | The source-location acceptance suggestion concerns the same unrelated TOML migration draft, not code or specification changed by this work. |

## Verification

**Commands:**

- `cargo fmt --check` -- formatting is clean.
- `cargo clippy --all-targets --all-features -- -D warnings` -- no lint warnings.
- `cargo test --all-targets --all-features` -- all 63 unit and integration tests pass.
- `git diff --check` -- no whitespace errors.
