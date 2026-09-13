//! Repository boundary and Git CLI adapter.
//!
//! [`HistorySource`] is the application-facing contract. [`GitHistory`]
//! implements it by executing configured Git commands directly, pairing
//! Git-rendered rows with stable commit IDs, loading cohesive show and status
//! data, performing whole-file status mutations, filtering diff presentation through
//! an optional direct subprocess, and sanitizing terminal output after filtering.
//! It also indexes recognized Git and delta patch headers independently of terminal
//! rendering so application navigation does not rescan loaded documents.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusFile {
    /// Git status text sanitized for display.
    pub display: String,
    /// The path before a rename or copy, retained as raw Git bytes.
    pub old_path: Option<Vec<u8>>,
    /// The path after a rename or copy, retained as raw Git bytes.
    pub new_path: Option<Vec<u8>>,
}

impl StatusFile {
    pub fn mutation_paths(&self) -> impl Iterator<Item = &[u8]> {
        self.old_path
            .iter()
            .chain(self.new_path.iter())
            .map(Vec::as_slice)
    }

    fn is_untracked(&self) -> bool {
        self.display.starts_with("?? ")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusData {
    pub staged: Vec<StatusFile>,
    pub unstaged: Vec<StatusFile>,
    pub staged_diff: Vec<String>,
    pub unstaged_diff: Vec<String>,
}

/// Patch-header locations for one loaded diff document.
///
/// Direct file lookup retains the first matching header. Reverse lookup is sorted
/// by source position and selects the latest header at or before an offset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchIndex {
    file_to_line: Vec<Option<usize>>,
    patch_starts: Vec<(usize, usize)>,
}

impl PatchIndex {
    pub fn for_changed_files(lines: &[String], files: &[ChangedFile]) -> Self {
        Self::build(lines, files)
    }

    pub fn for_status_files(lines: &[String], files: &[StatusFile]) -> Self {
        let files = files
            .iter()
            .map(|file| ChangedFile {
                display: file.display.clone(),
                old_path: file.old_path.as_deref().map(display_raw_path),
                new_path: file.new_path.as_deref().map(display_raw_path),
            })
            .collect::<Vec<_>>();
        Self::build(lines, &files)
    }

    pub fn offset_for(&self, file_index: usize) -> Option<usize> {
        self.file_to_line.get(file_index).copied().flatten()
    }

    pub fn file_at(&self, offset: usize) -> Option<usize> {
        let end = self
            .patch_starts
            .partition_point(|(start, _)| *start <= offset);
        end.checked_sub(1)
            .and_then(|index| self.patch_starts.get(index))
            .map(|(_, file_index)| *file_index)
    }

