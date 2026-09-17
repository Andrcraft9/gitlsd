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
| `u` | Stage or unstage file in status mode |
| `r` | Revert selected file in status mode (unstaged from index, staged from HEAD; remove untracked files) |
| `e` | Open the selected show or status file in the editor |
| Enter | Open selection; expand a focused diff full screen |

The focused view is indicated by a cyan border. Chunk navigation jumps to visible
hunk headers across files without wrapping, including in fullscreen diffs. Status
navigation stays within the active staged or unstaged group. With delta, omitted
or unrecognizable hunk headers provide no chunk targets.

Commands: `:help` (`:h`), `:goto <commit>` (`:gt <commit>`) in log mode, and `:quit` (`:q`). `:goto` accepts a full commit ID or commit ID prefix and loads additional history batches as needed.

## Configuration

gitlsd reads `$HOME/.config/gitlsd/config`. Use `--config PATH` to load another file, or `--create-config` to create a default one.

Each line sets an option or binds a key. `#` starts a comment, quotes preserve spaces, and `=` is optional.

```text
set log = git log --oneline --decorate --all --first-parent
set batch-size = 100
set editor = micro +line file
bind shift-down next-chunk
bind shift-up previous-chunk
bind } next-file
bind { previous-file
bind j move-down
bind x quit
```

- `log` controls the commit list and must use a one-line `git log` format.
- `batch-size` controls how many commits are loaded at a time.
- `preview`, `show-commit`, `show`, and `status-diff` control Git output.
- `diff-filter` optionally pipes displayed diffs through another command.
- `editor` sets the editor command and must include `file` and `line`. The default is `micro +line file`; for VS Code, use `code -g --goto file:line`.
- `bind` maps a character or named key to an action.

Press `?` to see the active settings and bindings. Configuration errors include the file and line number.

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
