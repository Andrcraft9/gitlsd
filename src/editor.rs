//! Configured editor command preparation and process launching.
//!
//! Commands are executed directly in the repository root. The `file` and
//! `line` placeholders may be standalone arguments or parts of an argument,
//! such as `file:line`; a missing diff line is represented as line one.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorRequest {
    directory: PathBuf,
    command: Vec<OsString>,
}

impl EditorRequest {
    pub fn new(command: &[String], directory: PathBuf, file: &Path, line: Option<usize>) -> Self {
        let line = line.unwrap_or(1).to_string();
        let command = command
            .iter()
            .map(|argument| expand_argument(argument, file.as_os_str(), &line))
            .collect();
        Self { directory, command }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn command(&self) -> &[OsString] {
        &self.command
    }

    pub fn run(&self) -> Result<(), String> {
        let Some((executable, arguments)) = self.command.split_first() else {
            return Err("editor command is empty".into());
        };
        let status = Command::new(executable)
            .args(arguments)
            .current_dir(&self.directory)
            .status()
            .map_err(|error| format!("could not launch editor: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "editor exited with {}",
                status
                    .code()
                    .map_or_else(|| "a signal".into(), |code| format!("status {code}"))
            ))
        }
    }
}

fn expand_argument(argument: &str, file: &OsStr, line: &str) -> OsString {
    match argument {
        "file" => file.to_owned(),
        "line" => line.into(),
        "+line" => format!("+{line}").into(),
        "file:line" => {
            let mut expanded = file.to_owned();
            expanded.push(":");
            expanded.push(line);
            expanded
        }
        _ => {
            let mut expanded = OsString::new();
            let mut remainder = argument;
            while let Some(index) = remainder.find(['{', '}']) {
                expanded.push(&remainder[..index]);
                remainder = &remainder[index..];
                if let Some(rest) = remainder.strip_prefix("{file}") {
                    expanded.push(file);
                    remainder = rest;
                } else if let Some(rest) = remainder.strip_prefix("{line}") {
                    expanded.push(line);
                    remainder = rest;
                } else {
                    expanded.push(&remainder[..1]);
                    remainder = &remainder[1..];
                }
            }
            expanded.push(remainder);
            expanded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_micro_and_vscode_placeholder_forms() {
        let root = PathBuf::from("/repo");
        let micro = EditorRequest::new(
            &["micro".into(), "+line".into(), "file".into()],
            root.clone(),
            Path::new("src/main.rs"),
            Some(42),
        );
        assert_eq!(micro.command, ["micro", "+42", "src/main.rs"]);

        let vscode = EditorRequest::new(
            &[
                "code".into(),
                "-g".into(),
                "--goto".into(),
                "file:line".into(),
            ],
            root,
            Path::new("src/main.rs"),
            None,
        );
        assert_eq!(vscode.command, ["code", "-g", "--goto", "src/main.rs:1"]);
    }

    #[test]
    fn expansion_does_not_rewrite_inserted_paths_or_ordinary_arguments() {
        let request = EditorRequest::new(
            &[
                "editor".into(),
                "--profile".into(),
                "file:line".into(),
                "--target={file}:{line}".into(),
            ],
            "/repo".into(),
            Path::new("src/online.rs"),
            Some(42),
        );
        assert_eq!(
            request.command,
            [
                "editor",
                "--profile",
                "src/online.rs:42",
                "--target=src/online.rs:42"
            ]
        );
    }

    #[test]
    fn launch_failure_is_actionable() {
        let request = EditorRequest {
            directory: ".".into(),
            command: vec!["gitlsd-missing-editor-executable".into()],
        };
        assert!(
            request
                .run()
                .unwrap_err()
                .contains("could not launch editor")
        );
    }

    #[cfg(unix)]
    #[test]
    fn combined_placeholder_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let file = PathBuf::from(OsString::from_vec(b"non-utf8-\xff.rs".to_vec()));
        let request = EditorRequest::new(
            &["code".into(), "file:line".into()],
            "/repo".into(),
            &file,
            Some(7),
        );
        assert_eq!(
            request.command[1].as_os_str().as_bytes(),
            b"non-utf8-\xff.rs:7"
        );
    }
}
