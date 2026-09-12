//! Repository boundary and Git CLI adapter.
//!
//! [`HistorySource`] is the application-facing contract. [`GitHistory`]
//! implements it by executing configured Git commands directly, pairing
//! Git-rendered rows with stable commit IDs, loading cohesive show data, and
//! sanitizing terminal output.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRecord {
    pub id: String,
    /// Git-produced text with only printable characters and safe SGR sequences.
    pub display: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangedFile {
    /// Git-produced name-status text with only printable characters and safe
    /// SGR sequences.
    pub display: String,
    /// The path on the old side of a rename or copy, when Git reports one.
    pub old_path: Option<String>,
    /// The path on the new side of a rename or copy, when Git reports one.
    pub new_path: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShowData {
    pub metadata: Vec<String>,
    pub files: Vec<ChangedFile>,
    pub diff: Vec<String>,
}

pub trait HistorySource {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError>;
    fn load_preview(&mut self, _id: &str) -> Result<Vec<String>, GitError> {
        Ok(Vec::new())
    }
    fn load_show(&mut self, _id: &str) -> Result<ShowData, GitError> {
        Ok(ShowData::default())
    }
}

#[derive(Debug)]
pub struct GitHistory {
    directory: PathBuf,
    command: Vec<String>,
    preview_command: Vec<String>,
    show_commit_command: Vec<String>,
    show_command: Vec<String>,
}

impl GitHistory {
    pub fn new(
        directory: impl Into<PathBuf>,
        command: Vec<String>,
        preview_command: Vec<String>,
        show_commit_command: Vec<String>,
        show_command: Vec<String>,
    ) -> Self {
        Self {
            directory: directory.into(),
            command,
            preview_command,
            show_commit_command,
            show_command,
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

    fn run_show(&self, stage: &str, arguments: &[String]) -> Result<Vec<u8>, GitError> {
        let output = Command::new("git")
            .arg("--no-pager")
            .args(arguments)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .current_dir(&self.directory)
            .output()
            .map_err(|error| GitError::Show {
                stage: stage.into(),
                reason: format!("could not run Git: {error}"),
            })?;
        if !output.status.success() {
            return Err(GitError::Show {
                stage: stage.into(),
                reason: format!(
                    "exit {}: {}",
                    output
                        .status
                        .code()
                        .map_or_else(|| "signal".into(), |code| code.to_string()),
                    if output.stderr.is_empty() {
                        "no error output".into()
                    } else {
                        safe_text(&String::from_utf8_lossy(&output.stderr), false)
                            .trim()
                            .to_owned()
                    }
                ),
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
        let arguments = command_with_commit(&self.preview_command, id);
        Ok(String::from_utf8_lossy(&self.run(&arguments)?)
            .lines()
            .map(|line| safe_text(line, true))
            .collect())
    }

    fn load_show(&mut self, id: &str) -> Result<ShowData, GitError> {
        let metadata = String::from_utf8_lossy(&self.run_show(
            "metadata",
            &command_with_commit(&self.show_commit_command, id),
        )?)
        .lines()
        .map(|line| safe_text(line, true))
        .collect();

        let files = parse_changed_files(&self.run_show(
            "file list",
            &[
                "show".into(),
                "--name-status".into(),
                "--format=".into(),
                "--color=always".into(),
                id.into(),
            ],
        )?)?;

        let diff = String::from_utf8_lossy(
            &self.run_show("diff", &command_with_commit(&self.show_command, id))?,
        )
        .lines()
        .map(|line| safe_text(line, true))
        .collect();

        Ok(ShowData {
            metadata,
            files,
            diff,
        })
    }
}

fn command_with_commit(command: &[String], id: &str) -> Vec<String> {
    let mut arguments = command[1..].to_vec();
    let insertion = arguments
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(arguments.len());
    arguments.insert(insertion, id.into());
    arguments
}

fn parse_changed_files(output: &[u8]) -> Result<Vec<ChangedFile>, GitError> {
    output_lines(output)
        .into_iter()
        .map(|line| {
            let line = String::from_utf8_lossy(line);
            let fields = line.split('\t').collect::<Vec<_>>();
            let Some(status) = fields.first().copied() else {
                return Err(GitError::Show {
                    stage: "file list".into(),
                    reason: "empty name-status row".into(),
                });
            };
            let paths = &fields[1..];
            if paths.is_empty() || paths.len() > 2 {
                return Err(GitError::Show {
                    stage: "file list".into(),
                    reason: format!("invalid name-status row `{line}`"),
                });
            }
            let status = safe_text(status, false);
            let display = safe_text(&line.replace('\t', " "), true);
            let path = |value: &str| sanitized_git_path(value);
            let (old_path, new_path) = if status.starts_with(['R', 'C']) {
                if paths.len() != 2 {
                    return Err(GitError::Show {
                        stage: "file list".into(),
                        reason: format!("invalid rename/copy row `{line}`"),
                    });
                }
                (Some(path(paths[0])), Some(path(paths[1])))
            } else if status.starts_with('D') {
                (Some(path(paths[0])), None)
            } else if status.starts_with('A') {
                (None, Some(path(paths[0])))
            } else {
                (Some(path(paths[0])), Some(path(paths[0])))
            };
            Ok(ChangedFile {
                display,
                old_path,
                new_path,
            })
        })
        .collect()
}

/// Return the first `diff --git` line that belongs to `file`.
pub fn patch_offset(lines: &[String], file: &ChangedFile) -> Option<usize> {
    lines.iter().enumerate().find_map(|(index, line)| {
        let line = safe_text(line, false);
        let rest = line.strip_prefix("diff --git ")?;
        diff_header_paths(rest)
            .into_iter()
            .find_map(|(old_path, new_path)| {
                let matches_old = file
                    .old_path
                    .as_deref()
                    .is_some_and(|path| old_path == format!("a/{path}") || old_path == path);
                let matches_new = file
                    .new_path
                    .as_deref()
                    .is_some_and(|path| new_path == format!("b/{path}") || new_path == path);
                let matches = match (file.old_path.as_deref(), file.new_path.as_deref()) {
                    (Some(_), Some(_)) => matches_old && matches_new,
                    (Some(_), None) => matches_old,
                    (None, Some(_)) => matches_new,
                    (None, None) => false,
                };
                matches.then_some(index)
            })
    })
}

/// Return the changed-file row whose patch contains `offset`.
pub fn file_at_patch_offset(
    lines: &[String],
    files: &[ChangedFile],
    offset: usize,
) -> Option<usize> {
    files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| patch_offset(lines, file).map(|start| (start, index)))
        .filter(|(start, _)| *start <= offset)
        .max_by_key(|(start, _)| *start)
        .map(|(_, index)| index)
}

fn diff_header_paths(input: &str) -> Vec<(String, String)> {
    if input.starts_with('"') {
        let mut tokens = git_path_tokens(input);
        return match (tokens.next(), tokens.next()) {
            (Some(old_path), Some(new_path)) => {
                vec![(sanitized_git_path(&old_path), sanitized_git_path(&new_path))]
            }
            _ => Vec::new(),
        };
    }

    input
        .match_indices(" b/")
        .filter_map(|(index, _)| {
            let old_path = &input[..index];
            let new_path = &input[index + 1..];
            (old_path.starts_with("a/") && new_path.starts_with("b/"))
                .then(|| (sanitized_git_path(old_path), sanitized_git_path(new_path)))
        })
        .collect()
}

fn git_path_tokens(input: &str) -> impl Iterator<Item = String> + '_ {
    struct Tokens<'a> {
        input: &'a str,
        position: usize,
    }

    impl Iterator for Tokens<'_> {
        type Item = String;

        fn next(&mut self) -> Option<Self::Item> {
            while self
                .input
                .get(self.position..)?
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                self.position += self.input[self.position..].chars().next()?.len_utf8();
            }
            let remainder = self.input.get(self.position..)?;
            if remainder.is_empty() {
                return None;
            }
            if let Some(remainder) = remainder.strip_prefix('"') {
                let quoted = format!("\"{remainder}");
                if let Some((value, consumed)) = parse_quoted_git_path(&quoted) {
                    self.position += consumed;
                    return Some(value);
                }
                let malformed = self.input[self.position..].to_owned();
                self.position = self.input.len();
                Some(malformed)
            } else {
                let end = remainder
                    .find(char::is_whitespace)
                    .unwrap_or(remainder.len());
                self.position += end;
                Some(remainder[..end].to_owned())
            }
        }
    }

    Tokens { input, position: 0 }
}

fn sanitized_git_path(value: &str) -> String {
    safe_text(&decode_git_path(value), false)
}

fn decode_git_path(value: &str) -> String {
    parse_quoted_git_path(value).map_or_else(|| value.to_owned(), |(decoded, _)| decoded)
}

fn parse_quoted_git_path(input: &str) -> Option<(String, usize)> {
    if !input.starts_with('"') {
        return None;
    }

    let mut bytes = Vec::new();
    let mut position = 1;
    while position < input.len() {
        let character = input[position..].chars().next()?;
        position += character.len_utf8();
        match character {
            '"' => return Some((String::from_utf8_lossy(&bytes).into_owned(), position)),
            '\\' => {
                let escaped = input[position..].chars().next()?;
                position += escaped.len_utf8();
                match escaped {
                    '0'..='7' => {
                        let mut value = escaped as u8 - b'0';
                        for _ in 0..2 {
                            let Some(digit) = input[position..].chars().next() else {
                                break;
                            };
                            if !matches!(digit, '0'..='7') {
                                break;
                            }
                            position += digit.len_utf8();
                            value = value * 8 + digit as u8 - b'0';
                        }
                        bytes.push(value);
                    }
                    'a' => bytes.push(b'\x07'),
                    'b' => bytes.push(b'\x08'),
                    't' => bytes.push(b'\t'),
                    'n' => bytes.push(b'\n'),
                    'v' => bytes.push(b'\x0b'),
                    'f' => bytes.push(b'\x0c'),
                    'r' => bytes.push(b'\r'),
                    other => {
                        let mut encoded = [0; 4];
                        bytes.extend_from_slice(other.encode_utf8(&mut encoded).as_bytes());
                    }
                }
            }
            other => {
                let mut encoded = [0; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut encoded).as_bytes());
            }
        }
    }
    None
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

#[derive(Clone, Debug)]
pub enum GitError {
    Launch { directory: PathBuf, reason: String },
    Process { status: Option<i32>, stderr: String },
    Output(String),
    Show { stage: String, reason: String },
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
            Self::Show { stage, reason } => write!(formatter, "Git show {stage} failed: {reason}"),
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

    #[test]
    fn parses_name_status_before_sanitizing_and_keeps_rename_paths() {
        let files = parse_changed_files(
            b"M\tmodified.rs\nA\tadded.rs\nD\tdeleted.rs\nR100\told.rs\tnew.rs\nC100\tsource.rs\tcopy.rs\n",
        )
        .unwrap();

        assert_eq!(files[0].display, "M modified.rs");
        assert_eq!(files[0].old_path.as_deref(), Some("modified.rs"));
        assert_eq!(files[0].new_path.as_deref(), Some("modified.rs"));
        assert_eq!(files[1].old_path, None);
        assert_eq!(files[1].new_path.as_deref(), Some("added.rs"));
        assert_eq!(files[2].old_path.as_deref(), Some("deleted.rs"));
        assert_eq!(files[2].new_path, None);
        assert_eq!(files[3].old_path.as_deref(), Some("old.rs"));
        assert_eq!(files[3].new_path.as_deref(), Some("new.rs"));
        assert_eq!(files[4].old_path.as_deref(), Some("source.rs"));
        assert_eq!(files[4].new_path.as_deref(), Some("copy.rs"));
    }

    #[test]
    fn patch_offsets_match_ordinary_and_rename_headers() {
        let files = parse_changed_files(b"M\tone.rs\nR100\told.rs\tnew.rs\n").unwrap();
        let diff = vec![
            "diff --git a/one.rs b/one.rs".into(),
            "@@ -1 +1 @@".into(),
            "diff --git a/old.rs b/new.rs".into(),
            "similarity index 100%".into(),
        ];
        assert_eq!(patch_offset(&diff, &files[0]), Some(0));
        assert_eq!(patch_offset(&diff, &files[1]), Some(2));
    }

    #[test]
    fn patch_offsets_support_git_quoted_paths_and_degrade_for_custom_output() {
        let file = ChangedFile {
            display: "R100 old name.rs new name.rs".into(),
            old_path: Some("old name.rs".into()),
            new_path: Some("new name.rs".into()),
        };
        let diff = vec!["diff --git \"a/old name.rs\" \"b/new name.rs\"".into()];
        assert_eq!(patch_offset(&diff, &file), Some(0));
        assert_eq!(patch_offset(&["custom output".into()], &file), None);
    }

    #[test]
    fn patch_offsets_support_unquoted_spaces_and_c_style_path_escapes() {
        let files = parse_changed_files(
            b"M\tdir with space/file name.rs\nM\t\"a-\\303\\251.txt\"\nM\t\"a\\tname.txt\"\n",
        )
        .unwrap();
        let diff = vec![
            "diff --git a/dir with space/file name.rs b/dir with space/file name.rs".into(),
            "diff --git \"a/a-\\303\\251.txt\" \"b/a-\\303\\251.txt\"".into(),
            "diff --git \"a/a\\tname.txt\" \"b/a\\tname.txt\"".into(),
        ];

        assert_eq!(
            files[0].old_path.as_deref(),
            Some("dir with space/file name.rs")
        );
        assert_eq!(files[1].old_path.as_deref(), Some("a-é.txt"));
        assert_eq!(files[2].old_path.as_deref(), Some("a�name.txt"));
        assert_eq!(patch_offset(&diff, &files[0]), Some(0));
        assert_eq!(patch_offset(&diff, &files[1]), Some(1));
        assert_eq!(patch_offset(&diff, &files[2]), Some(2));
    }

    #[test]
    fn maps_diff_offsets_back_to_changed_file_rows() {
        let files = parse_changed_files(b"M\tone.rs\nM\ttwo.rs\n").unwrap();
        let diff = vec![
            "diff --git a/one.rs b/one.rs".into(),
            "one change".into(),
            "diff --git a/two.rs b/two.rs".into(),
            "two change".into(),
        ];
        assert_eq!(file_at_patch_offset(&diff, &files, 0), Some(0));
        assert_eq!(file_at_patch_offset(&diff, &files, 1), Some(0));
        assert_eq!(file_at_patch_offset(&diff, &files, 2), Some(1));
        assert_eq!(file_at_patch_offset(&diff, &files, 3), Some(1));
        assert_eq!(file_at_patch_offset(&diff, &files, 4), Some(1));
        assert_eq!(file_at_patch_offset(&["metadata".into()], &files, 0), None);
    }

    #[test]
    fn malformed_name_status_rows_report_file_list_errors() {
        let error = parse_changed_files(b"M\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Git show file list failed: invalid name-status row `M`"
        );
    }

    #[test]
    fn commit_ids_are_inserted_before_configured_pathspecs() {
        let command = vec![
            "git".into(),
            "show".into(),
            "--format=%H".into(),
            "--".into(),
            "src".into(),
        ];
        assert_eq!(
            command_with_commit(&command, "commit"),
            ["show", "--format=%H", "commit", "--", "src"]
        );
    }
}
