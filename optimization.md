# Performance investigation and optimization plan

## Summary

The reported delays come from three independent costs that compound:

1. Preview and diff loading is synchronous. Git and the configured filter must
   finish before the event loop can process another key or draw the new state.
2. Every draw reparses and allocates styled data for every loaded diff line,
   even though only a terminal-sized window is visible.
3. Show/status vertical diff navigation rediscovers every file's patch offset
   on every key press by repeatedly scanning the whole diff.

Delta makes all three symptoms more visible. It is intrinsically substantial
work, it emits more bytes and many more ANSI sequences than Git, and its richer
output makes the repeated parsing and scanning in gitlsd more expensive. This
is not primarily a pipe implementation problem: the filter pipes are drained
concurrently and do not deadlock, but the whole pipeline is blocking and fully
buffered.

The highest-value changes are to index patch locations once, make rendering
viewport-aware with cached ANSI parsing, and move preview/filter work off the
input thread with cancellation or latest-request-wins semantics.

## Evidence and method

The code paths were inspected in `src/app.rs`, `src/git.rs`, and `src/ui.rs`.
As a representative large commit, the repository's
`1fd254be57678f1ea2970fd00bcc392c455e086e` changes 234 files and adds 24,294
lines. Commands were run from a warm filesystem cache on 2026-09-13. These are
microbenchmarks rather than end-to-end TUI profiles, but they isolate the cost
of the configured filter:

| Presentation command | Lines | Bytes | CSI/SGR sequences | Wall time (3 runs) |
| --- | ---: | ---: | ---: | ---: |
| `git show --stat --patch --color=always` | 25,943 | 2,122,533 | 89,813 | 0.03-0.04 s |
| same output through `delta --paging never` | 26,177 | 4,163,630 | 189,061 | 2.65-2.74 s |

For this fixture, delta roughly doubles the byte and escape-sequence volume and
adds about 2.6 seconds of CPU work before gitlsd can display anything. The same
larger result is then reparsed by the UI on every redraw and repeatedly stripped
of ANSI while synchronizing file selection.

Before implementation, add repeatable measurements for:

- selection-to-first-paint latency while moving through log commits;
- frame construction time for preview/show/status at 1k, 10k, and 100k lines,
  with raw Git color and delta output;
- per-key time in show/status diff navigation with increasing file counts;
- idle CPU usage and queued-key latency;
- load-stage timings for Git, filter execution, sanitization, indexing, and
  render-cache construction.

Use release builds for these measurements. A Criterion-style microbenchmark for
ANSI parsing/render preparation and patch indexing would make regressions easy
to catch; an end-to-end pseudo-terminal benchmark should cover input latency.

## Bottlenecks

### 1. Synchronous preview reload blocks navigation (critical)

`App::move_down`, `move_up`, page navigation, and log search call
`reload_preview`, which calls `HistorySource::load_preview` synchronously.
`GitHistory::load_preview` waits for Git, then waits for the full filter process,
then sanitizes the complete output. `app.initialize` also loads the first
preview before starting the terminal UI.

Consequences:

- A single navigation key can block for the complete Git plus delta runtime.
- No loading state can be painted because the same thread owns input, loading,
  and drawing.
- Keys typed while blocked queue in the terminal. The loop consumes only one
  event per iteration, so stale selections may each trigger another expensive
  preview load before the application catches up.
- Page Down avoids intermediate preview loads deliberately, but ordinary held
  `j`/Down input and searches do not coalesce.
- Clearing `preview_lines` before the blocking call cannot be observed by the
  user because no draw occurs during the call.

Delta is the dominant load cost for large diffs in the sample above, but even
without a filter, large Git diffs and sanitization still block input.

### 2. Every frame rebuilds the entire styled document (critical)

The render functions for preview, show diff, and status diff iterate every
stored line, call `highlighted_line`, collect a new `Vec<Line>`, and only then
give it to a scrolled `Paragraph`. The same pattern also processes all log and
file-list rows. Vertical scrolling changes an offset, but causes the entire
document to be rebuilt.

