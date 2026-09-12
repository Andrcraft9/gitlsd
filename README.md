# gitlsd

<p>
  <img src="logo.png" alt="gitlsd logo" width="180" align="left">
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
| Escape, `q` | Back out of the focused pane; quit when the log has focus (Escape cancels text input) |
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
bind j move-down
bind x quit
bind semicolon command
```

- `log` defines the one-line commit listing. It must start with `git log`; multiline, graph, stat, patch, notes, and signature output are rejected.
- `batch-size` sets pagination and must be greater than zero.
- `preview`, `show-commit`, and `show` customize commit views.
- `status-diff` customizes status diff presentation. Status discovery and staging commands remain fixed.
- `bind` maps a character or named key to an action. Enter and Backspace are reserved for text input.

Git commands run directly without a shell or external pager. Press `?` to see all effective settings and bindings. Invalid configuration reports the file, line, and reason.

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
