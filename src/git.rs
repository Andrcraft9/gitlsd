use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECORD_FORMAT: &str = "%H%x00%h%x00%an%x00%cs%x00%s";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRecord {
    pub id: String,
    pub short_id: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

pub trait HistorySource {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError>;
}

#[derive(Debug)]
pub struct GitHistory {
    directory: PathBuf,
    command: Vec<String>,
}

impl GitHistory {
    pub fn new(directory: impl Into<PathBuf>, command: Vec<String>) -> Self {
        Self {
            directory: directory.into(),
            command,
        }
    }
}

impl HistorySource for GitHistory {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError> {
        let format_argument = format!("--format={RECORD_FORMAT}");
        let skip_argument = format!("--skip={offset}");
        let count_argument = format!("--max-count={limit}");
        let mut arguments = self.command.clone();
        let mut before_pathspecs = true;
        arguments.retain(|argument| {
            if argument == "--" {
                before_pathspecs = false;
                true
            } else {
                !before_pathspecs || !expands_output(argument)
            }
        });
        let insertion = arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(arguments.len());
        arguments.splice(
            insertion..insertion,
            [
                "--no-color".to_owned(),
                "--no-decorate".to_owned(),
                "--no-patch".to_owned(),
                "--no-show-signature".to_owned(),
                "--no-notes".to_owned(),
                format_argument,
                "-z".to_owned(),
                skip_argument,
                count_argument,
            ],
        );
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
            let stderr = sanitize_text(&String::from_utf8_lossy(&output.stderr))
                .trim()
                .to_owned();
            return Err(GitError::Process {
                status: output.status.code(),
                stderr,
            });
        }

        parse_records(&output.stdout)
    }
}

fn expands_output(argument: &str) -> bool {
    matches!(
        argument,
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
            | "--log-size"
            | "-c"
            | "--cc"
    ) || argument.starts_with("--stat=")
        || argument.starts_with("--dirstat")
        || argument.starts_with("--notes=")
        || argument.starts_with("-p-")
        || argument.starts_with("-p+")
        || argument.starts_with("-pU")
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

fn parse_records(output: &[u8]) -> Result<Vec<CommitRecord>, GitError> {
    let mut fields: Vec<_> = output.split(|byte| *byte == 0).collect();
    if fields.last() == Some(&&[][..]) {
        fields.pop();
    }
    if fields.len() % 5 != 0 {
        return Err(GitError::Output(format!(
            "output had {} fields, which is not a multiple of 5",
            fields.len()
        )));
    }
    Ok(fields
        .chunks_exact(5)
        .map(|fields| CommitRecord {
            id: sanitize_bytes(fields[0]),
            short_id: sanitize_bytes(fields[1]),
            author: sanitize_bytes(fields[2]),
            date: sanitize_bytes(fields[3]),
            subject: sanitize_bytes(fields[4]),
        })
        .collect())
}

fn sanitize_bytes(input: &[u8]) -> String {
    sanitize_text(&String::from_utf8_lossy(input))
}

fn sanitize_text(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

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
    fn parses_delimited_git_records() {
        let records =
            parse_records(b"abcdef\0abcdef\0A U Thor\x002026-09-05\0A subject\0").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].author, "A U Thor");
        assert_eq!(records[0].subject, "A subject");
    }

    #[test]
    fn rejects_malformed_records() {
        let error = parse_records(b"not-a-record\0").unwrap_err();
        assert!(error.to_string().contains("not a multiple of 5"));
    }

    #[test]
    fn decoding_is_lossy_and_neutralizes_terminal_controls() {
        let records = parse_records(b"id\0short\0A\xff\x1b[31m\x002026\0sub\tject\0").unwrap();
        assert_eq!(records[0].author, "A��[31m");
        assert_eq!(records[0].subject, "sub�ject");
        assert!(!records[0].author.chars().any(char::is_control));
    }
}