`highlighted_line` calls `styled_line`, which scans ANSI sequences, allocates a
span vector, parses SGR numbers into another vector for every escape sequence,
and creates Ratatui spans. With an active search it additionally constructs the
plain line, case-folded text, source mappings, match boundaries, and owned
highlight spans.

Delta makes this substantially worse because its syntax and side-by-side line
styling produces many short colored spans. The representative output contains
about 189,000 CSI/SGR sequences and 4.2 MB of text; all of it is parsed and its
temporary render representation allocated again for every frame, although a
typical pane displays only tens of lines.

### 3. File synchronization is repeated whole-diff work per key (critical for
show/status diff focus)

After every vertical show/status diff movement,
`sync_show_selection_to_diff`/`sync_status_selection_to_diff` calls
`file_at_patch_offset`/`status_file_at_patch_offset`. Those functions iterate
every changed file and call `patch_offset` for each one. `patch_offset` scans
from the start of the diff until it finds that file's Git or delta header.

This is approximately O(files × diff lines) per navigation action. For the
234-file, 26,177-line sample, the loose upper bound is about 6.1 million line
visits for one key press. Actual early exits reduce that bound, but a large
fraction of the diff is still revisited for files whose headers occur late.

Each visited line calls `safe_text`, allocating a new ANSI-free `String`.
Non-`diff --git` lines may also sanitize the following line to detect delta's
horizontal rule. Status adds temporary `ChangedFile` construction and path
conversion. Delta therefore increases both the bytes scanned and allocations.

Entering diff focus (`patch_offset` for one selected file) also scans from the
start, but the per-key all-files synchronization is much more damaging.

### 4. The event loop redraws when nothing changed and serializes queued input
(high)

The UI loop draws first, polls for up to 250 ms, reads at most one event, handles
it, and repeats. It therefore rebuilds the screen about four times per second
while idle. Under load, each queued key is separated by another expensive full
draw. Ratatui can avoid writing unchanged cells to the terminal, but gitlsd has
already paid to construct and parse all widget content before buffer diffing.

This behavior amplifies bottleneck 2 and makes the application feel behind the
keyboard after any blocking operation.

### 5. Horizontal navigation rescans every line (high for large diffs)

Every Left, Right, Home, or End action calls `max_horizontal_offset`. For diff
panes it walks every line, calls `safe_text` to allocate an ANSI-free string,
computes Unicode display width, and finds the maximum. This value is invariant
until the document or viewport width changes and should not be recomputed for
each horizontal key.

### 6. Show and status perform several sequential commands (medium)

Opening show runs metadata, file-list, and diff Git commands sequentially, then
runs the filter for the diff. Opening status sequentially discovers the root,
loads porcelain status, loads and filters staged diff, loads the tracked
unstaged diff, runs one additional `git diff --no-index` process per untracked
file, and filters the combined unstaged result.

Specific consequences:

- Independent show metadata/file-list work adds directly to diff/filter latency.
- Repositories with many untracked files suffer process-per-file overhead.
- Both staged and unstaged documents are eagerly generated even though only one
  status group is active.
- `toggle_status_stage` loads all status and both filtered diffs before the
  mutation and again afterward, so a single stage action can invoke delta up to
  four times, in addition to Git commands.

These costs mostly affect opening/refreshing modes, not ordinary scrolling, but
they share the same unresponsive synchronous execution model.

### 7. Whole-output buffering and repeated representations increase memory
(medium)

Git `Command::output` buffers the entire raw diff. Filtering concurrently writes
that buffer while collecting the complete stdout and stderr into new buffers.
The result is then split and sanitized into separately allocated strings. During
rendering, another full vector of lines/spans is created temporarily on every
frame.

Whole buffering is simple and currently safe from pipe deadlock, but peak memory
and allocator pressure scale with diff size. Delta's output expansion increases
both. This becomes more important once CPU hotspots are removed.

### 8. Search work is uncached and can be superlinear (lower priority unless
search is active)

