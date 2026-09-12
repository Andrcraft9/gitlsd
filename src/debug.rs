//! Deterministic frontend for integration testing.
//!
//! Script tokens become the same keys used by the interactive frontend and
//! drive the same [`App`]. The resulting stable, plain-text snapshot avoids a
//! TTY while preserving the real configuration and Git process boundaries,
//! including show metadata, file rows, and diff state.

use std::fmt::Write;

use crate::app::{App, Screen, ShowFocus};
use crate::config::Key;
use crate::git::HistorySource;

const HORIZONTAL_VIEWPORT_WIDTH: usize = 80;

pub fn run_script(
    app: &mut App,
    source: &mut impl HistorySource,
    script: &str,
) -> Result<String, String> {
    app.set_horizontal_viewport_width(HORIZONTAL_VIEWPORT_WIDTH);
    if !script.is_empty() {
        for (index, token) in script.split(';').enumerate() {
            if token.is_empty() {
                return Err(format!("script key {} is empty", index + 1));
            }
            let key: Key = token
                .parse()
                .map_err(|reason| format!("script key {}: {reason}", index + 1))?;
            app.handle_key(key, source);
            if !app.is_running() {
                break;
            }
        }
    }
    Ok(snapshot(app))
}

pub fn snapshot(app: &App) -> String {
    let mut output = String::new();
    let screen = match app.screen() {
        Screen::Log => "log",
        Screen::Help(_) => "help",
        Screen::Show(_) => "show",
    };
    writeln!(output, "screen={screen}").unwrap();
    let log = app.log_state();
    writeln!(output, "running={}", app.is_running()).unwrap();
    writeln!(output, "loaded={}", log.records().len()).unwrap();
    writeln!(output, "selected={}", log.selected()).unwrap();
    writeln!(output, "preview.visible={}", log.preview_visible()).unwrap();
    writeln!(output, "preview.focused={}", log.preview_focused()).unwrap();
    writeln!(output, "preview.offset={}", log.preview_offset()).unwrap();
    writeln!(
        output,
        "log.horizontal-offset={}",
        log.log_horizontal_offset()
    )
    .unwrap();
    writeln!(
        output,
        "preview.horizontal-offset={}",
        log.preview_horizontal_offset()
    )
    .unwrap();
    writeln!(
        output,
        "help.horizontal-offset={}",
        match app.screen() {
            Screen::Help(help) => help.horizontal_offset(),
            _ => 0,
        }
    )
    .unwrap();
    if let Screen::Show(show) = app.screen() {
        let focus = match show.focus() {
            ShowFocus::Explorer => "explorer",
            ShowFocus::Diff => "diff",
        };
        writeln!(output, "show.focus={focus}").unwrap();
        writeln!(
            output,
            "show.selected={}",
            show.selected()
                .map_or_else(String::new, |selected| selected.to_string())
        )
        .unwrap();
        writeln!(output, "show.explorer.offset={}", show.explorer_offset()).unwrap();
        writeln!(
            output,
            "show.explorer.horizontal-offset={}",
            show.explorer_horizontal_offset()
        )
        .unwrap();
        writeln!(output, "show.diff.offset={}", show.diff_offset()).unwrap();
        writeln!(
            output,
            "show.diff.horizontal-offset={}",
            show.diff_horizontal_offset()
        )
        .unwrap();
    }
    if let Some(record) = app.selected_record() {
        writeln!(output, "commit={}", record.id).unwrap();
        writeln!(
            output,
            "display={}",
            crate::git::safe_text(&record.display, false)
        )
        .unwrap();
    } else {
        writeln!(output, "commit=").unwrap();
        writeln!(output, "display=").unwrap();
    }
    writeln!(output, "status={}", app.status().replace('\n', " ")).unwrap();
    if matches!(app.screen(), Screen::Help(_)) {
        writeln!(output, "help:").unwrap();
        for line in app.help_lines() {
            writeln!(output, "  {line}").unwrap();
        }
    } else if let Screen::Show(show) = app.screen() {
        writeln!(output, "show.metadata:").unwrap();
        for line in show.metadata() {
            writeln!(output, "  {}", crate::git::safe_text(line, false)).unwrap();
        }
        writeln!(output, "show.files:").unwrap();
        for (index, file) in show.files().iter().enumerate() {
            let marker = if show.selected() == Some(index) {
                '>'
            } else {
                ' '
            };
            writeln!(
                output,
                "{marker} {index} {}",
                crate::git::safe_text(&file.display, false)
            )
            .unwrap();
        }
        writeln!(output, "show.diff:").unwrap();
        for line in show.diff_lines() {
            writeln!(output, "  {}", crate::git::safe_text(line, false)).unwrap();
        }
    } else {
        if log.preview_visible() {
            writeln!(output, "preview:").unwrap();
            for line in log.preview_lines() {
                writeln!(output, "  {}", crate::git::safe_text(line, false)).unwrap();
            }
        }
        writeln!(output, "rows:").unwrap();
        for (index, record) in log.records().iter().enumerate() {
            let marker = if index == log.selected() { '>' } else { ' ' };
            writeln!(
                output,
                "{marker} {index} {}",
                crate::git::safe_text(&record.display, false)
            )
            .unwrap();
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::Config;
    use crate::git::{CommitRecord, GitError, HistorySource};

    use super::*;

    struct Empty;

    impl HistorySource for Empty {
        fn load(&mut self, _offset: usize, _limit: usize) -> Result<Vec<CommitRecord>, GitError> {
            Ok(Vec::new())
        }
    }

    struct Fixture {
        records: Vec<CommitRecord>,
        preview: Vec<String>,
    }

    impl HistorySource for Fixture {
        fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError> {
            Ok(self
                .records
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect())
        }

        fn load_preview(&mut self, _id: &str) -> Result<Vec<String>, GitError> {
            Ok(self.preview.clone())
        }
    }

    #[test]
    fn invalid_script_key_is_actionable() {
        let mut app = App::new(Config::default());
        let error = run_script(&mut app, &mut Empty, "not-a-key").unwrap_err();
        assert!(error.contains("script key 1"));
        assert!(error.contains("unknown key"));
    }

    #[test]
    fn named_semicolon_key_is_scriptable() {
        let config = Config::parse("bind semicolon quit\n", Path::new("debug.conf")).unwrap();
        let mut app = App::new(config);
        let snapshot = run_script(&mut app, &mut Empty, "semicolon").unwrap();
        assert!(snapshot.contains("running=false\n"));
    }

    #[test]
    fn preview_snapshot_strips_unsafe_controls_and_sgr() {
        let mut app = App::new(Config::default());
        let mut history = Fixture {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "row".into(),
            }],
            preview: vec!["\x1b[31mred\x1b[m\x1b[2J".into()],
        };
        app.initialize(&mut history).unwrap();
        assert_eq!(
            crate::git::safe_text(&app.log_state().preview_lines()[0], true),
            "\x1b[31mred\x1b[m�[2J"
        );
        let output = snapshot(&app);
        assert!(output.contains("red�[2J"));
        assert!(!output.contains('\x1b'));
    }

    #[test]
    fn scripted_horizontal_navigation_exposes_view_offsets() {
        let mut app = App::new(Config::default());
        let mut history = Fixture {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "x".repeat(200),
            }],
            preview: Vec::new(),
        };
        app.initialize(&mut history).unwrap();
        let output = run_script(&mut app, &mut history, "right;right;left").unwrap();
        assert!(output.contains("log.horizontal-offset=40\n"));
        assert!(output.contains("preview.horizontal-offset=0\n"));
        assert!(output.contains("help.horizontal-offset=0\n"));
    }

    #[test]
    fn scripted_home_and_end_jump_horizontal_offsets() {
        let mut app = App::new(Config::default());
        let mut history = Fixture {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "x".repeat(200),
            }],
            preview: Vec::new(),
        };
        app.initialize(&mut history).unwrap();
        let end = run_script(&mut app, &mut history, "end").unwrap();
        assert!(end.contains("log.horizontal-offset=120\n"));
        let start = run_script(&mut app, &mut history, "home").unwrap();
        assert!(start.contains("log.horizontal-offset=0\n"));
    }
}
