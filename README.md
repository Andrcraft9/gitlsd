# gitlsd

<p>
  <img src="logo.png" alt="gitlsd logo" width="120" align="left">
  <code>gitlsd</code> is a CLI wrapper around git for inspecting and interacting with logs, status, and diffs. It aims to be a modern, minimal, and focused alternative to <code>tig</code>.
</p>
<br clear="left">

## Usage

Run `gitlsd` to browse the current history, or pass a branch to browse commits
reachable from that branch:

```console
gitlsd
gitlsd feature/topic
```

## Controls

| Key | Action |
| --- | --- |
| `j`, Down | Navigate down |
| `k`, Up | Navigate up |
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
| `u` | Stage or unstage file |
| `e` | Open the selected show or status file in the editor |
| Enter | Open selection; expand a focused diff full screen |

Commands: `:help` (`:h`) and `:quit` (`:q`).

## Configuration

gitlsd reads `$HOME/.config/gitlsd/config` when it exists. Use `--config PATH` to select another file.
Use `--create-config` to write a configuration containing every default setting and
binding at the default location. If that file already exists, gitlsd leaves it
unchanged.

Configuration is line-oriented. Whitespace separates arguments, quotes preserve whitespace, backslash escapes a character, and `#` starts a comment. `=` is optional.

```text
set log = git log --oneline --decorate --all --first-parent
set batch-size = 100
set preview = git show --stat --patch
set show-commit = git show --no-patch
set show = git show --patch --format=
set status-diff = git diff --color=always
set diff-filter = delta --paging never --line-numbers
set editor = micro +line file
bind j move-down
bind x quit
bind semicolon command
```

- `log` defines the one-line commit listing. It must start with `git log`; multiline, graph, stat, patch, notes, and signature output are rejected.
- `batch-size` sets pagination and must be greater than zero.
- `preview`, `show-commit`, and `show` customize commit views.
- `status-diff` customizes status diff presentation. Status discovery and staging commands remain fixed.
- `diff-filter` optionally filters the full preview, show diff, and staged/unstaged diffs, including untracked files. It is disabled by default; `set diff-filter =` disables it explicitly.
- `editor` defines the editor command and must include both file and line placeholders. Standalone `file` and `line`, micro's `+line`, VS Code's `file:line`, and embedded `{file}`/`{line}` forms are supported. It defaults to `micro +line file`. To use VS Code, set `editor = code -g --goto file:line`.
- `bind` maps a character or named key to an action. Enter and Backspace are reserved for text input.

The `open-editor` action works in the show and status file explorers and diff panes. Diff panes open at the worktree line represented by the top visible diff row; rows without a source-line mapping and explorer selections open at line 1. Deleted files cannot be opened because they are absent from the worktree.

The example filter requires `delta` on PATH; delta is only needed when configured. Filters receive Git bytes on stdin and supply displayed text on stdout, using the same argument quoting as other commands. A launch failure or unsuccessful exit reports an error for the affected view without fallback. Empty diffs skip the filter. Safe ANSI colors are preserved; other terminal escape sequences are removed, and debug output remains plain text. File jumps recognize Git patch headers and delta's standard file headers. Delta's `--line-numbers` output also preserves editor line jumps from diff panes. Other custom filters must preserve `diff --git` headers for file jumps and selection synchronization. `--paging never` disables delta's pager.

Git, filter, and editor commands run directly without a shell. Git runs without an external pager. Press `?` to see all effective settings and bindings. Invalid configuration reports the file, line, and reason.

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
