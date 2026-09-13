---
title: 'Make loaded-diff navigation proportional to the viewport'
type: 'refactor'
created: '2026-09-13'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: 'acd5ecf5082b8342eaf124f092610092a8e862cc'
context:
  - '{project-root}/optimization.md'
  - '{project-root}/ARCHITECTURE.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Loaded preview, show, and status diffs are repeatedly scanned and fully converted into Ratatui styling on navigation and redraw. Large Git or delta output therefore makes scrolling cost scale with the entire document and can no longer render offsets beyond `u16::MAX` correctly.

**Approach:** Index recognizable patch headers once when each show or status diff is loaded, and give the interactive UI persistent per-document caches that parse and render only terminal-visible rows. Use indexed direct/binary lookup for explorer-to-patch jumps and patch-to-explorer synchronization while preserving current fallback behavior when custom output has no recognizable headers.

## Boundaries & Constraints

**Always:** Preserve ordinary Git headers, quoted and unquoted paths, custom diff prefixes, rename/copy/add/delete identity, standard delta headers, ANSI styling, Unicode/grapheme-aware horizontal scrolling, search highlighting, debug snapshots, and all current screen/focus behavior. Keep Ratatui types and rendering caches inside `ui`; keep Git header interpretation independent of terminal rendering. Maintain separate show, staged-status, and unstaged-status indexes and caches. Treat application offsets as `usize` and translate windowed content to local paragraph coordinates so large offsets render correctly.

**Never:** Change Git/filter command semantics, make loading asynchronous, change event-loop redraw policy, introduce streaming, add a large-diff truncation policy, or move Ratatui types into `app` or `git`. Do not require custom filters to emit recognizable headers; missing headers must continue to leave selection unchanged or reset the diff offset as current behavior specifies.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Indexed Git/delta diff | Recognizable headers for multiple files, including rename/add/delete | File-to-line lookup is direct and line-to-file lookup chooses the latest patch start at or before the offset | Unmatched file entries remain absent from the index |
| Custom filtered output | No recognizable patch headers | Explorer focus enters the diff at the existing fallback offset and diff scrolling does not force a file selection | No load/render failure |
| Large vertical offset | More than 65,535 lines with the viewport near the end | The requested rows are displayed at their true `usize` offset | Empty/out-of-range windows render safely |
| Search or document change | Query changes, preview reloads, show opens, status refreshes, or status group switches | The relevant cache is invalidated/rekeyed and visible rows reflect current text and highlights | Stale cached rows are never displayed |

</frozen-after-approval>

## Code Map

- `src/git.rs` -- Existing `patch_offset`, `status_patch_offset`, and reverse lookup helpers recognize Git and delta headers but rescan/sanitize the complete diff per file. Reuse `diff_header_paths`, `diff_header_matches_path`, delta matching, and raw-path conversion semantics in a reusable precomputed index; do not change command execution.
- `src/app.rs` -- `ShowState` and `StatusState` own loaded diff documents and call the scan helpers from focus and synchronization paths. Build indexes exactly when diff vectors enter these states, use them for jumps/synchronization, and expose stable document revision/identity needed by UI cache invalidation without exposing Ratatui.
- `src/ui.rs` -- `render_with_states`, `render_show`, and `render_status` currently style complete documents and pass saturated `u16` scroll offsets to `Paragraph`. Add UI-owned persistent caches, visible-window construction, local scrolling, and focused tests around cache invalidation, viewport-bounded parsing, styling/search preservation, and large offsets.
- `tests/debug_mode.rs` -- Existing end-to-end coverage asserts show/status file synchronization and custom-output fallback. Extend only if public behavior needs a regression case that unit tests cannot cover.
- `ARCHITECTURE.md` -- Update module responsibility wording only if the new index/cache responsibilities materially refine the documented boundaries.

## Tasks & Acceptance

**Execution:**
- [x] `src/git.rs` -- introduce a patch index built with one ANSI-stripping pass and file lookup tables; retain compatibility helpers only where useful for tests/callers.
- [x] `src/app.rs` -- store and rebuild independent show/staged/unstaged indexes at every load/refresh, then replace whole-diff navigation scans with indexed lookup.
- [x] `src/ui.rs` -- retain interactive render caches across frames, lazily parse visible rows, invalidate by document/query changes, window preview/show/status diff panes, and avoid global-to-`u16` offset truncation; apply equivalent viewport bounding to log/file explorers where compatible with Ratatui selection behavior.
- [x] `src/git.rs`, `src/app.rs`, `src/ui.rs`, and `tests/debug_mode.rs` -- add focused regression tests for the matrix and prove parsing/lookup work does not grow with off-screen lines during a steady-state redraw or scroll.
- [x] `ARCHITECTURE.md` and module docs -- keep responsibility descriptions consistent with the final implementation.