    fn build(lines: &[String], files: &[ChangedFile]) -> Self {
        let mut file_lookup = HashMap::<String, Vec<usize>>::new();
        for (file_index, file) in files.iter().enumerate() {
            for path in file.old_path.iter().chain(file.new_path.iter()) {
                file_lookup
                    .entry(path.clone())
                    .or_default()
                    .push(file_index);
            }
        }
        let plain = lines
            .iter()
            .map(|line| safe_text(line, false))
            .collect::<Vec<_>>();
        let mut index = Self {
            file_to_line: vec![None; files.len()],
            patch_starts: Vec::new(),
        };
        for (line_index, line) in plain.iter().enumerate() {
            let file_indices = if let Some(rest) = line.strip_prefix("diff --git ") {
                diff_header_paths(rest)
                    .into_iter()
                    .flat_map(|(old_path, new_path)| {
                        matching_file_indices(
                            files,
                            &file_lookup,
                            &[&old_path, &new_path],
                            |file| diff_header_matches_file(file, &old_path, &new_path),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                let next_is_delta_rule = plain.get(line_index + 1).is_some_and(|next| {
                    !next.is_empty() && next.chars().all(|character| character == '─')
                });
                if next_is_delta_rule {
                    matching_file_indices(files, &file_lookup, &delta_header_paths(line), |file| {
                        delta_header_matches_file(line, file)
                    })
                } else {
                    Vec::new()
                }
            };
            let mut file_indices = file_indices;
            file_indices.sort_unstable();
            file_indices.dedup();
            for file_index in file_indices {
                index.file_to_line[file_index].get_or_insert(line_index);
                index.patch_starts.push((line_index, file_index));
            }
        }
        index.patch_starts.sort_by_key(|(start, _)| *start);
        index
    }
}

fn matching_file_indices(
    files: &[ChangedFile],
    file_lookup: &HashMap<String, Vec<usize>>,
    header_paths: &[&str],
    matches: impl Fn(&ChangedFile) -> bool,
) -> Vec<usize> {
    let mut candidates = HashSet::new();
    for path in header_paths {
        for variant in path_variants(path) {
            if let Some(indices) = file_lookup.get(variant) {
                candidates.extend(indices);
            }
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates.extend(0..files.len());
    }
    candidates
        .into_iter()
        .filter(|index| matches(&files[*index]))
        .collect()
}

fn path_variants(path: &str) -> impl Iterator<Item = &str> {
    let stripped = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    std::iter::once(path).chain(
        std::iter::once(stripped).chain(
            stripped
                .char_indices()
                .filter(|(_, character)| *character == '/')
                .map(|(index, _)| &stripped[index + 1..]),
        ),
    )
}

fn delta_header_paths(header: &str) -> Vec<&str> {
    if let Some(paths) = header.strip_prefix("renamed: ") {
        return paths
            .split_once('⟶')
            .map(|(old, new)| vec![old.trim(), new.trim()])
            .unwrap_or_default();
    }
    if let Some(path) = header.strip_prefix("added: ") {
        return vec![path];
    }
    if let Some(path) = header.strip_prefix("removed: ") {
        return vec![path];
    }
    vec![header]
}

pub trait HistorySource {
    fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError>;
    fn load_preview(&mut self, _id: &str) -> Result<Vec<String>, GitError> {
        Ok(Vec::new())
    }
    fn load_show(&mut self, _id: &str) -> Result<ShowData, GitError> {
        Ok(ShowData::default())
    }
    fn load_status(&mut self) -> Result<StatusData, GitError> {
        Ok(StatusData::default())
    }
    fn toggle_stage(&mut self, _staged: bool, _file: &StatusFile) -> Result<(), GitError> {
        Err(GitError::Status {
            stage: "mutation".into(),
            reason: "status mutations are unavailable".into(),
        })
    }
}

#[derive(Debug)]
pub struct GitHistory {
    directory: PathBuf,
    command: Vec<String>,
    preview_command: Vec<String>,
    show_commit_command: Vec<String>,
    show_command: Vec<String>,
    status_diff_command: Vec<String>,
    diff_filter: Option<Vec<String>>,
}

impl GitHistory {
    pub fn new(
        directory: impl Into<PathBuf>,
        command: Vec<String>,
        preview_command: Vec<String>,
        show_commit_command: Vec<String>,
        show_command: Vec<String>,
        status_diff_command: Vec<String>,
        diff_filter: Option<Vec<String>>,
    ) -> Self {
        Self {
            directory: directory.into(),
            command,
            preview_command,
            show_commit_command,
            show_command,
            status_diff_command,
            diff_filter,
        }
    }

    /// Write stdin while independent readers drain both output pipes.
    /// Always join the writer and reap the child, including on I/O failures.
    fn presentation(&self, input: Vec<u8>, operation: &str) -> Result<Vec<String>, GitError> {
        let output = if input.is_empty() {
            input
        } else if let Some(command) = &self.diff_filter {
            let Some((executable, arguments)) = command.split_first() else {
                return Err(GitError::Filter {
                    operation: operation.into(),
                    command: String::new(),
                    reason: "filter command is empty".into(),
                });
            };
            let failure = |reason: String| GitError::Filter {
                operation: operation.into(),
                command: safe_text(executable, false),
                reason: safe_text(&reason, false),
            };
            let mut child = Command::new(executable)
                .args(arguments)
                .current_dir(&self.directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| failure(format!("could not launch: {error}")))?;
            let mut stdin = child.stdin.take().expect("piped stdin");
            let writer = thread::Builder::new().spawn(move || stdin.write_all(&input));
            let writer = match writer {
                Ok(writer) => writer,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(failure(format!("could not start stdin writer: {error}")));
                }
            };
            let mut stdout = child.stdout.take().expect("piped stdout");
            let mut stderr = child.stderr.take().expect("piped stderr");
            let collected = thread::scope(|scope| {
                let (sender, receiver) = std::sync::mpsc::channel();
                let mut read_error = None;
                for (is_stdout, pipe) in [
                    (true, &mut stdout as &mut (dyn Read + Send)),
                    (false, &mut stderr as &mut (dyn Read + Send)),
                ] {
                    let sender = sender.clone();
                    let reader = thread::Builder::new().spawn_scoped(scope, move || {
                        let mut bytes = Vec::new();
                        let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
                        let _ = sender.send((is_stdout, result));
                    });
                    if let Err(error) = reader {
                        let _ = child.kill();
                        read_error = Some(error);
                        break;
                    }
                }
                drop(sender);
                let mut output = Vec::new();
                let mut errors = Vec::new();
                for (is_stdout, result) in receiver {
                    match result {
                        Ok(bytes) if is_stdout => output = bytes,
                        Ok(bytes) => errors = bytes,
                        Err(error) => {
                            let _ = child.kill();
                            read_error = Some(error);
                        }
                    }
                }
                (output, errors, read_error)
            });
            let status = child.wait();
            let written = writer.join();
            let (output, stderr, read_error) = collected;
            let status =
                status.map_err(|error| failure(format!("could not reap filter: {error}")))?;
            if let Some(error) = read_error {
                return Err(failure(format!("could not collect output: {error}")));
            }
            if !status.success() {
                return Err(failure(format!(
                    "exit {}: {}",
                    status
                        .code()
                        .map_or_else(|| "signal".into(), |code| code.to_string()),
                    if stderr.is_empty() {
                        "no error output".into()
                    } else {
                        String::from_utf8_lossy(&stderr).trim().to_owned()
                    }
                )));
            }
            written
                .map_err(|_| failure("stdin writer panicked".into()))?
                .map_err(|error| failure(format!("could not write stdin: {error}")))?;
            output
        } else {
            input
        };
        Ok(String::from_utf8_lossy(&output)
            .lines()
            .map(|line| safe_text(line, true))
            .collect())
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

    fn repository_root(&self) -> Result<PathBuf, GitError> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--show-toplevel")
            .env("LC_ALL", "C")
            .current_dir(&self.directory)
            .output()
            .map_err(|error| GitError::Status {
                stage: "repository root".into(),
                reason: format!("could not run Git: {error}"),
            })?;
        if !output.status.success() {
            return Err(GitError::Status {
                stage: "repository root".into(),
                reason: format!(
                    "exit {}: {}",
                    output
                        .status
                        .code()
                        .map_or_else(|| "signal".into(), |code| code.to_string()),
                    safe_text(&String::from_utf8_lossy(&output.stderr), false)
                        .trim()
                        .to_owned()
                ),
            });
        }
        let root = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
        Ok(os_string_from_bytes(root).into())
    }

    fn run_status_command(
        &self,
        directory: &Path,
        arguments: &[OsString],
        stage: &str,
        accept_differences: bool,
    ) -> Result<Vec<u8>, GitError> {
        let output = Command::new("git")
            .arg("--no-pager")
            .args(arguments)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("LC_ALL", "C")
            .current_dir(directory)
            .output()
            .map_err(|error| GitError::Status {
                stage: stage.into(),
                reason: format!("could not run Git: {error}"),
            })?;
        if !(output.status.success() || accept_differences && output.status.code() == Some(1)) {
            return Err(GitError::Status {
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

    fn status_diff_arguments(&self, staged: bool) -> Vec<OsString> {
        let mut arguments = self.status_diff_command[1..]
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        if staged {
            arguments.insert(1.min(arguments.len()), OsString::from("--staged"));
        }
        arguments
    }

    fn untracked_diff_arguments(&self, path: &[u8]) -> Vec<OsString> {
        let mut arguments = self.status_diff_command[1..]
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .map(OsString::from)
            .collect::<Vec<_>>();
        arguments.extend([
            OsString::from("--no-index"),
            OsString::from("--"),
            OsString::from(null_device()),
            os_string_from_bytes(path),
        ]);
        arguments
    }

    fn load_untracked_diffs(&self, root: &Path, files: &[StatusFile]) -> Result<Vec<u8>, GitError> {
        let mut lines = Vec::new();
        for file in files.iter().filter(|file| file.is_untracked()) {
            let Some(path) = file.new_path.as_deref() else {
                continue;
            };
            let arguments = self.untracked_diff_arguments(path);
            let output = self.run_status_command(root, &arguments, "untracked diff", true)?;
            lines.extend(output);
        }
        Ok(lines)
    }

    fn has_head(&self, root: &Path) -> Result<bool, GitError> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--verify")
            .arg("HEAD")
            .env("LC_ALL", "C")
            .current_dir(root)
            .output()
            .map_err(|error| GitError::Status {
                stage: "HEAD discovery".into(),
                reason: format!("could not run Git: {error}"),
            })?;
        if output.status.success() {
            Ok(true)
        } else if is_unborn_error(&String::from_utf8_lossy(&output.stderr)) {
            Ok(false)
        } else {
            Err(GitError::Status {
                stage: "HEAD discovery".into(),
                reason: safe_text(&String::from_utf8_lossy(&output.stderr), false)
                    .trim()
                    .to_owned(),
            })
        }
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
        let display = match self.run(&arguments) {
            Err(GitError::Process { ref stderr, .. }) if is_unborn_error(stderr) => {
                return Err(GitError::Unborn);
            }
            result => result?,
        };
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
        self.presentation(self.run(&arguments)?, "preview")
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

        let diff = self.presentation(
            self.run_show("diff", &command_with_commit(&self.show_command, id))?,
            "show diff",
        )?;

        Ok(ShowData {
            metadata,
            files,
            diff,
        })
    }

    fn load_status(&mut self) -> Result<StatusData, GitError> {
        let root = self.repository_root()?;
        let output = self.run_status_command(
            &root,
            &[
                "status".into(),
                "--renames".into(),
                "--porcelain=v1".into(),
                "-z".into(),
                "--untracked-files=all".into(),
            ],
            "discovery",
            false,
        )?;
        let (staged, unstaged) = parse_status(&output)?;
        let staged_diff = self.presentation(
            self.run_status_command(
                &root,
                &self.status_diff_arguments(true),
                "staged diff",
                false,
            )?,
            "staged diff",
        )?;
        let mut unstaged_bytes = self.run_status_command(
            &root,
            &self.status_diff_arguments(false),
            "unstaged diff",
            false,
        )?;
        unstaged_bytes.extend(self.load_untracked_diffs(&root, &unstaged)?);
        let unstaged_diff = self.presentation(unstaged_bytes, "unstaged diff")?;
        Ok(StatusData {
            staged,
            unstaged,
            staged_diff,
            unstaged_diff,
        })
    }

    fn toggle_stage(&mut self, staged: bool, file: &StatusFile) -> Result<(), GitError> {
        let root = self.repository_root()?;
        let mut arguments = if staged {
            if self.has_head(&root)? {
                vec!["reset".into(), "HEAD".into(), "--".into()]
            } else {
                vec![
                    "rm".into(),
                    "--cached".into(),
                    "--force".into(),
                    "--".into(),
                ]
            }
        } else {
            vec!["add".into(), "--all".into(), "--".into()]
        };
        arguments.extend(file.mutation_paths().map(literal_path));
        self.run_status_command(&root, &arguments, "mutation", false)?;
        Ok(())
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

fn parse_status(output: &[u8]) -> Result<(Vec<StatusFile>, Vec<StatusFile>), GitError> {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut entries = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || entry[2] != b' ' {
            return Err(GitError::Status {
                stage: "discovery".into(),
                reason: format!(
                    "invalid porcelain status entry `{}`",
                    safe_text(&String::from_utf8_lossy(entry), false)
                ),
            });
        }
        let index_status = entry[0] as char;
        let worktree_status = entry[1] as char;
        let first_path = entry[3..].to_vec();
        let rename = matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C');
        let (old_path, new_path) = if rename {
            let Some(second_path) = entries.next() else {
                return Err(GitError::Status {
                    stage: "discovery".into(),
                    reason: "rename status entry has no second path".into(),
                });
            };
            // Porcelain v1 -z emits the destination first and the source
            // second. Keep both raw values for exact mutations.
            (Some(second_path.to_vec()), Some(first_path))
        } else if index_status == 'D' || worktree_status == 'D' {
            (Some(first_path), None)
        } else if index_status == 'A' || worktree_status == '?' {
            (None, Some(first_path))
        } else {
            (Some(first_path.clone()), Some(first_path))
        };
        let display = status_display(index_status, worktree_status, &old_path, &new_path);
        let file = StatusFile {
            display,
            old_path,
            new_path,
        };
        if index_status != ' ' && index_status != '?' {
            staged.push(file.clone());
        }
        if worktree_status != ' ' || index_status == '?' {
            unstaged.push(file);
        }
    }
    Ok((staged, unstaged))
}

fn status_display(
    index_status: char,
    worktree_status: char,
    old_path: &Option<Vec<u8>>,
    new_path: &Option<Vec<u8>>,
) -> String {
    let status = format!("{index_status}{worktree_status}");
    match (old_path, new_path) {
        (Some(old), Some(new)) if old != new => format!(
            "{status} {} -> {}",
            display_raw_path(old),
            display_raw_path(new)
        ),
        (Some(path), _) | (_, Some(path)) => format!("{status} {}", display_raw_path(path)),
        (None, None) => status,
    }
}

fn display_raw_path(path: &[u8]) -> String {
    safe_text(&String::from_utf8_lossy(path), false)
}

fn is_unborn_error(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    stderr.contains("does not have any commits yet")
        || stderr.contains("ambiguous argument 'head'")
        || stderr.contains("needed a single revision")
}

#[cfg(unix)]
fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

#[cfg(unix)]
fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(not(unix))]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(unix))]
fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(unix)]
fn literal_path(path: &[u8]) -> OsString {
    let mut value = b":(literal)".to_vec();
    value.extend_from_slice(path);
    OsString::from_vec(value)
}

#[cfg(not(unix))]
fn literal_path(path: &[u8]) -> OsString {
    OsString::from(format!(":(literal){}", String::from_utf8_lossy(path)))
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

/// Return the first Git or delta file header that belongs to `file`.
pub fn patch_offset(lines: &[String], file: &ChangedFile) -> Option<usize> {
    PatchIndex::for_changed_files(lines, std::slice::from_ref(file)).offset_for(0)
}

fn diff_header_matches_file(file: &ChangedFile, old_path: &str, new_path: &str) -> bool {
    let matches_old = file
        .old_path
        .as_deref()
        .is_some_and(|path| diff_header_matches_path(old_path, path));
    let matches_new = file
        .new_path
        .as_deref()
        .is_some_and(|path| diff_header_matches_path(new_path, path));
    match (file.old_path.as_deref(), file.new_path.as_deref()) {
        (Some(_), Some(_)) => matches_old && matches_new,
        (Some(_), None) => matches_old,
        (None, Some(_)) => matches_new,
        (None, None) => false,
    }
}

fn delta_header_matches_file(header: &str, file: &ChangedFile) -> bool {
    match (file.old_path.as_deref(), file.new_path.as_deref()) {
        (Some(old), Some(new)) if old != new => {
            header.strip_prefix("renamed: ").is_some_and(|paths| {
                paths
                    .split_once('⟶')
                    .is_some_and(|(left, right)| left.trim() == old && right.trim() == new)
            })
        }
        (None, Some(new)) => header == format!("added: {new}"),
        (Some(old), None) => header == format!("removed: {old}"),
        (Some(old), Some(new)) => old == new && header == new,
        (None, None) => false,
    }
}

pub fn status_patch_offset(lines: &[String], file: &StatusFile) -> Option<usize> {
    PatchIndex::for_status_files(lines, std::slice::from_ref(file)).offset_for(0)
}

pub fn status_file_at_patch_offset(
    lines: &[String],
    files: &[StatusFile],
    offset: usize,
) -> Option<usize> {
    PatchIndex::for_status_files(lines, files).file_at(offset)
}

/// Return the changed-file row whose patch contains `offset`.
pub fn file_at_patch_offset(
    lines: &[String],
    files: &[ChangedFile],
    offset: usize,
) -> Option<usize> {
    PatchIndex::for_changed_files(lines, files).file_at(offset)
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

    let standard = input
        .match_indices(" b/")
        .filter_map(|(index, _)| {
            let old_path = &input[..index];
            let new_path = &input[index + 1..];
            (old_path.starts_with("a/") && new_path.starts_with("b/"))
                .then(|| (sanitized_git_path(old_path), sanitized_git_path(new_path)))
        })
        .collect::<Vec<_>>();
    if !standard.is_empty() {
        return standard;
    }

    let mut tokens = git_path_tokens(input);
    match (tokens.next(), tokens.next()) {
        (Some(old_path), Some(new_path)) => {
            vec![(sanitized_git_path(&old_path), sanitized_git_path(&new_path))]
        }
        _ => Vec::new(),
    }
}

fn diff_header_matches_path(header: &str, path: &str) -> bool {
    header == path
        || header == format!("a/{path}")
        || header == format!("b/{path}")
        || header.ends_with(&format!("/{path}"))
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
                        let mut value = u16::from(escaped as u8 - b'0');
                        for _ in 0..2 {
                            let Some(digit) = input[position..].chars().next() else {
                                break;
                            };
                            if !matches!(digit, '0'..='7') {
                                break;
                            }
                            position += digit.len_utf8();
                            value = value * 8 + u16::from(digit as u8 - b'0');
                        }
                        bytes.push(u8::try_from(value).ok()?);
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

/// Retain SGR colors and remove other terminal control sequences.
pub fn safe_text(input: &str, retain_sgr: bool) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            let mut sequence = String::new();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    if next == 'm'
                        && sequence
                            .chars()
                            .all(|value| value.is_ascii_digit() || matches!(value, ';' | ':'))
                        && retain_sgr
                    {
                        output.push_str("\x1b[");
                        output.push_str(&sequence);
                        output.push('m');
                    }
                    break;
                }
                sequence.push(next);
            }
        } else if character == '\x1b' && chars.peek() == Some(&']') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '\x07' {
                    break;
                }
                if next == '\x1b' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        } else if character == '\x1b' {
            chars.next();
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
    Launch {
        directory: PathBuf,
        reason: String,
    },
    Process {
        status: Option<i32>,
        stderr: String,
    },
    Output(String),
    Show {
        stage: String,
        reason: String,
    },
    Status {
        stage: String,
        reason: String,
    },
    Filter {
        operation: String,
        command: String,
        reason: String,
    },
    Unborn,
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
            Self::Status { stage, reason } => {
                write!(formatter, "Git status {stage} failed: {reason}")
            }
            Self::Filter {
                operation,
                command,
                reason,
            } => write!(
                formatter,
                "diff filter `{command}` for {operation} failed: {reason}"
            ),
            Self::Unborn => write!(formatter, "repository has no commits yet"),
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
    #[cfg(unix)]
    fn filter_history(script: &str) -> GitHistory {
        GitHistory::new(
            ".",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(vec!["sh".into(), "-c".into(), script.into()]),
        )
    }

    #[cfg(unix)]
    #[test]
    fn filter_handles_large_bidirectional_output_without_deadlock() {
        let history = filter_history("head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; cat");
        let lines = history.presentation(vec![b'x'; 524288], "preview").unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(&"x".repeat(524288)));
        assert_eq!(lines[0].chars().count(), 262144 + 524288);
    }