Search navigation repeatedly strips ANSI, lowercases candidate lines, and scans
until a match. Rendering with an active query recomputes match ranges for every
line on every frame. `case_insensitive_match_ranges` tests the query at every
character and uses repeated linear searches through source mappings and
graphemes for matches. Lines with many overlapping matches can therefore do
substantially more than linear work.

This does not explain slow scrolling when no search is active, but it can make a
large diff much slower after a search has been submitted.

### 9. Offsets are truncated at 65,535 during rendering (scalability issue)

Application offsets are `usize`, but preview/show/status paragraph offsets are
converted to `u16` and saturated at `u16::MAX`. Diffs beyond 65,535 lines cannot
be rendered at their true vertical or horizontal application offset. This is
not a current CPU bottleneck, but viewport slicing would fix it naturally and
should be covered while large-document rendering changes.

## Recommended changes

### Priority 0: make loaded-diff navigation proportional to the viewport
Status: Done, 1ff6176c4ef5b04e360c59c977f0b9a39145a407

#### A. Build a patch index once

When a diff is loaded, parse its headers once and create both mappings needed by
the application:

- file index to patch-start line, for Enter from the explorer;
- sorted patch-start line to file index, for selection synchronization while
  scrolling.

Use direct lookup for file-to-line and binary search (or a moving cursor for
single-line scrolling) for line-to-file. Preserve the existing graceful
behavior when a custom filter removes recognizable headers. The parser should
strip ANSI once per line, not once per file per navigation action.

Target complexity is O(diff bytes + files) once at load, then O(1) for a file
jump and O(log files), or amortized O(1), per scroll key. Store separate indexes
for show, staged status, and unstaged status.

#### B. Cache parsed styling and render only visible rows

Introduce a UI-owned document/render cache that is invalidated only when source
lines or the search query change. At minimum, pass only the visible vertical
slice (plus a small overscan) to Ratatui and parse ANSI only for those rows.
Prefer caching each parsed line so scrolling by one row parses one new row
rather than the entire viewport again.

The cache should retain:

- parsed owned spans or an equivalent compact style-run representation;
- plain text without ANSI for search/header operations;
- display width;
- optional cached search-highlight variants keyed by the active query.

Keep Ratatui-specific objects in `ui`; alternatively define a UI-independent
styled-text representation if debug rendering also benefits. Do not put
Ratatui types into the Git adapter. Windowing should translate the `usize`
application offset into a local zero-based paragraph, avoiding the `u16` clamp.

Apply the same windowing principle to the log and file explorers as their
collections can also grow, but prioritize the three diff panes.

### Priority 1: keep input responsive while content loads

#### C. Make preview loading asynchronous and latest-request-wins

Move Git/filter execution away from the terminal event thread. A preview request
should carry a monotonically increasing generation or commit ID; publish a
result only if it still matches the selected commit. Draw the selection and a
loading indicator immediately.

Debounce rapid log movement briefly (for example, 50-100 ms) so holding a key
does not launch delta for every transient selection. Cancel obsolete Git/filter
children where practical; otherwise bound the worker count and discard stale
results. Ensure quitting and repository changes reap children cleanly.

An LRU cache keyed by commit ID plus the effective preview/filter commands will
make moving back to recently viewed commits instant. Bound it by output bytes,
not only entry count.

Show and status loading can use the same worker/result mechanism after preview
is stable. Their current screen may remain visible with an explicit loading
state until the requested result arrives.

#### D. Redraw on changes, and drain/coalesce input

Draw initially and after state changes, resize events, or async results instead
of on every 250 ms timeout. If a timer remains necessary, use it only for real
animations/progress state. Drain already queued input before expensive preview
work and coalesce repeated navigation to the final selection.

This change complements asynchronous loading; merely draining keys while loads
remain synchronous would still launch stale work from `handle_key`.

#### E. Cache document metrics

Compute maximum visible line width when a document is loaded or when its parsed
cache is built. Recompute only after content or viewport changes. Horizontal
navigation then becomes O(1). The same cache can provide ANSI-free text for
search and patch indexing.

