use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Action {
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    Search,
    SearchNext,
    SearchPrevious,
    Command,
    Help,
    Back,
    Quit,
}

impl Action {
    pub const ALL: [Self; 11] = [
        Self::MoveDown,
        Self::MoveUp,
        Self::PageDown,
        Self::PageUp,
        Self::Search,
        Self::SearchNext,
        Self::SearchPrevious,
        Self::Command,
        Self::Help,
        Self::Back,
        Self::Quit,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::MoveDown => "move-down",
            Self::MoveUp => "move-up",
            Self::PageDown => "page-down",
            Self::PageUp => "page-up",
            Self::Search => "search",
            Self::SearchNext => "search-next",
            Self::SearchPrevious => "search-previous",
            Self::Command => "command",
            Self::Help => "help",
            Self::Back => "back",
            Self::Quit => "quit",
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
    Escape,
    Enter,
    Backspace,
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
            Self::Escape => "esc".into(),
            Self::Enter => "enter".into(),
            Self::Backspace => "backspace".into(),
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
            "esc" | "escape" => Ok(Self::Escape),
            "enter" | "return" => Ok(Self::Enter),
            "backspace" | "bs" => Ok(Self::Backspace),
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
    pub batch_size: usize,
    pub bindings: BTreeMap<Key, Action>,
    pub source: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        let bindings = [
            (Key::Char('j'), Action::MoveDown),
            (Key::Down, Action::MoveDown),
            (Key::Char('k'), Action::MoveUp),
            (Key::Up, Action::MoveUp),
            (Key::PageDown, Action::PageDown),
            (Key::PageUp, Action::PageUp),
            (Key::Char('/'), Action::Search),
            (Key::Char('n'), Action::SearchNext),
            (Key::Char('N'), Action::SearchPrevious),
            (Key::Char(':'), Action::Command),
            (Key::Char('?'), Action::Help),
            (Key::Escape, Action::Back),
            (Key::Char('q'), Action::Quit),
            (Key::Ctrl('c'), Action::Quit),
        ]
        .into_iter()
        .collect();

        Self {
            log_command: vec!["log".into()],
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
                if values.first().map(String::as_str) != Some("log") {
                    return Err(ConfigError::new(
                        location,
                        "`set log` must start with the Git `log` subcommand",
                    ));
                }
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

fn without_optional_separator(input: &[String]) -> Vec<String> {
    let mut output = input.to_vec();
    if output.get(1).map(String::as_str) == Some("=") {
        output.remove(1);
    }
    output
}

fn default_path() -> Option<PathBuf> {
    default_path_from(
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn default_path_from(config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(config_home) = config_home {
        let path = Path::new(config_home);
        if !config_home.is_empty() && path.is_absolute() {
            return Some(path.join("gitlsd/config"));
        }
    }
    home.filter(|home| !home.is_empty())
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
    use super::*;

    #[test]
    fn parses_settings_bindings_quotes_and_comments() {
        let config = Config::parse(
            "set log = log --all --author 'Grace Hopper' '' = # comment\nset batch-size 7\nbind x move-down\nbind = quit\nbind semicolon = help",
            Path::new("sample.conf"),
        )
        .unwrap();

        assert_eq!(
            config.log_command,
            ["log", "--all", "--author", "Grace Hopper", "", "="]
        );
        assert_eq!(config.batch_size, 7);
        assert_eq!(config.action_for(&Key::Char('x')), Some(Action::MoveDown));
        assert_eq!(config.action_for(&Key::Char('=')), Some(Action::Quit));
        assert_eq!(config.action_for(&Key::Char(';')), Some(Action::Help));
        assert_eq!(
            config.help_lines()[0],
            "setting.log=\"log\" \"--all\" \"--author\" \"Grace Hopper\" \"\" \"=\""
        );
    }

    #[test]
    fn xdg_path_requires_nonempty_absolute_value() {
        let home = OsStr::new("/home/tester");
        assert_eq!(
            default_path_from(Some(OsStr::new("")), Some(home)),
            Some(PathBuf::from("/home/tester/.config/gitlsd/config"))
        );
        assert_eq!(
            default_path_from(Some(OsStr::new("relative")), Some(home)),
            Some(PathBuf::from("/home/tester/.config/gitlsd/config"))
        );
        assert_eq!(
            default_path_from(Some(OsStr::new("/xdg")), Some(home)),
            Some(PathBuf::from("/xdg/gitlsd/config"))
        );
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
}
