# gitlsd

`gitlsd` is a CLI wrapper around git for inspecting and interacting with logs, status, and diffs. It aims to be a modern, minimal, and focused alternative to `tig`.

## Controls

| Key | Action |
| --- | --- |
| `j`, Down | Select next commit |
| `k`, Up | Select previous commit |
| Page Down, Page Up | Move ten rows |
| `/` | Enter a case-insensitive search |
| `n`, `N` | Repeat search forward or backward, wrapping at the ends |
| `:` | Enter a command |
| `?` | Show help and the effective configuration |
| Escape | Return to the log or cancel input |
| `q`, Ctrl-C | Quit |

Commands are `:help`/`:h` and `:quit`/`:q`. An empty or unknown command is reported in the status line.

## Configuration

By default, gitlsd reads `$XDG_CONFIG_HOME/gitlsd/config` when `XDG_CONFIG_HOME` is a non-empty absolute path. Otherwise it falls back to `$HOME/.config/gitlsd/config`. A missing default file is fine; other filesystem errors are reported. Select another file with `--config PATH`; an explicitly selected file must exist and be valid.

Configuration is line-oriented. Whitespace separates arguments, single and double quotes preserve whitespace, backslash escapes a character, and `#` starts a comment outside quotes. `=` is optional:

```text
set log = log --all --first-parent
set batch-size = 100
bind j move-down
bind x quit
bind semicolon command
```

`set log` supplies argv following the `git` executable and must begin with the `log` subcommand. Empty quoted arguments and literal `=` arguments are preserved. gitlsd appends machine-output and pagination arguments. The command is executed directly, never through a shell, with Git paging and color disabled. Batch size must be greater than zero.

Keys are a single character or one of `up`, `down`, `page-up`, `page-down`, `esc`, `space`, `semicolon`, and `ctrl-<character>`. Available actions are `move-down`, `move-up`, `page-down`, `page-up`, `search`, `search-next`, `search-previous`, `command`, `help`, `back`, and `quit`. Enter and Backspace are reserved for text entry. Help displays all effective settings and bindings, including defaults. It quotes every log argv element reversibly so empty and whitespace-containing arguments remain distinguishable.

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