### Priority 2: reduce initial-load and refresh work

#### F. Avoid redundant/eager subprocess work

- Run independent show metadata and file-list queries concurrently with the
  diff pipeline, or combine compatible Git queries where doing so does not blur
  error reporting or configured-command semantics.
- Load/filter only the active status group's diff initially; load the other on
  first Tab or in a background task.
- After staging, refresh discovery first and generate visible diff content once
  after the mutation. Preserve the existing stale-selection safety check using
  lightweight porcelain status rather than a full pre-mutation diff refresh.
- Replace process-per-untracked-file generation with a bounded parallel strategy
  or a safe batched approach. Retain exact handling for unusual path bytes and
  `--no-index` exit code 1.
- Reuse already loaded results only when commit ID, repository state, and the
  exact configured commands/filter match. Configuration-driven semantics make
  broad implicit reuse unsafe.

#### G. Stream and preprocess the pipeline

After responsiveness and rendering are fixed, connect Git stdout to the filter
stdin directly and process filter stdout incrementally. Sanitize, split lines,
compute plain text/width, and build the patch index as output arrives. This
reduces peak memory and enables progressive display.

Streaming needs explicit limits and lifecycle handling: cap retained output or
define a large-diff policy, drain stderr, propagate failures, cancel both Git and
filter children together, and never publish partial output as a successful
result. It is a larger change than caching and should not be the first fix.

#### H. Cache search data and improve matching

Reuse the ANSI-free text created during preprocessing. Cache case-folded text or
a search index only when justified by measurement, because it adds memory.
Replace repeated mapping/grapheme scans in `case_insensitive_match_ranges` with a
single-pass mapping from folded byte boundaries to source grapheme boundaries.
Invalidate highlighted render rows only when the query changes.

## Suggested implementation sequence

1. Add timing/scale benchmarks and reproduce with raw Git and delta fixtures.
2. Add one-pass patch indexes and replace per-key `file_at_patch_offset` scans.
3. Add viewport-aware, cached ANSI/style conversion and cached line widths.
4. Change redraw scheduling so idle time and queued input do not trigger full
   document reconstruction.
5. Add an asynchronous, debounced, cancellable preview loader with stale-result
   rejection and a byte-bounded LRU cache.
6. Extend asynchronous/lazy loading to show and status, then remove redundant
   status refresh work.
7. Consider streaming only after profiling the preceding changes.
8. Optimize active-search processing if benchmarks still show it materially in
   frame time.

Steps 2 and 3 are focused internal changes with the strongest scrolling payoff.
Step 5 is the main fix for the log's delayed navigation response when delta is
configured. Avoid starting with thread-pool or streaming infrastructure before
removing the deterministic repeated scans and redraw allocations.

## Acceptance targets

Define exact thresholds on the project's supported test machine, but useful
goals are:

- Vertical scrolling in an already loaded 100k-line delta document has p95
  key-to-frame latency below 16 ms and does not grow with total line count.
- Show/status file synchronization per key grows with patch count at most
  logarithmically and stays below 1 ms for 1,000 files.
- Idle UI CPU is effectively zero; no redraw occurs without an event or changed
  async state.
- Log selection paints within one frame even if preview generation takes
  seconds; obsolete preview results never replace the current selection.
- Holding navigation launches at most one active preview pipeline and settles on
  the final selected commit without replaying stale previews.
- Horizontal navigation is independent of total diff size after load.
- Existing ANSI safety, search highlighting, Git/delta header navigation,
  configured command behavior, filter error reporting, and status mutation
  tests continue to pass.
- Diffs beyond 65,535 lines can scroll to the actual end.

## Expected impact

Patch indexing removes the worst per-key algorithm in show/status. Cached,
windowed rendering makes all preview/diff modes depend on terminal height rather
than total diff size and prevents delta's styling density from being paid on
every frame. Asynchronous latest-wins preview loading cannot make delta itself
finish faster, but it removes that external cost from input latency and avoids
doing it for transient selections. Together these changes address both the
delta-specific amplification and the less visible unfiltered performance
problem.