    #[cfg(unix)]
    #[test]
    fn filter_preserves_sgr_and_sanitizes_controls_after_processing() {
        let history = filter_history(r"cat; printf '\033[31mred\033[m\033[2J\007'");
        let lines = history
            .presentation(b"original\n".to_vec(), "show diff")
            .unwrap();
        assert_eq!(lines, ["original", "\x1b[31mred\x1b[m�"]);
        assert_eq!(safe_text(&lines[1], false), "red�");
    }

    #[cfg(unix)]
    #[test]
    fn filter_skips_empty_input_and_prioritizes_exit_diagnostics() {
        let history = filter_history(r"printf '\033[31mfailed\033[m\007' >&2; exit 7");
        assert!(
            history
                .presentation(Vec::new(), "staged diff")
                .unwrap()
                .is_empty()
        );
        let error = history
            .presentation(vec![b'x'; 524288], "unstaged diff")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("diff filter `sh` for unstaged diff failed: exit 7: failed�"),
            "{error}"
        );
        assert!(!error.contains('\x1b'));
        let history = filter_history("exec 0<&-; sleep 0.05");
        assert!(
            history
                .presentation(vec![b'x'; 524288], "preview")
                .unwrap_err()
                .to_string()
                .contains("could not write stdin")
        );
    }

