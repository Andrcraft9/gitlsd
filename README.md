# gitlsd

<p>
  <img src="logo.png" alt="gitlsd logo" width="120" align="left">
  <code>gitlsd</code> is a CLI wrapper around git for inspecting and interacting with logs, status, and diffs. It aims to be a modern, minimal, and focused alternative to <code>tig</code>.
</p>
<br clear="left">

## Controls

| Key | Action |
| --- | --- |
| `j`, Down | Select next commit |
| `k`, Up | Select previous commit |
| Page Down, Page Up | Move ten rows |
| Right, Left | Scroll the active log, preview, show, or help pane horizontally by half its width |
| Home, End | Jump to the horizontal beginning or end of the active pane |
| `/` | Enter a case-insensitive search and highlight all matches |
| `n`, `N` | Repeat search until `(END)` or `(TOP)` without wraparound |
| `:` | Enter a command |
| `?` | Show help and the effective configuration |
| Escape, `q` | Clear the active search, or back out of the focused pane; quit when the log has focus (Escape cancels text input) |
| Ctrl-C | Quit |
| `p` | Toggle selected-commit preview |
| `d` | Open selected-commit show mode |
| `s` | Open or close working-tree status mode |
| Tab | Switch between staged and unstaged status groups |
| `u` | Stage the selected unstaged file or unstage the selected staged file |
| Enter | Focus preview, show file, or status diff; Escape backs out one focus level |

Commands are `:help`/`:h` and `:quit`/`:q`. An empty or unknown command is reported in the status line.

## Configuration

gitlsd reads `$HOME/.config/gitlsd/config` when it exists. Use `--config PATH` to select another file.

Configuration is line-oriented. Whitespace separates arguments, quotes preserve whitespace, backslash escapes a character, and `#` starts a comment. `=` is optional.

```text
set log = git log --oneline --decorate --all --first-parent
set batch-size = 100
set preview = git show --stat --patch
set show-commit = git show --no-patch
set show = git show --patch --format=
set status-diff = git diff --color=always
set diff-filter = delta --paging never
bind j move-down
bind x quit
bind semicolon command
```

- `log` defines the one-line commit listing. It must start with `git log`; multiline, graph, stat, patch, notes, and signature output are rejected.
- `batch-size` sets pagination and must be greater than zero.
- `preview`, `show-commit`, and `show` customize commit views.
- `status-diff` customizes status diff presentation. Status discovery and staging commands remain fixed.
- `diff-filter` optionally filters the full preview, show diff, and staged/unstaged diffs, including untracked files. It is disabled by default; `set diff-filter =` disables it explicitly.
- `bind` maps a character or named key to an action. Enter and Backspace are reserved for text input.

The example filter requires `delta` on PATH; delta is only needed when configured. Filters receive Git bytes on stdin and supply displayed text on stdout, using the same argument quoting as other commands. A launch failure or unsuccessful exit reports an error for the affected view without fallback. Empty diffs skip the filter. Safe ANSI colors are preserved; other terminal escape sequences are removed, and debug output remains plain text. File jumps recognize Git patch headers and delta's standard file headers. Other custom filters must preserve `diff --git` headers for file jumps and selection synchronization. `--paging never` disables delta's pager.

Git and filter commands run directly without a shell. Git runs without an external pager. Press `?` to see all effective settings and bindings. Invalid configuration reports the file, line, and reason.

## Deterministic debug interface

Debug builds provide `--debug KEYS` for integration tests without a TTY. Separate keys with semicolons; literal characters and the same named keys used by the interactive frontend are accepted.

```console
cargo run -- --debug 'down;down;up'
cargo run -- --debug '/;r;e;l;e;a;s;e;enter;n'
```

The command prints a stable plain-text state snapshot. Use `semicolon` for the separator key itself. Invalid keys fail with a nonzero exit status. Release builds omit this option.

## Development

Project documentation:

- `CONTRIBUTING.md` — contribution and commit workflow.
- `ARCHITECTURE.md` — top-level architecture overview.
- `PLAN.md` — high-level completed and planned work.
- `AGENTS.md` — instructions for coding agents.
