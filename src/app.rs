use std::collections::HashSet;

use crate::config::{Action, Config, Key};
use crate::git::{CommitRecord, GitError, HistorySource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Screen {
    Log,
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Search(String),
    Command(String),
}

pub struct App {
    pub config: Config,
    pub records: Vec<CommitRecord>,
    pub selected: usize,
    pub screen: Screen,
    pub input: InputMode,
    pub status: String,
    pub running: bool,
    pub help_offset: usize,
    pub preview_visible: bool,
    pub preview_focused: bool,
    pub preview_lines: Vec<String>,
    pub preview_offset: usize,
    has_more: bool,
    next_offset: usize,
    last_search: Option<String>,
    last_preview_search: Option<String>,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            records: Vec::new(),
            selected: 0,
            screen: Screen::Log,
            input: InputMode::Normal,
            status: String::new(),
            running: true,
            help_offset: 0,
            preview_visible: true,
            preview_focused: false,
            preview_lines: Vec::new(),
            preview_offset: 0,
            has_more: true,
            next_offset: 0,
            last_search: None,
            last_preview_search: None,
        }
    }

    pub fn initialize(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        self.fetch_more(source)?;
        self.reload_preview(source);
        if self.records.is_empty() {
            self.status = "No commits found".into();
        } else if self.status.is_empty() {
            self.status = format!("Loaded {} commits", self.records.len());
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key, source: &mut impl HistorySource) {
        if matches!(key, Key::Ctrl(_)) && self.config.action_for(&key) == Some(Action::Quit) {
            self.dispatch(Action::Quit, source);
            return;
        }
        match &mut self.input {
            InputMode::Search(buffer) => match key {
                Key::Enter => {
                    let query = std::mem::take(buffer);
                    self.input = InputMode::Normal;
                    self.submit_search(query, true, source);
                }
                Key::Escape => self.input = InputMode::Normal,
                Key::Backspace => {
                    buffer.pop();
                }
                Key::Char(character) => buffer.push(character),
                _ => {}
            },
            InputMode::Command(buffer) => match key {
                Key::Enter => {
                    let command = std::mem::take(buffer);
                    self.input = InputMode::Normal;
                    self.submit_command(&command);
                }
                Key::Escape => self.input = InputMode::Normal,
                Key::Backspace => {
                    buffer.pop();
                }
                Key::Char(character) => buffer.push(character),
                _ => {}
            },
            InputMode::Normal => {
                if key == Key::Enter {
                    if self.screen == Screen::Log
                        && self.preview_visible
                        && self.selected_record().is_some()
                    {
                        self.preview_focused = true;
                    }
                    return;
                }
                if let Some(action) = self.config.action_for(&key) {
                    self.dispatch(action, source);
                }
            }
        }
    }

    pub fn dispatch(&mut self, action: Action, source: &mut impl HistorySource) {
        if self.preview_focused && self.screen == Screen::Log {
            match action {
                Action::MoveDown => {
                    self.preview_offset =
                        (self.preview_offset + 1).min(self.preview_lines.len().saturating_sub(1))
                }
                Action::MoveUp => self.preview_offset = self.preview_offset.saturating_sub(1),
                Action::PageDown => {
                    self.preview_offset =
                        (self.preview_offset + 10).min(self.preview_lines.len().saturating_sub(1))
                }
                Action::PageUp => self.preview_offset = self.preview_offset.saturating_sub(10),
                Action::Search => {
                    self.input = InputMode::Search(String::new());
                }
                Action::SearchNext => self.repeat_preview_search(true),
                Action::SearchPrevious => self.repeat_preview_search(false),
                Action::Back => {
                    self.preview_focused = false;
                    self.input = InputMode::Normal;
                }
                Action::Quit | Action::TogglePreview => {}
                _ => {}
            }
            if matches!(action, Action::Quit | Action::TogglePreview) {
                // These remain global even while preview owns navigation.
            } else {
                return;
            }
        }
        if self.screen == Screen::Help {
            let last_line = self.config.help_lines().len().saturating_sub(1);
            match action {
                Action::MoveDown => self.help_offset = (self.help_offset + 1).min(last_line),
                Action::MoveUp => self.help_offset = self.help_offset.saturating_sub(1),
                Action::PageDown => self.help_offset = (self.help_offset + 10).min(last_line),
                Action::PageUp => self.help_offset = self.help_offset.saturating_sub(10),
                _ => {}
            }
            if matches!(
                action,
                Action::MoveDown | Action::MoveUp | Action::PageDown | Action::PageUp
            ) {
                return;
            }
        }
        match action {
            Action::MoveDown => self.move_down(source),
            Action::MoveUp => self.move_up(source),
            Action::PageDown => {
                let preview_visible = self.preview_visible;
                self.preview_visible = false;
                for _ in 0..10 {
                    let before = self.selected;
                    self.move_down(source);
                    if self.selected == before {
                        break;
                    }
                }
                self.preview_visible = preview_visible;
                if preview_visible {
                    self.reload_preview(source);
                }
            }
            Action::PageUp => {
                let before = self.selected;
                self.selected = self.selected.saturating_sub(10);
                if self.selected != before {
                    self.reload_preview(source);
                }
            }
            Action::Search => {
                self.screen = Screen::Log;
                self.input = InputMode::Search(String::new());
            }
            Action::SearchNext => self.repeat_search(true, source),
            Action::SearchPrevious => self.repeat_search(false, source),
            Action::Command => self.input = InputMode::Command(String::new()),
            Action::Help => {
                self.screen = Screen::Help;
                self.help_offset = 0;
                self.status = "Showing effective configuration".into();
            }
            Action::Back => {
                self.screen = Screen::Log;
                self.input = InputMode::Normal;
            }
            Action::TogglePreview => {
                self.preview_visible = !self.preview_visible;
                self.preview_focused = false;
                if self.preview_visible {
                    self.reload_preview(source);
                }
            }
            Action::Quit => {
                self.running = false;
                self.status = "Quit requested".into();
            }
        }
    }

    fn move_down(&mut self, source: &mut impl HistorySource) {
        let mut fetched_to_move = false;
        if self.selected + 1 >= self.records.len() && self.has_more {
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not load more history: {error}");
                return;
            }
            fetched_to_move = true;
        }
        if self.selected + 1 < self.records.len() {
            self.selected += 1;
            self.reload_preview(source);
        }
        if !fetched_to_move && self.selected + 1 == self.records.len() && self.has_more {
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not load more history: {error}");
            }
        }
    }

    fn move_up(&mut self, source: &mut impl HistorySource) {
        let before = self.selected;
        self.selected = self.selected.saturating_sub(1);
        if self.selected != before {
            self.reload_preview(source);
        }
    }

    fn fetch_more(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        let batch = source.load(self.next_offset, self.config.batch_size)?;
        self.next_offset += batch.len();
        self.has_more = batch.len() == self.config.batch_size;
        if batch.is_empty() {
            self.has_more = false;
            return Ok(());
        }
        let mut known: HashSet<String> = self
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect();
        self.records.extend(
            batch
                .into_iter()
                .filter(|record| known.insert(record.id.clone())),
        );
        Ok(())
    }

    fn submit_search(&mut self, query: String, forward: bool, source: &mut impl HistorySource) {
        if query.is_empty() {
            self.status = "Search query is empty".into();
            return;
        }
        if self.preview_focused {
            self.last_preview_search = Some(query);
            self.repeat_preview_search(forward);
            return;
        }
        self.last_search = Some(query);
        self.repeat_search(forward, source);
    }

    fn repeat_search(&mut self, forward: bool, source: &mut impl HistorySource) {
        let Some(query) = self.last_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        if self.records.is_empty() {
            self.status = format!("No match for `{query}`");
            return;
        }
        let query_lower = query.to_lowercase();
        let mut found = if forward {
            (self.selected + 1..self.records.len())
                .find(|index| matches_record(&self.records[*index], &query_lower))
        } else {
            (0..self.selected)
                .rev()
                .find(|index| matches_record(&self.records[*index], &query_lower))
        };

        while found.is_none() && self.has_more {
            let previous_length = self.records.len();
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not continue search: {error}");
                return;
            }
            if forward {
                found = (previous_length..self.records.len())
                    .find(|index| matches_record(&self.records[*index], &query_lower));
            }
        }

        if found.is_none() {
            found = if forward {
                (0..=self.selected)
                    .find(|index| matches_record(&self.records[*index], &query_lower))
            } else {
                (self.selected..self.records.len())
                    .rev()
                    .find(|index| matches_record(&self.records[*index], &query_lower))
            };
        }
        match found {
            Some(index) => {
                self.selected = index;
                self.reload_preview(source);
                self.status = format!("Match for `{query}`");
            }
            None => self.status = format!("No match for `{query}`"),
        }
    }

    fn reload_preview(&mut self, source: &mut impl HistorySource) {
        if !self.preview_visible {
            return;
        }
        self.preview_offset = 0;
        self.preview_lines.clear();
        let Some(id) = self.selected_record().map(|record| record.id.clone()) else {
            return;
        };
        match source.load_preview(&id) {
            Ok(lines) => self.preview_lines = lines,
            Err(error) => self.status = format!("Could not load preview: {error}"),
        }
    }

    fn repeat_preview_search(&mut self, forward: bool) {
        let Some(query) = self.last_preview_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        let query_lower = query.to_lowercase();
        let length = self.preview_lines.len();
        if length == 0 {
            self.status = format!("No match for `{query}`");
            return;
        }
        let mut indices: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(
                (self.preview_offset + 1..length).chain(0..=self.preview_offset.min(length - 1)),
            )
        } else {
            Box::new(
                (0..self.preview_offset)
                    .rev()
                    .chain((self.preview_offset..length).rev()),
            )
        };
        if let Some(index) = indices.find(|&index| {
            crate::git::safe_text(&self.preview_lines[index], false)
                .to_lowercase()
                .contains(&query_lower)
        }) {
            self.preview_offset = index;
            self.status = format!("Match for `{query}`");
        } else {
            self.status = format!("No match for `{query}`");
        }
    }

    fn submit_command(&mut self, command: &str) {
        match command.trim() {
            "help" | "h" => {
                self.screen = Screen::Help;
                self.help_offset = 0;
                self.status = "Showing effective configuration".into();
            }
            "quit" | "q" => {
                self.running = false;
                self.status = "Quit requested".into();
            }
            "" => self.status = "Command is empty".into(),
            unknown => self.status = format!("Unknown command: {unknown}"),
        }
    }

    pub fn selected_record(&self) -> Option<&CommitRecord> {
        self.records.get(self.selected)
    }

    pub fn input_label(&self) -> Option<String> {
        match &self.input {
            InputMode::Normal => None,
            InputMode::Search(value) => Some(format!("/{value}")),
            InputMode::Command(value) => Some(format!(":{value}")),
        }
    }
}