    #[test]
    fn missing_filter_reports_executable_and_operation() {
        let mut history = GitHistory::new(
            ".",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(vec!["gitlsd-missing-filter-executable".into()]),
        );
        let error = history
            .presentation(b"patch".to_vec(), "preview")
            .unwrap_err()
            .to_string();
        assert!(error.contains("gitlsd-missing-filter-executable"));
        assert!(error.contains("preview"));
        assert!(error.contains("could not launch"));
        history.diff_filter = None;
        assert_eq!(
            history
                .presentation(b"patch\n".to_vec(), "preview")
                .unwrap(),
            ["patch"]
        );
    }

    #[test]
    fn empty_filter_command_is_reported_without_panicking() {
        let history = GitHistory::new(
            ".",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(Vec::new()),
        );
        assert!(
            history
                .presentation(b"patch".to_vec(), "preview")
                .unwrap_err()
                .to_string()
                .contains("filter command is empty")
        );
    }

    #[cfg(unix)]
    #[test]
    fn filter_receives_raw_bytes_before_sanitization() {
        let history = filter_history("od -An -tx1 | tr -d ' \\n'");
        assert_eq!(
            history
                .presentation(vec![0xff, 0x1b, 0x07], "preview")
                .unwrap(),
            ["ff1b07"]
        );
    }

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
        assert_eq!(safe, "\x1b[31mred\x1b[m�");
        assert_eq!(safe_text(&safe, false), "red�");
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
    fn patch_offsets_accept_custom_diff_prefixes() {
        let files = parse_changed_files(b"M\tpath.txt\n").unwrap();
        let diff = vec!["diff --git old/path.txt new/path.txt".into()];
        assert_eq!(patch_offset(&diff, &files[0]), Some(0));
    }

