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
    has_more: bool,
    next_offset: usize,
    last_search: Option<String>,
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
            has_more: true,
            next_offset: 0,
            last_search: None,
        }
    }

    pub fn initialize(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        self.fetch_more(source)?;
        if self.records.is_empty() {
            self.status = "No commits found".into();
        } else {
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
                    return;
                }
                if let Some(action) = self.config.action_for(&key) {
                    self.dispatch(action, source);
                }
            }
        }
    }

    pub fn dispatch(&mut self, action: Action, source: &mut impl HistorySource) {
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
            Action::MoveUp => self.move_up(),
            Action::PageDown => {
                for _ in 0..10 {
                    let before = self.selected;
                    self.move_down(source);
                    if self.selected == before {
                        break;
                    }
                }
            }
            Action::PageUp => self.selected = self.selected.saturating_sub(10),
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
        }
        if !fetched_to_move && self.selected + 1 == self.records.len() && self.has_more {
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not load more history: {error}");
            }
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
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
                self.status = format!("Match for `{query}`");
            }
            None => self.status = format!("No match for `{query}`"),
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
    record.subject.to_lowercase().contains(query)
        || record.author.to_lowercase().contains(query)
        || record.id.to_lowercase().contains(query)
        || record.short_id.to_lowercase().contains(query)
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
                short_id: format!("{index:07}"),
                author: "Author".into(),
                date: "2026-09-05".into(),
                subject: format!("Subject {index}"),
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
}
