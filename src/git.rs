//! Repository boundary and Git CLI adapter.
//!
//! [`HistorySource`] is the application-facing contract. [`GitHistory`]
//! implements it by executing configured Git commands directly, pairing
//! Git-rendered rows with stable commit IDs, and sanitizing terminal output.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRecord {
    pub id: String,
    /// Git-produced text with only printable characters and safe SGR sequences.
    pub display: String,
}

pub trait HistorySource {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError>;
    fn load_preview(&mut self, _id: &str) -> Result<Vec<String>, GitError> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
pub struct GitHistory {
    directory: PathBuf,
    command: Vec<String>,
    preview_command: Vec<String>,
}

impl GitHistory {
    pub fn new(
        directory: impl Into<PathBuf>,
        command: Vec<String>,
        preview_command: Vec<String>,
    ) -> Self {
        Self {
            directory: directory.into(),
            command,
            preview_command,
        }
    }

    fn run(&self, arguments: &[String]) -> Result<Vec<u8>, GitError> {
        let output = Command::new("git")
            .arg("--no-pager")
            .args(arguments)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .current_dir(&self.directory)
            .output()
            .map_err(|error| GitError::Launch {
                directory: self.directory.clone(),
                reason: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(GitError::Process {
                status: output.status.code(),
                stderr: safe_text(&String::from_utf8_lossy(&output.stderr), false)
                    .trim()
                    .to_owned(),
            });
        }
        Ok(output.stdout)
    }
}

impl HistorySource for GitHistory {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError> {
        let mut arguments = self.command[1..].to_vec();
        let insertion = arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(arguments.len());
        arguments.splice(
            insertion..insertion,
            [format!("--skip={offset}"), format!("--max-count={limit}")],
        );
        let display = self.run(&arguments)?;
        let insertion = arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(arguments.len());
        // These presentation overrides apply only to the companion identity query.
        arguments.splice(
            insertion..insertion,
            [
                "--format=%H".into(),
                "--no-color".into(),
                "--no-decorate".into(),
            ],
        );
        let ids = self.run(&arguments)?;
        pair_records(&display, &ids)
    }

    fn load_preview(&mut self, id: &str) -> Result<Vec<String>, GitError> {
        let mut arguments = self.preview_command[1..].to_vec();
        let insertion = arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(arguments.len());
        arguments.insert(insertion, id.into());
        Ok(String::from_utf8_lossy(&self.run(&arguments)?)
            .lines()
            .map(|line| safe_text(line, true))
            .collect())
    }
}

fn output_lines(output: &[u8]) -> Vec<&[u8]> {
    if output.is_empty() {
        return Vec::new();
    }
    output
        .strip_suffix(b"\n")
        .unwrap_or(output)
        .split(|byte| *byte == b'\n')
        .collect()
}

fn pair_records(display: &[u8], ids: &[u8]) -> Result<Vec<CommitRecord>, GitError> {
    let rows = output_lines(display);
    let ids = output_lines(ids);
    if rows.len() != ids.len()
        || ids
            .iter()
            .any(|id| !matches!(id.len(), 40 | 64) || !id.iter().all(u8::is_ascii_hexdigit))
    {
        return Err(GitError::Output("configured output does not contain exactly one line per commit; use `set log git log --oneline` or a one-line `--format`".into()));
    }
    Ok(rows
        .into_iter()
        .zip(ids)
        .map(|(row, id)| CommitRecord {
            id: String::from_utf8_lossy(id).into_owned(),
            display: safe_text(&String::from_utf8_lossy(row), true),
        })
        .collect())
}

/// SGR has no cursor, clipboard, title, or other terminal side effects.
pub fn safe_text(input: &str, retain_sgr: bool) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            let mut parameters = String::new();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || *c == ';')
            {
                parameters.push(chars.next().unwrap());
            }
            if chars.peek() == Some(&'m') {
                chars.next();
                if retain_sgr {
                    output.push_str(&format!("\x1b[{parameters}m"));
                }
            } else {
                output.push('�');
                output.push('[');
                output.push_str(&parameters);
            }
        } else if character.is_control() {
            output.push('�');
        } else {
            output.push(character);
        }
    }
    output
}

#[derive(Debug)]
pub enum GitError {
    Launch { directory: PathBuf, reason: String },
    Process { status: Option<i32>, stderr: String },
    Output(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Launch { directory, reason } => write!(
                formatter,
                "could not run Git in {}: {reason}",
                directory.display()
            ),
            Self::Process { status, stderr } => write!(
                formatter,
                "Git log failed (exit {}): {}",
                status.map_or_else(|| "signal".into(), |code| code.to_string()),
                if stderr.is_empty() {
                    "no error output"
                } else {
                    stderr
                }
            ),
            Self::Output(reason) => write!(formatter, "could not parse Git log output: {reason}"),
        }
    }
}
impl std::error::Error for GitError {}

pub fn current_directory() -> Result<PathBuf, GitError> {
    std::env::current_dir().map_err(|error| GitError::Launch {
        directory: Path::new(".").to_path_buf(),
        reason: format!("could not determine current directory: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairs_git_text_without_rebuilding_it() {
        let id = "a".repeat(40);
        let rows = pair_records(b"custom row\n", format!("{id}\n").as_bytes()).unwrap();
        assert_eq!(rows[0].display, "custom row");
        assert_eq!(rows[0].id, id);
        assert!(pair_records(b"two\nlines\n", format!("{id}\n").as_bytes()).is_err());
    }
    #[test]
    fn only_sgr_survives_and_plain_text_is_stable() {
        let input = "\x1b[31mred\x1b[m\x1b[2J\x1b]52;clipboard\x07\t";
        let safe = safe_text(input, true);
        assert_eq!(safe, "\x1b[31mred\x1b[m�[2J�]52;clipboard��");
        assert_eq!(safe_text(&safe, false), "red�[2J�]52;clipboard��");
    }
}
