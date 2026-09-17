- source_spec: `/home/andr/code/git-tools/gitlsd/_specs/implementation-artifacts/spec-gitlsd-mvp.md`
  summary: Determine whether pagination should snapshot repository refs when history changes during a running session.
  evidence: The review could not establish the expected behavior if refs move between offset-based page loads; reproducing a concurrent ref update and deciding whether sessions promise snapshot consistency would settle it.
- source_spec: `/home/andr/code/git-tools/gitlsd/_specs/implementation-artifacts/spec-horizontal-navigation-extensions.md`
  summary: Avoid recomputing and reallocating every loaded row's horizontal rendering on each scroll or redraw.
  evidence: The review verified this is pre-existing horizontal-scroll rendering work rather than caused by Home/End or mouse support; it needs a broader caching or viewport-rendering design.
- source_spec: `/home/andr/code/git-tools/gitlsd/_specs/implementation-artifacts/spec-encapsulate-application-state.md`
  summary: Resolve the misleading show explorer offset ownership between App and Ratatui ListState.
  evidence: The application reports explorer_offset, but the interactive viewport is independently owned by Ratatui's ListState; this is REVIEW finding 8 and predates the state-visibility refactor.
- source_spec: `/home/andr/code/git-tools/gitlsd/_specs/implementation-artifacts/spec-optimization-priority-0.md`
  summary: Cache loaded-document display widths so horizontal navigation does not rescan every row.
  evidence: The review verified that max-horizontal-offset still sanitizes and measures all rows, a pre-existing path not changed by this viewport-rendering work.
