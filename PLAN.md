# Plan

## Planned: features

- [ ] Multi-line log entries: navigate entries not lines
- [ ] Support .toml format for configuration file

Done:
- [x] Preview split view
- [x] Shared diff filter configuration for preview, show, and status (delta support)
- [x] Show mode: file explorer, diff per file
- [x] Status mode (staged/unstaged changes)
- [x] Configurable Editor: be able to open any file in Editor from gitlsd.
- [x] MVP: log mode

## Planned: improvements
(L) - large; (M) - medium; (S) - small.

- [x] (S) Don't print "Loaded commits N".
- [ ] (S) Preview/show: add new keys to adjust context size, e.g. `[` and `]` defaults like in `tig`.
- [ ] (S) Don't show "Focus the show diff to search". Always search through diff.
- [ ] (M) Simplify case_insensitive_match_ranges() and possibly other places assuming that we only support Latin alphabet.
- [ ] (L) Make diff filter not dependent on delta output, make it usable with non-delta. E.g. function like is_delta_decoration_rule(), delta_header_paths(), etc should be generic. Expand filter configuration to be able to easily achieve that (e.g. config for file tags, decorations, etc.).
- [ ] (M) Commands: be able to call commands in any mode/view. Pressing `:` should work everywhere, not only in log.

Done:
- [x] (L) Support text selection by mouse. User should be able to select (and copy from terminal) any text shown in views. Make output text based.
- [x] (L) Search: highlight search matches.
- [x] (S) Highlight view which is in focus.
- [x] (S) Adjust automatic vertical/horizontal tab split (used for preview).
- [x] (S) There is no returning back from search state. Once searched, you always see highlighted search results. ESC does't reset search.
- [x] (S) "q" key should work as "ESC" - the same. Pressing "ESC/q" when in log mode and if log is in focus should exit app.
- [-] (S) In show mode: split file explorer and diff tabs with 1:2 ratio (diff should be wider).
