//! Runtime policy and frontend-independent input vocabulary.
//!
//! This module owns defaults, configuration discovery and parsing, effective
//! settings, and the mapping from physical [`Key`] values to domain-grouped
//! application [`Action`] values. The grouped runtime vocabulary retains the
//! same user-facing action names for configuration and effective help.

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Action {
    Navigation(NavigationAction),
    Search(SearchAction),
    Global(GlobalAction),
    Preview(PreviewAction),
    Show(ShowAction),
    Status(StatusAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum NavigationAction {
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    ScrollStart,
    ScrollEnd,
    ScrollRight,
    ScrollLeft,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SearchAction {
    Start,
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum GlobalAction {
    Command,
    Help,
    Back,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PreviewAction {
    Toggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ShowAction {
    Open,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum StatusAction {
    Open,
    SwitchGroup,
    ToggleStage,
}

impl Action {
    pub const ALL: [Self; 20] = [
        Self::Navigation(NavigationAction::MoveDown),
        Self::Navigation(NavigationAction::MoveUp),
        Self::Navigation(NavigationAction::PageDown),
        Self::Navigation(NavigationAction::PageUp),
        Self::Navigation(NavigationAction::ScrollStart),
        Self::Navigation(NavigationAction::ScrollEnd),
        Self::Navigation(NavigationAction::ScrollRight),
        Self::Navigation(NavigationAction::ScrollLeft),
        Self::Search(SearchAction::Start),
        Self::Search(SearchAction::Next),
        Self::Search(SearchAction::Previous),
        Self::Global(GlobalAction::Command),
        Self::Global(GlobalAction::Help),
        Self::Global(GlobalAction::Back),
        Self::Global(GlobalAction::Quit),
        Self::Preview(PreviewAction::Toggle),
        Self::Show(ShowAction::Open),
        Self::Status(StatusAction::Open),
        Self::Status(StatusAction::SwitchGroup),
        Self::Status(StatusAction::ToggleStage),
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Navigation(action) => action.name(),
            Self::Search(action) => action.name(),
            Self::Global(action) => action.name(),
            Self::Preview(action) => action.name(),
            Self::Show(action) => action.name(),
            Self::Status(action) => action.name(),
        }
    }
}

impl NavigationAction {
    const fn name(self) -> &'static str {
        match self {
            Self::MoveDown => "move-down",
            Self::MoveUp => "move-up",
            Self::PageDown => "page-down",
            Self::PageUp => "page-up",
            Self::ScrollStart => "scroll-start",
            Self::ScrollEnd => "scroll-end",
            Self::ScrollRight => "scroll-right",
            Self::ScrollLeft => "scroll-left",
        }
    }
}

impl SearchAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Start => "search",
            Self::Next => "search-next",
            Self::Previous => "search-previous",
        }
    }
}

impl GlobalAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Help => "help",
            Self::Back => "back",
            Self::Quit => "quit",
        }
    }
}

impl PreviewAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Toggle => "toggle-preview",
        }
    }
}

impl ShowAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Open => "show-mode",
        }
    }
}

impl StatusAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Open => "status-mode",
            Self::SwitchGroup => "status-switch-group",
            Self::ToggleStage => "status-toggle-stage",
        }
    }
}

