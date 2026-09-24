# gitlsd

<p>
  <img src="logo.png" alt="gitlsd logo" width="120" align="left">
  <code>gitlsd</code> is a CLI wrapper around git for inspecting and interacting with logs, status, and diffs. It aims to be a modern, minimal, and focused alternative to <code>tig</code>.
</p>
<br clear="left">

## Usage

Run `gitlsd` to browse the current history, or pass a branch to browse commits reachable from that branch:

```console
gitlsd
gitlsd feature/topic
```

## Controls

| Key | Action |
| --- | --- |
| `j`, Down | Navigate down |
| `k`, Up | Navigate up |
| Shift+Down, Shift+Up | Next or previous chunk in the focused show or status diff |
| `}`, `{` | Next or previous file in the focused show or status diff |
| Page Down, Page Up | Move down or up ten rows |
| Right, Left | Scroll horizontally |
| Home, End | Jump to the start or end |
| `/` | Search |
| `n`, `N` | Next or previous match |
| `:` | Enter a command |
| `?` | Show help |
| Escape, `q` | Go back or quit |
| Ctrl-C | Quit |
| `p` | Toggle preview |
| `d` | Show selected commit |
| `s` | Toggle status |
| Tab | Switch status group |
| `u` | Stage or unstage selected chunk (diff focus) or file (explorer focus) in status mode |
| `r` | Refresh the current view (log and show reset selection and scroll position) |
| `R` | Revert selected chunk (diff focus) or file (explorer focus) in status mode (unstaged from index, staged from HEAD; remove untracked files) |
| `e` | Open the selected show or status file in the editor |
| Enter | Open selection; expand a focused diff full screen |

Commands: `:help` (`:h`), `:goto <commit>` (`:gt <commit>`) in log mode, and `:quit` (`:q`). `:goto` accepts a full commit ID or commit ID prefix and loads additional history batches as needed.

## Configuration

gitlsd reads `$HOME/.config/gitlsd/config`; `--config PATH` selects another file.
Use `--create-config` for the defaults or `--create-config-delta` for delta and
VS Code. Existing files are never overwritten.

Lines use `set NAME VALUE` or `bind KEY ACTION`. `=` is optional, `#` starts a
comment, and quotes preserve spaces.

```text
set log = git log --oneline --decorate --all --first-parent
set batch-size = 100
set editor = micro +line file
bind x quit
```

Settings cover view commands, batch size, split ratios, an optional diff filter,
and the editor. `log` must produce one line per commit; `editor` must include
`file` and `line`. Press `?` to inspect the active configuration.

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
