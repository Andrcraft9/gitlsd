# Plan

## Done

- [x] MVP: log mode

## Planned: features

- [x] Preview split view
- [ ] Preview filter config to support delta
- [ ] Multi-line log entries: navigate entries not lines
- [x] Show mode: file explorer, diff per file
- [ ] Status mode (staged/unstaged changes)

## Planned: improvements
(L) - large; (M) - medium; (S) - small.

- [x] (L) Support text selection by mouse. User should be able to select (and copy from terminal) any text shown in views. Make output text based.
- [x] (L) Search: highlight search matches.
- [ ] (M) Commands: be able to call commands in any mode/view. Pressing `:` should work everywhere, not only in log.
- [ ] (S) Highlight view which is in focus.
- [x] (S) Adjust automatic vertical/horizontal tab split (used for preview).
- [x] (S) "q" key should work as "ESC" - the same. Pressing "ESC/q" when in log mode and if log is in focus should exit app.
- [ ] (S) Preview/show: add new keys to adjust context size, e.g. `[` and `]` defaults like in `tig`.
- [ ] (S) Don't print "Loaded commits N".
- [ ] (M) Simplify case_insensitive_match_ranges() and possibly other places assuming that we only support Latin alphabet.
- [ ] (S) Don't show "Focus the show diff to search". Always search through diff.
- [ ] (S) There is no returning back from search state. Once searched, you always see highlighted search results. ESC does't reset search.
- [ ] (S) In show mode: split file explorer and diff tabs with 1:2 ratio (diff should be wider).