impl FromStr for Action {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|action| action.name() == value)
            .ok_or_else(|| format!("unknown action `{value}`"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Key {
    Char(char),
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Escape,
    Enter,
    Backspace,
    Tab,
    Ctrl(char),
}

impl Key {
    pub fn display(&self) -> String {
        match self {
            Self::Char(' ') => "space".into(),
            Self::Char(';') => "semicolon".into(),
            Self::Char(value) => value.to_string(),
            Self::Up => "up".into(),
            Self::Down => "down".into(),
            Self::PageUp => "page-up".into(),
            Self::PageDown => "page-down".into(),
            Self::Home => "home".into(),
            Self::End => "end".into(),
            Self::Left => "left".into(),
            Self::Right => "right".into(),
            Self::Escape => "esc".into(),
            Self::Enter => "enter".into(),
            Self::Backspace => "backspace".into(),
            Self::Tab => "tab".into(),
            Self::Ctrl(value) => format!("ctrl-{value}"),
        }
    }
}

impl FromStr for Key {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "page-up" | "pgup" => Ok(Self::PageUp),
            "page-down" | "pgdn" => Ok(Self::PageDown),
            "home" => Ok(Self::Home),
            "end" => Ok(Self::End),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "esc" | "escape" => Ok(Self::Escape),
            "enter" | "return" => Ok(Self::Enter),
            "backspace" | "bs" => Ok(Self::Backspace),
            "tab" => Ok(Self::Tab),
            "space" => Ok(Self::Char(' ')),
            "semicolon" => Ok(Self::Char(';')),
            _ if value.starts_with("ctrl-") && value.chars().count() == 6 => {
                Ok(Self::Ctrl(value.chars().nth(5).expect("validated length")))
            }
            _ if value.chars().count() == 1 => {
                Ok(Self::Char(value.chars().next().expect("validated length")))
            }
            _ => Err(format!("unknown key `{value}`")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub log_command: Vec<String>,
    pub preview_command: Vec<String>,
    pub show_commit_command: Vec<String>,
    pub show_command: Vec<String>,
    pub status_diff_command: Vec<String>,
    pub diff_filter: Option<Vec<String>>,
    pub batch_size: usize,
    pub bindings: BTreeMap<Key, Action>,
    pub source: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        let bindings = [
            (
                Key::Char('j'),
                Action::Navigation(NavigationAction::MoveDown),
            ),
            (Key::Down, Action::Navigation(NavigationAction::MoveDown)),
            (Key::Char('k'), Action::Navigation(NavigationAction::MoveUp)),
            (Key::Up, Action::Navigation(NavigationAction::MoveUp)),
            (
                Key::PageDown,
                Action::Navigation(NavigationAction::PageDown),
            ),
            (Key::PageUp, Action::Navigation(NavigationAction::PageUp)),
            (Key::Home, Action::Navigation(NavigationAction::ScrollStart)),
            (Key::End, Action::Navigation(NavigationAction::ScrollEnd)),
            (
                Key::Right,
                Action::Navigation(NavigationAction::ScrollRight),
            ),
            (Key::Left, Action::Navigation(NavigationAction::ScrollLeft)),
            (Key::Char('/'), Action::Search(SearchAction::Start)),
            (Key::Char('n'), Action::Search(SearchAction::Next)),
            (Key::Char('N'), Action::Search(SearchAction::Previous)),
            (Key::Char(':'), Action::Global(GlobalAction::Command)),
            (Key::Char('?'), Action::Global(GlobalAction::Help)),
            (Key::Escape, Action::Global(GlobalAction::Back)),
            (Key::Char('q'), Action::Global(GlobalAction::Back)),
            (Key::Ctrl('c'), Action::Global(GlobalAction::Quit)),
            (Key::Char('p'), Action::Preview(PreviewAction::Toggle)),
            (Key::Char('d'), Action::Show(ShowAction::Open)),
            (Key::Char('s'), Action::Status(StatusAction::Open)),
            (Key::Tab, Action::Status(StatusAction::SwitchGroup)),
            (Key::Char('u'), Action::Status(StatusAction::ToggleStage)),
        ]
        .into_iter()
        .collect();

        Self {
            log_command: vec![
                "git".into(),
                "log".into(),
                "--oneline".into(),
                "--decorate".into(),
                "--color=always".into(),
            ],
            preview_command: vec![
                "git".into(),
                "show".into(),
                "--stat".into(),
                "--patch".into(),
                "--color=always".into(),
            ],
            show_commit_command: vec![
                "git".into(),
                "show".into(),
                "--no-patch".into(),
                "--color=always".into(),
            ],
            show_command: vec![
                "git".into(),
                "show".into(),
                "--patch".into(),
                "--format=".into(),
                "--color=always".into(),
            ],
            status_diff_command: vec!["git".into(), "diff".into(), "--color=always".into()],
            diff_filter: None,
            batch_size: 100,
            bindings,
            source: None,
        }
    }
}

#[derive(Debug)]
pub struct ConfigError {
    location: String,
    reason: String,
}

impl ConfigError {
    fn new(location: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.location, self.reason)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn load(explicit: Option<&Path>) -> Result<Self, ConfigError> {
        let path = match explicit {
            Some(path) => Some(path.to_path_buf()),
            None => default_path(),
        };
        let Some(path) = path else {
            return Ok(Self::default());
        };
        Self::load_path(&path, explicit.is_none())
    }

    fn load_path(path: &Path, missing_is_default: bool) -> Result<Self, ConfigError> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if missing_is_default && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(ConfigError::new(
                    path.display().to_string(),
                    error.to_string(),
                ));
            }
        };
        Self::parse(&contents, path)
    }

    pub fn parse(contents: &str, source: &Path) -> Result<Self, ConfigError> {
        let mut config = Self {
            source: Some(source.to_path_buf()),
            ..Self::default()
        };
        for (index, line) in contents.lines().enumerate() {
            let location = format!("{}:{}", source.display(), index + 1);
            let tokens = tokenize(line).map_err(|reason| ConfigError::new(&location, reason))?;
            if tokens.is_empty() {
                continue;
            }
            match tokens[0].as_str() {
                "set" => config.parse_set(&tokens[1..], &location)?,
                "bind" => config.parse_bind(&tokens[1..], &location)?,
                directive => {
                    return Err(ConfigError::new(
                        location,
                        format!("unknown directive `{directive}`; expected `set` or `bind`"),
                    ));
                }
            }
        }
        Ok(config)
    }

    fn parse_set(&mut self, input: &[String], location: &str) -> Result<(), ConfigError> {
        let input = without_optional_separator(input);
        let Some((name, values)) = input.split_first() else {
            return Err(ConfigError::new(location, "`set` requires a setting name"));
        };
        match name.as_str() {
            "log" => {
                if values.get(..2) != Some(&["git".to_owned(), "log".to_owned()]) {
                    return Err(ConfigError::new(
                        location,
                        "required form: `set log git log <options>`",
                    ));
                }
                validate_log(values).map_err(|reason| ConfigError::new(location, reason))?;
                self.log_command = values.to_vec();
            }
            "batch-size" => {
                if values.len() != 1 {
                    return Err(ConfigError::new(
                        location,
                        "`set batch-size` requires one positive integer",
                    ));
                }
                self.batch_size = values[0].parse().map_err(|_| {
                    ConfigError::new(location, "batch size must be a positive integer")
                })?;
                if self.batch_size == 0 {
                    return Err(ConfigError::new(
                        location,
                        "batch size must be greater than zero",
                    ));
                }
            }
            "preview" => {
                validate_show(values, "preview", location)?;
                self.preview_command = values.to_vec();
            }
            "show-commit" => {
                validate_show(values, "show-commit", location)?;
                self.show_commit_command = values.to_vec();
            }
            "show" => {
                validate_show(values, "show", location)?;
                self.show_command = values.to_vec();
            }
            "diff-filter" => {
                if values.first().is_some_and(|value| value.is_empty())
                    || values.iter().any(|value| value.contains('\0'))
                {
                    return Err(ConfigError::new(
                        location,
                        "diff-filter requires a nonempty executable and arguments without NUL bytes",
                    ));
                }
                self.diff_filter = (!values.is_empty()).then(|| values.to_vec());
            }
            "status-diff" => {
                validate_status_diff(values, location)?;
                self.status_diff_command = values.to_vec();
            }
            _ => {
                return Err(ConfigError::new(
                    location,
                    format!("unknown setting `{name}`"),
                ));
            }
        }
        Ok(())
    }

    fn parse_bind(&mut self, input: &[String], location: &str) -> Result<(), ConfigError> {
        let input = without_optional_separator(input);
        if input.len() != 2 {
            return Err(ConfigError::new(
                location,
                "`bind` requires exactly a key and an action",
            ));
        }
        let key: Key = input[0]
            .parse()
            .map_err(|reason: String| ConfigError::new(location, reason))?;
        if matches!(key, Key::Enter | Key::Backspace) {
            return Err(ConfigError::new(
                location,
                "enter and backspace are reserved for text entry",
            ));
        }
        let action = input[1]
            .parse()
            .map_err(|reason: String| ConfigError::new(location, reason))?;
        self.bindings.insert(key, action);
        Ok(())
    }

    pub fn action_for(&self, key: &Key) -> Option<Action> {
        self.bindings.get(key).copied()
    }

    pub fn help_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "setting.log={}",
                self.log_command
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!("setting.batch-size={}", self.batch_size),
            format!(
                "setting.diff-filter={}",
                self.diff_filter
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!(
                "setting.preview={}",
                self.preview_command
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!(
                "setting.show-commit={}",
                self.show_commit_command
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!(
                "setting.show={}",
                self.show_command
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!(
                "setting.status-diff={}",
                self.status_diff_command
                    .iter()
                    .map(|argument| quote_argument(argument))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            format!(
                "setting.config={}",
                self.source
                    .as_ref()
                    .map_or_else(|| "defaults".into(), |path| path.display().to_string())
            ),
        ];
        lines.extend(
            self.bindings
                .iter()
                .map(|(key, action)| format!("binding.{}={}", key.display(), action.name())),
        );
        lines
    }
}

fn validate_show(arguments: &[String], setting: &str, location: &str) -> Result<(), ConfigError> {
    if arguments.get(..2) != Some(&["git".to_owned(), "show".to_owned()]) {
        return Err(ConfigError::new(
            location,
            format!("required form: `set {setting} git show <options>`"),
        ));
    }
    Ok(())
}

fn validate_status_diff(arguments: &[String], location: &str) -> Result<(), ConfigError> {
    if arguments.get(..2) != Some(&["git".to_owned(), "diff".to_owned()]) {
        return Err(ConfigError::new(
            location,
            "required form: `set status-diff git diff <options>`",
        ));
    }
    Ok(())
}

fn validate_log(arguments: &[String]) -> Result<(), String> {
    let mut one_line = false;
    let mut arguments = arguments[2..].iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        let name = argument.split('=').next().unwrap_or(argument);
        if !argument.contains('=')
            && matches!(
                name,
                "--grep"
                    | "--author"
                    | "--committer"
                    | "--since"
                    | "--until"
                    | "--after"
                    | "--before"
                    | "--date"
                    | "--encoding"
                    | "--max-count"
                    | "--skip"
                    | "-n"
                    | "-S"
                    | "-G"
            )
        {
            arguments.next();
            continue;
        }
        if matches!(
            name,
            "--graph"
                | "-p"
                | "-u"
                | "--patch"
                | "--raw"
                | "--stat"
                | "--numstat"
                | "--shortstat"
                | "--summary"
                | "--name-only"
                | "--name-status"
                | "--patch-with-raw"
                | "--patch-with-stat"
                | "--full-diff"
                | "--binary"
                | "--check"
                | "--show-signature"
                | "--notes"
                | "--show-notes"
                | "--log-size"
                | "-c"
                | "--cc"
                | "-z"
                | "--null"
                | "--walk-reflogs"
        ) || name.starts_with("--dirstat")
            || name.starts_with("-U")
            || name.starts_with("-L")
            || name.starts_with("-p") && !name.starts_with("--")
            || name == "-g"
        {
            return Err(format!(
                "incompatible option `{argument}`; use a one-line `git log --oneline` or `--format` command"
            ));
        }
        if argument == "--oneline" {
            one_line = true;
        }
        if matches!(name, "--format" | "--pretty") {
            let format = if let Some((_, value)) = argument.split_once('=') {
                value
            } else {
                arguments.next().map(String::as_str).unwrap_or("")
            };
            let format = format
                .strip_prefix("format:")
                .or_else(|| format.strip_prefix("tformat:"))
                .unwrap_or(format);
            let checked = format
                .replace("%%", "")
                .replace("% ", "%")
                .replace("%-", "%");
            let lower = checked.to_lowercase();
            if matches!(
                format,
                "short" | "medium" | "full" | "fuller" | "email" | "mboxrd" | "raw"
            ) || ["%n", "%b", "%B", "%N", "%GG", "%+", "%w("]
                .iter()
                .any(|token| checked.contains(token))
                || lower.contains("%x0a")
                || lower.contains("%x0d")
                || lower.contains("%x00")
                || format.contains(['\n', '\r'])
            {
                return Err(format!(
                    "incompatible format `{format}`; use `--oneline` or a one-line format such as `--format=%h %s`"
                ));
            }
            one_line = true;
        }
    }
    if !one_line {
        return Err("multiline default format is incompatible; add `--oneline` or a one-line `--format` to `set log git log <options>`".into());
    }
    Ok(())
}

fn without_optional_separator(input: &[String]) -> Vec<String> {
    let mut output = input.to_vec();
    if output.get(1).map(String::as_str) == Some("=") {
        output.remove(1);
    }
    output
}

fn default_path() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".config/gitlsd/config"))
}