    #[test]
    fn patch_offsets_match_standard_delta_file_headers() {
        let files = parse_changed_files(
            b"M\tmodified.txt\nA\tadded.txt\nD\tdeleted.txt\nR100\told name.txt\tnew name.txt\n",
        )
        .unwrap();
        let diff = vec![
            "modified.txt".into(),
            "────".into(),
            "added: added.txt".into(),
            "────".into(),
            "removed: deleted.txt".into(),
            "────".into(),
            "renamed: old name.txt ⟶   new name.txt".into(),
            "────".into(),
        ];
        assert_eq!(patch_offset(&diff, &files[0]), Some(0));
        assert_eq!(patch_offset(&diff, &files[1]), Some(2));
        assert_eq!(patch_offset(&diff, &files[2]), Some(4));
        assert_eq!(patch_offset(&diff, &files[3]), Some(6));
        let index = PatchIndex::for_changed_files(&diff, &files);
        assert_eq!(index.file_at(1), Some(0));
        assert_eq!(index.file_at(3), Some(1));
        assert_eq!(index.file_at(5), Some(2));
        assert_eq!(index.file_at(7), Some(3));
    }

    #[test]
    fn patch_index_keeps_suffix_compatible_file_rows_in_source_order() {
        let files = parse_changed_files(b"M\tfoo\nM\tz/foo\n").unwrap();
        let diff = vec!["diff --git a/z/foo b/z/foo".into()];
        let index = PatchIndex::for_changed_files(&diff, &files);
        assert_eq!(index.offset_for(0), Some(0));
        assert_eq!(index.offset_for(1), Some(0));
        assert_eq!(index.file_at(0), Some(1));
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
    fn patch_index_keeps_first_direct_match_and_binary_reverse_lookup() {
        let files = parse_changed_files(b"M\tone.rs\nM\ttwo.rs\n").unwrap();
        let mut diff = vec!["diff --git a/one.rs b/one.rs".into()];
        diff.extend((0..70_000).map(|index| format!("one line {index}")));
        diff.push("diff --git a/two.rs b/two.rs".into());
        diff.push("diff --git a/one.rs b/one.rs".into());

        let index = PatchIndex::for_changed_files(&diff, &files);
        assert_eq!(index.offset_for(0), Some(0));
        assert_eq!(index.offset_for(1), Some(70_001));
        assert_eq!(index.file_at(70_000), Some(0));
        assert_eq!(index.file_at(70_001), Some(1));
        assert_eq!(index.file_at(70_002), Some(0));
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

    #[test]
    fn parses_porcelain_status_into_independent_groups_and_raw_paths() {
        let (staged, unstaged) = parse_status(
            b"MM partial.txt\0R  renamed target.txt\0rename source.txt\0?? literal[*].txt\0",
        )
        .unwrap();
        assert_eq!(staged.len(), 2);
        assert_eq!(unstaged.len(), 2);
        assert_eq!(staged[0].display, "MM partial.txt");
        assert_eq!(unstaged[0].display, "MM partial.txt");
        assert_eq!(
            staged[1].display,
            "R  rename source.txt -> renamed target.txt"
        );
        assert_eq!(
            staged[1].old_path.as_deref(),
            Some(b"rename source.txt".as_slice())
        );
        assert_eq!(
            staged[1].new_path.as_deref(),
            Some(b"renamed target.txt".as_slice())
        );
        assert_eq!(
            unstaged[1].new_path.as_deref(),
            Some(b"literal[*].txt".as_slice())
        );
    }

    #[test]
    fn staged_status_diff_flag_precedes_configured_path_arguments() {
        let history = GitHistory::new(
            ".",
            vec!["git".into(), "log".into()],
            vec!["git".into(), "show".into()],
            vec!["git".into(), "show".into()],
            vec!["git".into(), "show".into()],
            vec![
                "git".into(),
                "diff".into(),
                "--word-diff".into(),
                "--".into(),
                "*.rs".into(),
            ],
            None,
        );
        assert_eq!(
            history.status_diff_arguments(true),
            ["diff", "--staged", "--word-diff", "--", "*.rs"]
        );
        assert_eq!(
            history.status_diff_arguments(false),
            ["diff", "--word-diff", "--", "*.rs"]
        );
    }

    #[test]
    fn untracked_diff_uses_configured_presentation_options_and_exact_path() {
        let history = GitHistory::new(
            ".",
            vec!["git".into(), "log".into()],
            vec!["git".into(), "show".into()],
            vec!["git".into(), "show".into()],
            vec!["git".into(), "show".into()],
            vec![
                "git".into(),
                "diff".into(),
                "--word-diff".into(),
                "--".into(),
                "*.rs".into(),
            ],
            None,
        );
        assert_eq!(
            history.untracked_diff_arguments(b"literal[*].txt"),
            [
                OsString::from("diff"),
                OsString::from("--word-diff"),
                OsString::from("--no-index"),
                OsString::from("--"),
                OsString::from(null_device()),
                OsString::from("literal[*].txt"),
            ]
        );
    }
}