**Acceptance Criteria:**
- Given a loaded diff with many files, when show/status diff focus moves or search changes the offset, then file synchronization uses the prebuilt sorted index rather than rescanning diff text or rebuilding temporary status files.
- Given a large loaded document and a small terminal, when the UI redraws or scrolls by one row, then ANSI parsing and Ratatui line construction are bounded by visible/ newly visible rows rather than total document length.
- Given an application offset above 65,535, when preview, show, or status diff renders, then it displays the correct source window without saturating the vertical offset.
- Given unchanged source text and search query, when another frame is drawn, then already parsed visible lines are reused.
- Given existing debug and integration scenarios, when the complete test suite runs, then externally observable navigation, styling, search, and fallback behavior remains unchanged.

## Implementation Notes

- Added `PatchIndex`, which sanitizes each loaded line once and provides direct
  file jumps plus binary-search reverse synchronization. Show and staged/unstaged
  status documents rebuild independent indexes and revisions when loaded.
- Added UI-owned per-document caches for preview and diff rows. They parse only
  the visible window plus two rows of overscan, rekey on revision/query changes,
  and crop horizontally before rendering a zero-offset paragraph.
- File explorers window their rows around the application selection. The log keeps
  Ratatui's existing full-list state because its cursor-offset behavior is relied
  on by interactive navigation.
- Added index, cache, viewport, and large-offset unit coverage; existing debug
  integration coverage verifies public show/status synchronization and fallback.

## Spec Change Log

## Review Triage Log

- patch — Header indexing scanned all file rows per recognized header; added a path lookup with suffix-compatible fallback, retaining every matching source-order row.
- defer — Horizontal extent measurement still scans loaded rows, but this was pre-existing and is recorded in `deferred-work.md` for a metrics-cache change.
- false — A document cache intentionally has one slot per source row and may retain styled rows visited by the user; this is the approved no-truncation cache design, not an unbounded temporary allocation.
- false — Status path display conversion was already lossy before indexing; matching all compatible rows preserves the previous observable ambiguity rather than introducing a new one.
- false — Document revisions intentionally identify distinct loaded documents; no semantic state comparison depends on fresh defaults comparing equal.
- patch — Explorer windowing initially recentered selection on each draw; it now advances the prior viewport only when selection leaves it.
- patch — The transient render helper could not prove persistent-cache replacement; added shared-cache preview and status refresh tests.
- low — A generic large-offset cache/window test plus the show rendering test exercise the shared preview/show/status path; no separate duplicate pane test is needed.
- patch — Delta reverse lookup had only direct-jump coverage; added reverse assertions at each delta patch boundary.
- patch — Preview and status cache invalidation lacked interactive shared-cache coverage; added reload and refresh rendering regressions.
- patch — Cached horizontal cropping lacked styled search coverage; added a colored cross-span search crop regression.
- patch — Zero-height panes parsed overscan rows; they now return an empty window.
- false — The path-collision concern reflected existing suffix-compatible matching; the index now retains all matching rows in stable source order.
- patch — App-level indexed navigation remains covered by existing show/status integration tests and now has direct delta reverse-index coverage.

## Design Notes

Use a compact index with `file_to_line: Vec<Option<usize>>` and sorted `(patch_start, file_index)` entries. Build file identity lookup once, strip ANSI from each diff line once, and use binary search for reverse lookup. If duplicate or ambiguous headers occur, preserve the first recognized start for direct jumps and stable source order for reverse lookup.

The UI cache should own `Line<'static>` or an equivalent style-run representation, keyed by a non-Ratatui document revision plus the active query. Allocate cache slots for document length, populate slots only for the visible range, and crop horizontally in `usize` space before constructing a zero-scroll paragraph. A small overscan is acceptable but must remain proportional to viewport height.

## Verification

**Commands:**
- `cargo fmt --check` -- formatting is clean.
- `cargo test` -- unit and debug integration behavior passes.
- `cargo clippy --all-targets --all-features -- -D warnings` -- no lint regressions.
- `cargo build --release` -- optimized application builds successfully.