fn quote_argument(argument: &str) -> String {
    format!(
        "\"{}\"",
        argument.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut token_started = false;
    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            token_started = true;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            token_started = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                token_started = true;
            }
            '#' => break,
            value if value.is_whitespace() => {
                if token_started {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            _ => {
                current.push(character);
                token_started = true;
            }
        }
    }
    if escaped {
        return Err("trailing escape".into());
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    if token_started {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    #[test]
    fn optional_diff_filter_parses_disables_and_round_trips() {
        let path = Path::new("filter.conf");
        assert!(Config::default().diff_filter.is_none());
        assert!(
            Config::default()
                .help_lines()
                .contains(&"setting.diff-filter=".into())
        );
        let config = Config::parse(
            r##"set diff-filter = "custom filter" "quoted argument" "" "#literal""##,
            path,
        )
        .unwrap();
        let arguments = config.diff_filter.as_ref().unwrap();
        assert_eq!(
            arguments,
            &["custom filter", "quoted argument", "", "#literal"]
        );
        let help = config
            .help_lines()
            .into_iter()
            .find(|line| line.starts_with("setting.diff-filter="))
            .unwrap();
        let value = help.strip_prefix("setting.diff-filter=").unwrap();
        assert_eq!(
            Config::parse(&format!("set diff-filter = {value}"), path)
                .unwrap()
                .diff_filter,
            config.diff_filter
        );
        assert!(
            Config::parse("set diff-filter = cat\nset diff-filter =", path)
                .unwrap()
                .diff_filter
                .is_none()
        );
        for malformed in [
            "set diff-filter = ''",
            "set diff-filter = 'unterminated",
            "set diff-filter = cat \0",
        ] {
            let error = Config::parse(malformed, path).unwrap_err().to_string();
            assert!(error.starts_with("filter.conf:1:"), "{error}");
        }
    }
    use super::*;

    #[test]
    fn parses_settings_bindings_quotes_and_comments() {
        let config = Config::parse(
            "set log = git log --oneline --all --author 'Grace Hopper' '' = # comment\nset batch-size 7\nbind x move-down\nbind = quit\nbind semicolon = help",
            Path::new("sample.conf"),
        )
        .unwrap();

        assert_eq!(
            config.log_command,
            [
                "git",
                "log",
                "--oneline",
                "--all",
                "--author",
                "Grace Hopper",
                "",
                "="
            ]
        );
        assert_eq!(config.batch_size, 7);
        assert_eq!(
            config.action_for(&Key::Char('x')),
            Some(Action::Navigation(NavigationAction::MoveDown))
        );
        assert_eq!(
            config.action_for(&Key::Char('=')),
            Some(Action::Global(GlobalAction::Quit))
        );
        assert_eq!(
            config.action_for(&Key::Char(';')),
            Some(Action::Global(GlobalAction::Help))
        );
        assert_eq!(
            config.help_lines()[0],
            "setting.log=\"git\" \"log\" \"--oneline\" \"--all\" \"--author\" \"Grace Hopper\" \"\" \"=\""
        );
    }

    #[test]
    fn parses_show_commands_and_action_with_reversible_help() {
        let config = Config::parse(
            "set show-commit = git show --format='%H' --no-patch\nset show git show --patch --format=\nbind x show-mode\n",
            Path::new("sample.conf"),
        )
        .unwrap();

        assert_eq!(
            config.show_commit_command,
            ["git", "show", "--format=%H", "--no-patch"]
        );
        assert_eq!(config.show_command, ["git", "show", "--patch", "--format="]);
        assert_eq!(
            config.action_for(&Key::Char('d')),
            Some(Action::Show(ShowAction::Open))
        );
        assert_eq!(
            config.action_for(&Key::Char('x')),
            Some(Action::Show(ShowAction::Open))
        );
        assert!(config.help_lines().contains(
            &"setting.show-commit=\"git\" \"show\" \"--format=%H\" \"--no-patch\"".into()
        ));
        assert!(
            config
                .help_lines()
                .contains(&"setting.show=\"git\" \"show\" \"--patch\" \"--format=\"".into())
        );
    }

    #[test]
    fn parses_status_diff_and_status_actions() {
        let config = Config::parse(
            "set status-diff = git diff --stat\nbind tab status-switch-group\nbind x status-toggle-stage\n",
            Path::new("sample.conf"),
        )
        .unwrap();
        assert_eq!(config.status_diff_command, ["git", "diff", "--stat"]);
        assert_eq!(
            config.action_for(&Key::Tab),
            Some(Action::Status(StatusAction::SwitchGroup))
        );
        assert_eq!(
            config.action_for(&Key::Char('x')),
            Some(Action::Status(StatusAction::ToggleStage))
        );
        assert!(
            config
                .help_lines()
                .contains(&"setting.status-diff=\"git\" \"diff\" \"--stat\"".into())
        );
    }

    #[test]
    fn horizontal_bindings_are_available_by_default_and_configurable() {
        let config = Config::parse(
            "bind x scroll-right\nbind y scroll-left\n",
            Path::new("sample.conf"),
        )
        .unwrap();
        assert_eq!(
            config.action_for(&Key::Right),
            Some(Action::Navigation(NavigationAction::ScrollRight))
        );
        assert_eq!(
            config.action_for(&Key::Left),
            Some(Action::Navigation(NavigationAction::ScrollLeft))
        );
        assert_eq!(
            config.action_for(&Key::Home),
            Some(Action::Navigation(NavigationAction::ScrollStart))
        );
        assert_eq!(
            config.action_for(&Key::End),
            Some(Action::Navigation(NavigationAction::ScrollEnd))
        );
        assert_eq!(
            config.action_for(&Key::Char('x')),
            Some(Action::Navigation(NavigationAction::ScrollRight))
        );
        assert_eq!(
            config.action_for(&Key::Char('y')),
            Some(Action::Navigation(NavigationAction::ScrollLeft))
        );
        assert!(
            config
                .help_lines()
                .contains(&"binding.x=scroll-right".into())
        );
        assert!(
            config
                .help_lines()
                .contains(&"binding.right=scroll-right".into())
        );
    }

    #[test]
    fn grouped_actions_keep_the_configured_action_names() {
        let names = [
            "move-down",
            "move-up",
            "page-down",
            "page-up",
            "scroll-start",
            "scroll-end",
            "scroll-right",
            "scroll-left",
            "search",
            "search-next",
            "search-previous",
            "command",
            "help",
            "back",
            "quit",
            "toggle-preview",
            "show-mode",
            "status-mode",
            "status-switch-group",
            "status-toggle-stage",
        ];

        assert_eq!(Action::ALL.len(), names.len());
        for (action, name) in Action::ALL.into_iter().zip(names) {
            assert_eq!(action.name(), name);
            assert_eq!(name.parse::<Action>(), Ok(action));
        }
    }

    #[test]
    fn non_file_default_path_is_reported_instead_of_treated_as_missing() {
        let directory = std::env::temp_dir();
        let error = Config::load_path(&directory, true).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with(&directory.display().to_string())
        );
    }

    #[test]
    fn invalid_directive_reports_file_and_line() {
        let error = Config::parse("set batch-size 0", Path::new("bad.conf")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "bad.conf:1: batch size must be greater than zero"
        );
    }

    #[test]
    fn invalid_preview_command_reports_file_and_line() {
        let error =
            Config::parse("set preview git log --oneline", Path::new("bad.conf")).unwrap_err();
        assert_eq!(
            error.to_string(),
            "bad.conf:1: required form: `set preview git show <options>`"
        );
    }

    #[test]
    fn invalid_show_commands_report_file_and_line() {
        for (setting, message) in [
            (
                "show-commit",
                "required form: `set show-commit git show <options>`",
            ),
            ("show", "required form: `set show git show <options>`"),
        ] {
            let error = Config::parse(
                &format!("set {setting} git log --oneline"),
                Path::new("bad.conf"),
            )
            .unwrap_err();
            assert_eq!(error.to_string(), format!("bad.conf:1: {message}"));
        }
    }
}
