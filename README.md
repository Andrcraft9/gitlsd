# gitlsd

`gitlsd` is a CLI wrapper around git for inspecting and interacting with logs, status, and diffs. It aims to be a modern, minimal, and focused alternative to `tig`.

## Controls

| Key | Action |
| --- | --- |
| `j`, Down | Select next commit |
| `k`, Up | Select previous commit |
| Page Down, Page Up | Move ten rows |
| Right, Left | Scroll the active log, preview, or help pane horizontally |
| Home, End | Jump to the horizontal beginning or end of the active pane |
| `/` | Enter a case-insensitive search and highlight all matches |
| `n`, `N` | Repeat search until `(END)` or `(TOP)` without wraparound |
| `:` | Enter a command |
| `?` | Show help and the effective configuration |
| Escape | Return to the log or cancel input |
| `q`, Ctrl-C | Quit |
| `p` | Toggle selected-commit preview |
| Enter | Focus preview; Escape returns to log |

Commands are `:help`/`:h` and `:quit`/`:q`. An empty or unknown command is reported in the status line.

## Configuration

By default, gitlsd reads `$HOME/.config/gitlsd/config`. A missing default file is fine; other filesystem errors are reported. Select another file with `--config PATH`; an explicitly selected file must exist and be valid.

Configuration is line-oriented. Whitespace separates arguments, single and double quotes preserve whitespace, backslash escapes a character, and `#` starts a comment outside quotes. `=` is optional:

```text
set log = git log --oneline --decorate --all --first-parent
set batch-size = 100
set preview = git show --stat --patch
bind j move-down
bind x quit
bind semicolon command
```

`set log` supplies the full command and must begin with `git log`. The default is `git log --oneline --decorate`. Empty quoted arguments and literal `=` arguments are preserved. Git owns the displayed text: for example, `set log git log --format='%h %an: %s'` selects a custom one-line presentation. gitlsd adds pagination before any `-- <path>` arguments and disables external paging. Commands execute directly without a shell. Batch size must be greater than zero.

Each row must represent one commit. Multiline formats and graph, stat, patch, notes, and signature output are rejected with a configuration error; use `--oneline` or a one-line `--format`. A companion query retrieves stable commit IDs using the same history selection and pagination. Search matches displayed text. Git ANSI SGR colors are rendered interactively and stripped from deterministic debug output; other terminal controls are neutralized.

Keys are a single character or one of `up`, `down`, `left`, `right`, `home`, `end`, `page-up`, `page-down`, `esc`, `space`, `semicolon`, and `ctrl-<character>`. Available actions are `move-down`, `move-up`, `page-down`, `page-up`, `scroll-start`, `scroll-end`, `scroll-right`, `scroll-left`, `search`, `search-next`, `search-previous`, `command`, `help`, `back`, `quit`, and `toggle-preview`. Enter and Backspace are reserved for text entry. Help displays all effective settings and bindings, including defaults. It quotes every log argv element reversibly so empty and whitespace-containing arguments remain distinguishable.

The interactive frontend leaves mouse input to the terminal. Drag-select any currently rendered log, preview, help, status, or input text and use the terminal's ordinary copy operation. Mouse-wheel and touchpad scrolling are likewise terminal-owned; because gitlsd uses the alternate screen, that may not provide normal scrollback. Use the configured keyboard controls to scroll gitlsd panes.

The selected commit preview is visible by default. It runs `git show --stat --patch` with the selected full commit ID appended; `set preview git show ...` customizes that command. Enter focuses the preview, where navigation and search act only on preview text.
The preview is placed beside the log only when the terminal is sufficiently wide; otherwise it is stacked below the log. The automatic choice accounts for the rectangular shape of terminal cells.

Invalid configuration reports its file, line, and reason.

## Deterministic debug interface

Debug builds expose `--debug KEYS` for integration testing without a TTY. `KEYS` is a semicolon-separated sequence of the same key names used by the interactive frontend; literal characters are individual keys. Use the named key `semicolon` to represent the separator character itself. For example:

```console
cargo run -- --debug 'down;down;up'
cargo run -- --debug '/;r;e;l;e;a;s;e;enter;n'
cargo run -- --debug ':;h;e;l;p;enter'
```

The command prints a stable plain-text snapshot including screen, selection, loaded rows, selected commit, status, and either log rows or effective help. Invalid scripted keys return a nonzero exit status. This option is compiled out of release builds and is not shown by `target/release/gitlsd --help`.

## Development

Project documentation:

- `CONTRIBUTING.md` — contribution and commit workflow.
- `ARCHITECTURE.md` — top-level architecture overview.
- `PLAN.md` — high-level completed and planned work.
- `AGENTS.md` — instructions for coding agents.