fn matches_record(record: &CommitRecord, query: &str) -> bool {
    crate::git::safe_text(&record.display, false)
        .to_lowercase()
        .contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHistory {
        records: Vec<CommitRecord>,
        fail_at: Option<usize>,
    }

    impl HistorySource for FakeHistory {
        fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError> {
            if self.fail_at == Some(offset) {
                return Err(GitError::Output("planned failure".into()));
            }
            Ok(self
                .records
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect())
        }
    }

    fn records(count: usize) -> Vec<CommitRecord> {
        (0..count)
            .map(|index| CommitRecord {
                id: format!("id-{index}"),
                display: format!("Subject {index}"),
            })
            .collect()
    }

    #[test]
    fn reaching_tail_loads_distinct_next_batch() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(5),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.records.len(), 4);
        assert_eq!(
            app.records
                .iter()
                .map(|record| &record.id)
                .collect::<HashSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn batch_of_one_can_move_on_first_down_action() {
        let config = Config {
            batch_size: 1,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(3),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.selected, 1);
        assert_eq!(app.records.len(), 2);
    }

    #[test]
    fn failed_next_batch_preserves_loaded_rows() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(5),
            fail_at: Some(2),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.records.len(), 2);
        assert!(app.status.contains("planned failure"));
    }

    #[test]
    fn empty_next_batch_marks_end_of_history() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(2),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.records.len(), 2);
        assert!(!app.has_more);
    }

    #[test]
    fn no_match_keeps_selection_and_unknown_command_reports_status() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(3),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.submit_search(String::new(), true, &mut history);
        assert_eq!(app.selected, 0);
        assert_eq!(app.status, "Search query is empty");
        app.submit_search("missing".into(), true, &mut history);
        assert_eq!(app.selected, 0);
        assert_eq!(app.status, "No match for `missing`");
        app.submit_command("wat");
        assert_eq!(app.status, "Unknown command: wat");
        assert!(app.running);
    }

    #[test]
    fn enter_on_log_row_does_nothing() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        let status = app.status.clone();
        app.handle_key(Key::Enter, &mut history);
        assert_eq!(app.status, status);
        assert!(app.running);
    }

    #[test]
    fn control_quit_binding_wins_during_text_entry() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.input = InputMode::Search("query".into());
        app.handle_key(Key::Ctrl('c'), &mut history);
        assert!(!app.running);

        let mut app = App::new(Config::default());
        app.input = InputMode::Command("help".into());
        app.handle_key(Key::Ctrl('c'), &mut history);
        assert!(!app.running);
    }

    #[test]
    fn help_navigation_scrolls_without_changing_log_selection() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(3),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.selected = 1;
        app.dispatch(Action::Help, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.help_offset, 1);
        assert_eq!(app.selected, 1);
        app.dispatch(Action::PageDown, &mut history);
        assert!(app.help_offset > 1);
        assert_eq!(app.selected, 1);
        app.dispatch(Action::PageUp, &mut history);
        assert_eq!(app.help_offset, 1);
    }

    #[test]
    fn overlapping_pages_do_not_duplicate_commits() {
        struct OverlappingHistory;

        impl HistorySource for OverlappingHistory {
            fn load(
                &mut self,
                offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                let all = records(3);
                Ok(match offset {
                    0 => all[..2].to_vec(),
                    2 => all[1..].to_vec(),
                    _ => Vec::new(),
                })
            }
        }

        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = OverlappingHistory;
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(
            app.records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            ["id-0", "id-1", "id-2"]
        );
    }

    #[test]
    fn preview_focus_scrolling_search_and_escape_do_not_move_log_selection() {
        struct PreviewHistory;

        impl HistorySource for PreviewHistory {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(records(2))
            }

            fn load_preview(&mut self, id: &str) -> Result<Vec<String>, GitError> {
                Ok(vec![
                    format!("{id} first"),
                    "second needle".into(),
                    "third".into(),
                ])
            }
        }

        let mut app = App::new(Config::default());
        let mut history = PreviewHistory;
        app.initialize(&mut history).unwrap();
        assert!(app.preview_visible);
        assert_eq!(app.preview_lines[0], "id-0 first");

        app.handle_key(Key::Enter, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert!(app.preview_focused);
        assert_eq!(app.preview_offset, 1);
        assert_eq!(app.selected, 0);

        app.handle_key(Key::Char('/'), &mut history);
        for key in "needle".chars().map(Key::Char).chain([Key::Enter]) {
            app.handle_key(key, &mut history);
        }
        assert_eq!(app.preview_offset, 1);
        app.dispatch(Action::Back, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert!(!app.preview_focused);
        assert_eq!(app.selected, 1);
        assert_eq!(app.preview_offset, 0);
        assert_eq!(app.preview_lines[0], "id-1 first");
    }

    #[test]
    fn preview_failure_keeps_log_usable_and_reports_status() {
        struct FailingPreview;

        impl HistorySource for FailingPreview {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(records(2))
            }

            fn load_preview(&mut self, _id: &str) -> Result<Vec<String>, GitError> {
                Err(GitError::Output("planned preview failure".into()))
            }
        }

        let mut app = App::new(Config::default());
        let mut history = FailingPreview;
        app.initialize(&mut history).unwrap();
        assert!(app.status.contains("planned preview failure"));
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn preview_focus_keeps_quit_and_toggle_global_and_does_not_escape_help() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.handle_key(Key::Enter, &mut history);
        app.dispatch(Action::TogglePreview, &mut history);
        assert!(!app.preview_visible);
        assert!(!app.preview_focused);
        app.preview_visible = true;
        app.screen = Screen::Help;
        app.handle_key(Key::Enter, &mut history);
        assert!(!app.preview_focused);
        app.screen = Screen::Log;
        app.preview_focused = true;
        app.dispatch(Action::Quit, &mut history);
        assert!(!app.running);
    }
}
