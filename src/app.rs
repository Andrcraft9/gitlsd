//! Application state machine and behavior.
//!
//! [`App`] owns persistent log state plus enum-scoped help and show state,
//! input modes, commands, and shutdown intent. It works through
//! [`HistorySource`] and has no dependency on terminal libraries or concrete
//! process execution. Frontends drive it through intent methods and consume
//! read-only state accessors.

use std::collections::HashSet;

use crate::config::{Action, Config, Key};
use crate::git::{
    ChangedFile, CommitRecord, GitError, HistorySource, file_at_patch_offset, patch_offset,
};
use unicode_width::UnicodeWidthStr;

/// The active screen and the state that exists only while that screen is open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Screen {
    Log,
    Help(HelpState),
    Show(ShowState),
}

impl Screen {
    pub fn help(&self) -> Option<&HelpState> {
        match self {
            Self::Help(state) => Some(state),
            _ => None,
        }
    }

    fn help_mut(&mut self) -> Option<&mut HelpState> {
        match self {
            Self::Help(state) => Some(state),
            _ => None,
        }
    }

    pub fn show(&self) -> Option<&ShowState> {
        match self {
            Self::Show(state) => Some(state),
            _ => None,
        }
    }

    fn show_mut(&mut self) -> Option<&mut ShowState> {
        match self {
            Self::Show(state) => Some(state),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShowFocus {
    Explorer,
    Diff,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InputMode {
    Normal,
    Search(String),
    Command(String),
}

/// Persistent log and preview state retained across screen transitions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogState {
    records: Vec<CommitRecord>,
    selected: usize,
    log_horizontal_offset: usize,
    preview_visible: bool,
    preview_focused: bool,
    preview_lines: Vec<String>,
    preview_offset: usize,
    preview_horizontal_offset: usize,
    has_more: bool,
    next_offset: usize,
    last_search: Option<String>,
    last_preview_search: Option<String>,
}

/// Scroll state that exists only while help is active.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HelpState {
    offset: usize,
    horizontal_offset: usize,
}

/// Explorer, diff, and search state that exists only while show is active.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShowState {
    show_focus: ShowFocus,
    show_metadata: Vec<String>,
    show_files: Vec<ChangedFile>,
    show_diff_lines: Vec<String>,
    show_selected: Option<usize>,
    show_explorer_offset: usize,
    show_explorer_horizontal_offset: usize,
    show_diff_offset: usize,
    show_diff_horizontal_offset: usize,
    last_show_search: Option<String>,
}

impl Default for ShowState {
    fn default() -> Self {
        Self {
            show_focus: ShowFocus::Explorer,
            show_metadata: Vec::new(),
            show_files: Vec::new(),
            show_diff_lines: Vec::new(),
            show_selected: None,
            show_explorer_offset: 0,
            show_explorer_horizontal_offset: 0,
            show_diff_offset: 0,
            show_diff_horizontal_offset: 0,
            last_show_search: None,
        }
    }
}

impl LogState {
    pub fn records(&self) -> &[CommitRecord] {
        &self.records
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn log_horizontal_offset(&self) -> usize {
        self.log_horizontal_offset
    }

    pub fn preview_visible(&self) -> bool {
        self.preview_visible
    }

    pub fn preview_focused(&self) -> bool {
        self.preview_focused
    }

    pub fn preview_lines(&self) -> &[String] {
        &self.preview_lines
    }

    pub fn preview_offset(&self) -> usize {
        self.preview_offset
    }

    pub fn preview_horizontal_offset(&self) -> usize {
        self.preview_horizontal_offset
    }
}

impl HelpState {
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn horizontal_offset(&self) -> usize {
        self.horizontal_offset
    }
}

impl ShowState {
    pub fn focus(&self) -> ShowFocus {
        self.show_focus
    }

    pub fn metadata(&self) -> &[String] {
        &self.show_metadata
    }

    pub fn files(&self) -> &[ChangedFile] {
        &self.show_files
    }

    pub fn diff_lines(&self) -> &[String] {
        &self.show_diff_lines
    }

    pub fn selected(&self) -> Option<usize> {
        self.show_selected
    }

    pub fn explorer_offset(&self) -> usize {
        self.show_explorer_offset
    }

    pub fn explorer_horizontal_offset(&self) -> usize {
        self.show_explorer_horizontal_offset
    }

    pub fn diff_offset(&self) -> usize {
        self.show_diff_offset
    }

    pub fn diff_horizontal_offset(&self) -> usize {
        self.show_diff_horizontal_offset
    }
}

pub struct App {
    config: Config,
    log: LogState,
    screen: Screen,
    input: InputMode,
    status: String,
    running: bool,
    horizontal_viewport_width: usize,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            log: LogState {
                records: Vec::new(),
                selected: 0,
                log_horizontal_offset: 0,
                preview_visible: true,
                preview_focused: false,
                preview_lines: Vec::new(),
                preview_offset: 0,
                preview_horizontal_offset: 0,
                has_more: true,
                next_offset: 0,
                last_search: None,
                last_preview_search: None,
            },
            screen: Screen::Log,
            input: InputMode::Normal,
            status: String::new(),
            running: true,
            horizontal_viewport_width: 1,
        }
    }

    pub fn initialize(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        self.fetch_more(source)?;
        self.reload_preview(source);
        if self.log.records.is_empty() {
            self.status = "No commits found".into();
        } else if self.status.is_empty() {
            self.status = format!("Loaded {} commits", self.log.records.len());
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
                    if matches!(self.screen, Screen::Log)
                        && self.log.preview_visible
                        && self.selected_record().is_some()
                    {
                        self.log.preview_focused = true;
                    } else if matches!(
                        self.screen,
                        Screen::Show(ShowState {
                            show_focus: ShowFocus::Explorer,
                            ..
                        })
                    ) {
                        self.focus_show_diff();
                    }
                    return;
                }
                if let Some(action) = self.config.action_for(&key) {
                    self.dispatch(action, source);
                }
            }
        }
    }

    pub fn set_horizontal_viewport_width(&mut self, width: usize) {
        self.horizontal_viewport_width = width.max(1);
    }

    pub fn log_state(&self) -> &LogState {
        &self.log
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn help_lines(&self) -> Vec<String> {
        self.config.help_lines()
    }

    pub fn dispatch(&mut self, action: Action, source: &mut impl HistorySource) {
        if self.log.preview_focused && matches!(self.screen, Screen::Log) {
            match action {
                Action::MoveDown => {
                    self.log.preview_offset = (self.log.preview_offset + 1)
                        .min(self.log.preview_lines.len().saturating_sub(1))
                }
                Action::MoveUp => {
                    self.log.preview_offset = self.log.preview_offset.saturating_sub(1)
                }
                Action::PageDown => {
                    self.log.preview_offset = (self.log.preview_offset + 10)
                        .min(self.log.preview_lines.len().saturating_sub(1))
                }
                Action::PageUp => {
                    self.log.preview_offset = self.log.preview_offset.saturating_sub(10)
                }
                Action::ScrollRight => self.scroll_horizontal(action),
                Action::ScrollLeft => self.scroll_horizontal(action),
                Action::ScrollStart | Action::ScrollEnd => self.scroll_horizontal(action),
                Action::Search => {
                    self.input = InputMode::Search(String::new());
                }
                Action::SearchNext => self.repeat_preview_search(true),
                Action::SearchPrevious => self.repeat_preview_search(false),
                Action::Back => {
                    self.log.preview_focused = false;
                    self.input = InputMode::Normal;
                }
                Action::Quit | Action::TogglePreview | Action::ShowMode => {}
                _ => {}
            }
            if matches!(
                action,
                Action::Quit | Action::TogglePreview | Action::ShowMode
            ) {
                // These remain global even while preview owns navigation.
            } else {
                return;
            }
        }
        if let Some(focus) = self.screen.show().map(|state| state.show_focus) {
            match focus {
                ShowFocus::Explorer => match action {
                    Action::MoveDown => self.move_show_down(),
                    Action::MoveUp => self.move_show_up(),
                    Action::PageDown => self.page_show_down(),
                    Action::PageUp => self.page_show_up(),
                    Action::ScrollRight
                    | Action::ScrollLeft
                    | Action::ScrollStart
                    | Action::ScrollEnd => self.scroll_horizontal(action),
                    Action::Search => {
                        self.status = "Focus the show diff to search".into();
                    }
                    Action::Back => {
                        self.screen = Screen::Log;
                        self.input = InputMode::Normal;
                        self.status.clear();
                    }
                    Action::Quit | Action::TogglePreview | Action::ShowMode => {}
                    _ => return,
                },
                ShowFocus::Diff => match action {
                    Action::MoveDown => {
                        let show = self.screen.show_mut().unwrap();
                        show.show_diff_offset = (show.show_diff_offset + 1)
                            .min(show.show_diff_lines.len().saturating_sub(1));
                        self.sync_show_selection_to_diff();
                    }
                    Action::MoveUp => {
                        let show = self.screen.show_mut().unwrap();
                        show.show_diff_offset = show.show_diff_offset.saturating_sub(1);
                        self.sync_show_selection_to_diff();
                    }
                    Action::PageDown => {
                        let show = self.screen.show_mut().unwrap();
                        show.show_diff_offset = (show.show_diff_offset + 10)
                            .min(show.show_diff_lines.len().saturating_sub(1));
                        self.sync_show_selection_to_diff();
                    }
                    Action::PageUp => {
                        let show = self.screen.show_mut().unwrap();
                        show.show_diff_offset = show.show_diff_offset.saturating_sub(10);
                        self.sync_show_selection_to_diff();
                    }
                    Action::ScrollRight
                    | Action::ScrollLeft
                    | Action::ScrollStart
                    | Action::ScrollEnd => self.scroll_horizontal(action),
                    Action::Search => self.input = InputMode::Search(String::new()),
                    Action::SearchNext => self.repeat_show_search(true),
                    Action::SearchPrevious => self.repeat_show_search(false),
                    Action::Back => {
                        self.screen.show_mut().unwrap().show_focus = ShowFocus::Explorer;
                        self.input = InputMode::Normal;
                    }
                    Action::Quit | Action::TogglePreview | Action::ShowMode => {}
                    _ => return,
                },
            }
            if matches!(
                action,
                Action::Quit | Action::TogglePreview | Action::ShowMode
            ) {
                // These actions remain global while show owns navigation.
            } else {
                return;
            }
        }
        if matches!(self.screen, Screen::Help(_)) {
            let last_line = self.config.help_lines().len().saturating_sub(1);
            match action {
                Action::MoveDown => {
                    let help = self.screen.help_mut().unwrap();
                    help.offset = (help.offset + 1).min(last_line);
                }
                Action::MoveUp => {
                    let help = self.screen.help_mut().unwrap();
                    help.offset = help.offset.saturating_sub(1);
                }
                Action::PageDown => {
                    let help = self.screen.help_mut().unwrap();
                    help.offset = (help.offset + 10).min(last_line);
                }
                Action::PageUp => {
                    let help = self.screen.help_mut().unwrap();
                    help.offset = help.offset.saturating_sub(10);
                }
                Action::ScrollRight => self.scroll_horizontal(action),
                Action::ScrollLeft => self.scroll_horizontal(action),
                Action::ScrollStart | Action::ScrollEnd => self.scroll_horizontal(action),
                _ => {}
            }
            if matches!(
                action,
                Action::MoveDown
                    | Action::MoveUp
                    | Action::PageDown
                    | Action::PageUp
                    | Action::ScrollStart
                    | Action::ScrollEnd
                    | Action::ScrollRight
                    | Action::ScrollLeft
            ) {
                return;
            }
        }
        match action {
            Action::MoveDown => self.move_down(source),
            Action::MoveUp => self.move_up(source),
            Action::PageDown => {
                let preview_visible = self.log.preview_visible;
                self.log.preview_visible = false;
                for _ in 0..10 {
                    let before = self.log.selected;
                    self.move_down(source);
                    if self.log.selected == before {
                        break;
                    }
                }
                self.log.preview_visible = preview_visible;
                if preview_visible {
                    self.reload_preview(source);
                }
            }
            Action::PageUp => {
                let before = self.log.selected;
                self.log.selected = self.log.selected.saturating_sub(10);
                if self.log.selected != before {
                    self.reload_preview(source);
                }
            }
            Action::ScrollRight => self.scroll_horizontal(action),
            Action::ScrollLeft => self.scroll_horizontal(action),
            Action::ScrollStart | Action::ScrollEnd => self.scroll_horizontal(action),
            Action::Search => {
                self.screen = Screen::Log;
                self.input = InputMode::Search(String::new());
            }
            Action::SearchNext => self.repeat_search(true, source),
            Action::SearchPrevious => self.repeat_search(false, source),
            Action::Command => self.input = InputMode::Command(String::new()),
            Action::Help => {
                self.screen = Screen::Help(HelpState::default());
                self.status = "Showing effective configuration".into();
            }
            Action::Back => {
                self.screen = Screen::Log;
                self.input = InputMode::Normal;
            }
            Action::TogglePreview => {
                self.log.preview_visible = !self.log.preview_visible;
                self.log.preview_focused = false;
                if self.log.preview_visible {
                    self.reload_preview(source);
                }
            }
            Action::ShowMode => self.open_show(source),
            Action::Quit => {
                self.running = false;
                self.status = "Quit requested".into();
            }
        }
    }

    fn scroll_horizontal(&mut self, action: Action) {
        let maximum = self.max_horizontal_offset();
        let step = (self.horizontal_viewport_width / 2).max(1);
        let offset = if self.log.preview_focused && matches!(self.screen, Screen::Log) {
            &mut self.log.preview_horizontal_offset
        } else {
            match &mut self.screen {
                Screen::Help(help) => &mut help.horizontal_offset,
                Screen::Show(show) => match show.show_focus {
                    ShowFocus::Explorer => &mut show.show_explorer_horizontal_offset,
                    ShowFocus::Diff => &mut show.show_diff_horizontal_offset,
                },
                Screen::Log => &mut self.log.log_horizontal_offset,
            }
        };
        *offset = match action {
            Action::ScrollStart => 0,
            Action::ScrollEnd => maximum,
            Action::ScrollLeft => offset.saturating_sub(step),
            Action::ScrollRight => offset.saturating_add(step).min(maximum),
            _ => *offset,
        };
    }

    fn max_horizontal_offset(&self) -> usize {
        let content_width = match &self.screen {
            Screen::Log if self.log.preview_focused => self
                .log
                .preview_lines
                .iter()
                .map(|line| UnicodeWidthStr::width(crate::git::safe_text(line, false).as_str()))
                .max()
                .unwrap_or(0),
            Screen::Help(_) => self
                .config
                .help_lines()
                .iter()
                .map(|line| UnicodeWidthStr::width(line.as_str()))
                .max()
                .unwrap_or(0),
            Screen::Show(show) if show.show_focus == ShowFocus::Diff => show
                .show_diff_lines
                .iter()
                .map(|line| UnicodeWidthStr::width(crate::git::safe_text(line, false).as_str()))
                .max()
                .unwrap_or(0),
            Screen::Show(show) => show
                .show_metadata
                .iter()
                .chain(show.show_files.iter().map(|file| &file.display))
                .map(|line| UnicodeWidthStr::width(crate::git::safe_text(line, false).as_str()))
                .max()
                .unwrap_or(0),
            Screen::Log => self
                .log
                .records
                .iter()
                .map(|record| {
                    UnicodeWidthStr::width(crate::git::safe_text(&record.display, false).as_str())
                })
                .max()
                .unwrap_or(0),
        };
        content_width.saturating_sub(self.horizontal_viewport_width)
    }

    fn move_down(&mut self, source: &mut impl HistorySource) {
        let mut fetched_to_move = false;
        if self.log.selected + 1 >= self.log.records.len() && self.log.has_more {
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not load more history: {error}");
                return;
            }
            fetched_to_move = true;
        }
        if self.log.selected + 1 < self.log.records.len() {
            self.log.selected += 1;
            self.reload_preview(source);
        }
        if !fetched_to_move && self.log.selected + 1 == self.log.records.len() && self.log.has_more
        {
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not load more history: {error}");
            }
        }
    }

    fn move_show_down(&mut self) {
        let show = self.screen.show_mut().unwrap();
        let Some(selected) = show.show_selected else {
            return;
        };
        if selected + 1 < show.show_files.len() {
            show.show_selected = Some(selected + 1);
            show.show_explorer_offset = selected + 1;
        }
    }

    fn move_show_up(&mut self) {
        let show = self.screen.show_mut().unwrap();
        if let Some(selected) = show.show_selected {
            show.show_selected = Some(selected.saturating_sub(1));
            show.show_explorer_offset = selected.saturating_sub(1);
        }
    }

    fn page_show_down(&mut self) {
        let show = self.screen.show_mut().unwrap();
        let Some(selected) = show.show_selected else {
            return;
        };
        if !show.show_files.is_empty() {
            let next = (selected + 10).min(show.show_files.len() - 1);
            show.show_selected = Some(next);
            show.show_explorer_offset = next;
        }
    }

    fn page_show_up(&mut self) {
        let show = self.screen.show_mut().unwrap();
        if let Some(selected) = show.show_selected {
            let next = selected.saturating_sub(10);
            show.show_selected = Some(next);
            show.show_explorer_offset = next;
        }
    }

    fn move_up(&mut self, source: &mut impl HistorySource) {
        let before = self.log.selected;
        self.log.selected = self.log.selected.saturating_sub(1);
        if self.log.selected != before {
            self.reload_preview(source);
        }
    }

    fn fetch_more(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        let batch = source.load(self.log.next_offset, self.config.batch_size)?;
        self.log.next_offset += batch.len();
        self.log.has_more = batch.len() == self.config.batch_size;
        if batch.is_empty() {
            self.log.has_more = false;
            return Ok(());
        }
        let mut known: HashSet<String> = self
            .log
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect();
        self.log.records.extend(
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
        if let Screen::Show(show) = &mut self.screen {
            if show.show_focus == ShowFocus::Diff {
                show.last_show_search = Some(query);
                self.search_show_diff(forward, true);
                return;
            }
        }
        if self.log.preview_focused {
            self.log.last_preview_search = Some(query);
            self.search_preview(forward, true);
            return;
        }
        self.log.last_search = Some(query);
        self.search_log(forward, true, source);
    }

    fn repeat_search(&mut self, forward: bool, source: &mut impl HistorySource) {
        self.search_log(forward, false, source);
    }

    fn search_log(&mut self, forward: bool, wrap: bool, source: &mut impl HistorySource) {
        let Some(query) = self.log.last_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        if self.log.records.is_empty() {
            self.status = format!("No match for `{query}`");
            return;
        }
        let query_lower = query.to_lowercase();
        let mut found = if forward {
            (self.log.selected + 1..self.log.records.len())
                .find(|index| matches_record(&self.log.records[*index], &query_lower))
        } else {
            (0..self.log.selected)
                .rev()
                .find(|index| matches_record(&self.log.records[*index], &query_lower))
        };

        while forward && found.is_none() && self.log.has_more {
            let previous_length = self.log.records.len();
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not continue search: {error}");
                return;
            }
            if forward {
                found = (previous_length..self.log.records.len())
                    .find(|index| matches_record(&self.log.records[*index], &query_lower));
            }
        }

        if found.is_none() && wrap {
            found = if forward {
                (0..=self.log.selected)
                    .find(|index| matches_record(&self.log.records[*index], &query_lower))
            } else {
                (self.log.selected..self.log.records.len())
                    .rev()
                    .find(|index| matches_record(&self.log.records[*index], &query_lower))
            };
        }
        match found {
            Some(index) => {
                self.log.selected = index;
                self.reload_preview(source);
                self.status = format!("Match for `{query}`");
            }
            None if !wrap
                && self
                    .log
                    .records
                    .iter()
                    .any(|record| matches_record(record, &query_lower)) =>
            {
                self.status = if forward { "(END)" } else { "(TOP)" }.into()
            }
            None => self.status = format!("No match for `{query}`"),
        }
    }

    fn reload_preview(&mut self, source: &mut impl HistorySource) {
        if !self.log.preview_visible {
            return;
        }
        self.log.preview_offset = 0;
        self.log.preview_horizontal_offset = 0;
        self.log.preview_lines.clear();
        let Some(id) = self.selected_record().map(|record| record.id.clone()) else {
            return;
        };
        match source.load_preview(&id) {
            Ok(lines) => self.log.preview_lines = lines,
            Err(error) => self.status = format!("Could not load preview: {error}"),
        }
    }

    fn repeat_preview_search(&mut self, forward: bool) {
        self.search_preview(forward, false);
    }

    fn search_preview(&mut self, forward: bool, wrap: bool) {
        let Some(query) = self.log.last_preview_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        let query_lower = query.to_lowercase();
        let length = self.log.preview_lines.len();
        if length == 0 {
            self.status = format!("No match for `{query}`");
            return;
        }
        let matches = |index: &usize| {
            crate::git::safe_text(&self.log.preview_lines[*index], false)
                .to_lowercase()
                .contains(&query_lower)
        };
        let index = if forward {
            let later = (self.log.preview_offset + 1..length).find(&matches);
            later.or_else(|| {
                wrap.then(|| (0..=self.log.preview_offset.min(length - 1)).find(&matches))
                    .flatten()
            })
        } else {
            let earlier = (0..self.log.preview_offset).rev().find(&matches);
            earlier.or_else(|| {
                wrap.then(|| (self.log.preview_offset..length).rev().find(&matches))
                    .flatten()
            })
        };
        if let Some(index) = index {
            self.log.preview_offset = index;
            self.status = format!("Match for `{query}`");
        } else if !wrap
            && self
                .log
                .preview_lines
                .iter()
                .enumerate()
                .any(|(index, _)| matches(&index))
        {
            self.status = if forward { "(END)" } else { "(TOP)" }.into();
        } else {
            self.status = format!("No match for `{query}`");
        }
    }

    fn open_show(&mut self, source: &mut impl HistorySource) {
        let Some(id) = self.selected_record().map(|record| record.id.clone()) else {
            self.status = "Could not open show: No selected commit".into();
            return;
        };

        match source.load_show(&id) {
            Ok(data) => {
                self.screen = Screen::Show(ShowState {
                    show_metadata: data.metadata,
                    show_selected: (!data.files.is_empty()).then_some(0),
                    show_files: data.files,
                    show_diff_lines: data.diff,
                    ..ShowState::default()
                });
                self.input = InputMode::Normal;
                self.status.clear();
            }
            Err(error) => {
                self.screen = Screen::Log;
                self.status = format!("Could not load show: {error}");
            }
        }
    }

    fn focus_show_diff(&mut self) {
        let show = self.screen.show_mut().unwrap();
        let Some(selected) = show.show_selected else {
            return;
        };
        let Some(file) = show.show_files.get(selected) else {
            show.show_selected = None;
            return;
        };
        show.show_focus = ShowFocus::Diff;
        show.show_diff_horizontal_offset = 0;
        let status = if let Some(offset) = patch_offset(&show.show_diff_lines, file) {
            show.show_diff_offset = offset;
            None
        } else {
            show.show_diff_offset = 0;
            Some(format!(
                "Patch location unavailable for {}",
                crate::git::safe_text(&file.display, false)
            ))
        };
        if let Some(status) = status {
            self.status = status;
        } else {
            self.status.clear();
        }
    }

    fn sync_show_selection_to_diff(&mut self) {
        let show = self.screen.show_mut().unwrap();
        if let Some(selected) = file_at_patch_offset(
            &show.show_diff_lines,
            &show.show_files,
            show.show_diff_offset,
        ) {
            show.show_selected = Some(selected);
            show.show_explorer_offset = selected;
        }
    }

    fn repeat_show_search(&mut self, forward: bool) {
        self.search_show_diff(forward, false);
    }

    fn search_show_diff(&mut self, forward: bool, wrap: bool) {
        let show = self.screen.show_mut().unwrap();
        let Some(query) = show.last_show_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        let query_lower = query.to_lowercase();
        let length = show.show_diff_lines.len();
        if length == 0 {
            self.status = format!("No match for `{query}`");
            return;
        }
        let matches = |index: &usize| {
            crate::git::safe_text(&show.show_diff_lines[*index], false)
                .to_lowercase()
                .contains(&query_lower)
        };
        let index = if forward {
            let later = (show.show_diff_offset + 1..length).find(&matches);
            later.or_else(|| {
                wrap.then(|| (0..=show.show_diff_offset.min(length - 1)).find(&matches))
                    .flatten()
            })
        } else {
            let earlier = (0..show.show_diff_offset).rev().find(&matches);
            earlier.or_else(|| {
                wrap.then(|| (show.show_diff_offset..length).rev().find(&matches))
                    .flatten()
            })
        };
        if let Some(index) = index {
            show.show_diff_offset = index;
            self.sync_show_selection_to_diff();
            self.status = format!("Match for `{query}`");
        } else {
            let has_match = !wrap
                && show
                    .show_diff_lines
                    .iter()
                    .enumerate()
                    .any(|(index, _)| matches(&index));
            self.status = if has_match {
                if forward { "(END)" } else { "(TOP)" }.into()
            } else {
                format!("No match for `{query}`")
            };
        }
    }

    fn submit_command(&mut self, command: &str) {
        match command.trim() {
            "help" | "h" => {
                self.screen = Screen::Help(HelpState::default());
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
        self.log.records.get(self.log.selected)
    }

    pub fn input_label(&self) -> Option<String> {
        match &self.input {
            InputMode::Normal => None,
            InputMode::Search(value) => Some(format!("/{value}")),
            InputMode::Command(value) => Some(format!(":{value}")),
        }
    }

    pub fn log_search_query(&self) -> Option<&str> {
        self.log.last_search.as_deref()
    }

    pub fn preview_search_query(&self) -> Option<&str> {
        self.log.last_preview_search.as_deref()
    }

    pub fn show_search_query(&self) -> Option<&str> {
        self.screen
            .show()
            .and_then(|show| show.last_show_search.as_deref())
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
    use crate::git::ShowData;

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

    fn show_data() -> ShowData {
        ShowData {
            metadata: vec!["commit id-0".into(), "Author: Test".into()],
            files: vec![
                ChangedFile {
                    display: "M one.rs".into(),
                    old_path: Some("one.rs".into()),
                    new_path: Some("one.rs".into()),
                },
                ChangedFile {
                    display: "R100 old.rs new.rs".into(),
                    old_path: Some("old.rs".into()),
                    new_path: Some("new.rs".into()),
                },
            ],
            diff: vec![
                "".into(),
                "diff --git a/one.rs b/one.rs".into(),
                "@@ -1 +1 @@".into(),
                "needle".into(),
                "diff --git a/old.rs b/new.rs".into(),
                "other needle".into(),
            ],
        }
    }

    struct ShowHistory {
        records: Vec<CommitRecord>,
        result: Result<ShowData, GitError>,
    }

    impl HistorySource for ShowHistory {
        fn load(&mut self, offset: usize, limit: usize) -> Result<Vec<CommitRecord>, GitError> {
            Ok(self
                .records
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect())
        }

        fn load_show(&mut self, _id: &str) -> Result<ShowData, GitError> {
            self.result.clone()
        }
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
        assert_eq!(app.log.records.len(), 4);
        assert_eq!(
            app.log
                .records
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
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.log.records.len(), 2);
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
        assert_eq!(app.log.records.len(), 2);
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
        assert_eq!(app.log.records.len(), 2);
        assert!(!app.log.has_more);
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
        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "Search query is empty");
        app.submit_search("missing".into(), true, &mut history);
        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "No match for `missing`");
        app.submit_command("wat");
        assert_eq!(app.status, "Unknown command: wat");
        assert!(app.running);
    }

    #[test]
    fn help_command_replaces_scrolled_help_with_fresh_state() {
        let mut app = App::new(Config::default());
        app.screen = Screen::Help(HelpState {
            offset: 5,
            horizontal_offset: 7,
        });

        app.submit_command("help");

        assert_eq!(app.screen, Screen::Help(HelpState::default()));
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
        app.log.selected = 1;
        app.log.preview_offset = 1;
        app.log.preview_horizontal_offset = 2;
        let log_before = app.log.clone();
        app.dispatch(Action::Help, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.screen.help().unwrap().offset, 1);
        assert_eq!(app.log.selected, 1);
        app.dispatch(Action::PageDown, &mut history);
        assert!(app.screen.help().unwrap().offset > 1);
        assert_eq!(app.log.selected, 1);
        app.dispatch(Action::PageUp, &mut history);
        assert_eq!(app.screen.help().unwrap().offset, 1);
        app.dispatch(Action::Back, &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log, log_before);
    }

    #[test]
    fn horizontal_scroll_routes_to_active_view_and_clamps_at_zero() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(3),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.log.selected = 1;
        app.set_horizontal_viewport_width(4);

        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 2);
        app.dispatch(Action::ScrollRight, &mut history);
        app.dispatch(Action::ScrollLeft, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 2);
        assert_eq!(app.log.selected, 1);

        app.dispatch(Action::Help, &mut history);
        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 2);
        app.dispatch(Action::ScrollLeft, &mut history);
        app.dispatch(Action::ScrollLeft, &mut history);
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 0);
        assert_eq!(app.log.selected, 1);

        app.dispatch(Action::Back, &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.log.preview_lines = vec!["abcdef".into()];
        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 2);
        app.dispatch(Action::ScrollRight, &mut history);
        app.dispatch(Action::ScrollLeft, &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 0);
        assert_eq!(app.log.preview_offset, 0);
        assert_eq!(app.log.selected, 1);
    }

    #[test]
    fn horizontal_start_and_end_use_the_active_pane_width() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "abcdef".into(),
            }],
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.set_horizontal_viewport_width(3);

        app.dispatch(Action::ScrollEnd, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 3);
        app.dispatch(Action::ScrollStart, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 0);

        app.dispatch(Action::Help, &mut history);
        app.set_horizontal_viewport_width(8);
        app.dispatch(Action::ScrollEnd, &mut history);
        assert!(app.screen.help().unwrap().horizontal_offset > 0);
        app.dispatch(Action::ScrollStart, &mut history);
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 0);

        app.dispatch(Action::Back, &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.log.preview_lines = vec!["abcdef".into()];
        app.set_horizontal_viewport_width(3);
        app.dispatch(Action::ScrollEnd, &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 3);
        app.dispatch(Action::ScrollStart, &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 0);
    }

    #[test]
    fn horizontal_scroll_moves_half_the_viewport_width() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "abcdefghijklmnopqrstuvwxyz".into(),
            }],
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.set_horizontal_viewport_width(10);

        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 5);
        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 10);
        app.dispatch(Action::ScrollLeft, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 5);
    }

    #[test]
    fn horizontal_end_accounts_for_the_log_selection_marker() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: vec![CommitRecord {
                id: "id".into(),
                display: "abcdef".into(),
            }],
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.set_horizontal_viewport_width(2);
        app.dispatch(Action::ScrollEnd, &mut history);
        assert_eq!(app.log.log_horizontal_offset, 4);
    }

    #[test]
    fn reloading_preview_and_opening_help_reset_horizontal_offsets() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(2),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.log.preview_horizontal_offset = 4;
        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 0);

        app.screen = Screen::Help(HelpState {
            horizontal_offset: 4,
            ..HelpState::default()
        });
        app.dispatch(Action::Help, &mut history);
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 0);
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
            app.log
                .records
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
        assert!(app.log.preview_visible);
        assert_eq!(app.log.preview_lines[0], "id-0 first");

        app.handle_key(Key::Enter, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert!(app.log.preview_focused);
        assert_eq!(app.log.preview_offset, 1);
        assert_eq!(app.log.selected, 0);

        app.handle_key(Key::Char('/'), &mut history);
        for key in "needle".chars().map(Key::Char).chain([Key::Enter]) {
            app.handle_key(key, &mut history);
        }
        assert_eq!(app.log.preview_offset, 1);
        app.dispatch(Action::Back, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        assert!(!app.log.preview_focused);
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.log.preview_offset, 0);
        assert_eq!(app.log.preview_lines[0], "id-1 first");
    }

    #[test]
    fn repeated_preview_search_stops_at_both_boundaries() {
        let mut app = App::new(Config::default());
        app.log.preview_lines = vec![
            "match first".into(),
            "skip".into(),
            "match second".into(),
            "match third".into(),
        ];
        app.log.last_preview_search = Some("match".into());

        app.log.preview_offset = 1;
        app.repeat_preview_search(true);
        assert_eq!(app.log.preview_offset, 2);
        app.repeat_preview_search(true);
        assert_eq!(app.log.preview_offset, 3);
        app.repeat_preview_search(true);
        assert_eq!(app.log.preview_offset, 3);
        assert_eq!(app.status, "(END)");

        app.repeat_preview_search(false);
        assert_eq!(app.log.preview_offset, 2);
        app.repeat_preview_search(false);
        assert_eq!(app.log.preview_offset, 0);
        app.repeat_preview_search(false);
        assert_eq!(app.log.preview_offset, 0);
        assert_eq!(app.status, "(TOP)");

        app.log.preview_lines = vec!["only match".into(), "skip".into()];
        app.log.preview_offset = 1;
        app.repeat_preview_search(true);
        assert_eq!(app.log.preview_offset, 1);
        assert_eq!(app.status, "(END)");
        app.repeat_preview_search(false);
        assert_eq!(app.log.preview_offset, 0);

        app.log.last_preview_search = Some("missing".into());
        app.repeat_preview_search(true);
        assert_eq!(app.log.preview_offset, 0);
        assert_eq!(app.status, "No match for `missing`");
    }

    #[test]
    fn repeated_log_search_loads_batches_and_stops_at_both_boundaries() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut history = FakeHistory {
            records: vec![
                CommitRecord {
                    id: "0".into(),
                    display: "match first".into(),
                },
                CommitRecord {
                    id: "1".into(),
                    display: "skip".into(),
                },
                CommitRecord {
                    id: "2".into(),
                    display: "match second".into(),
                },
                CommitRecord {
                    id: "3".into(),
                    display: "skip again".into(),
                },
                CommitRecord {
                    id: "4".into(),
                    display: "match third".into(),
                },
            ],
            fail_at: None,
        };
        let mut app = App::new(config);
        app.initialize(&mut history).unwrap();

        app.log.last_search = Some("match".into());
        app.repeat_search(false, &mut history);
        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "(TOP)");
        assert_eq!(app.log.records.len(), 2);

        app.submit_search("match".into(), true, &mut history);
        assert_eq!(app.log.selected, 2);
        assert_eq!(app.log.records.len(), 4);
        app.repeat_search(true, &mut history);
        assert_eq!(app.log.selected, 4);
        assert_eq!(app.log.records.len(), 5);
        app.repeat_search(true, &mut history);
        assert_eq!(app.log.selected, 4);
        assert_eq!(app.status, "(END)");

        app.repeat_search(false, &mut history);
        assert_eq!(app.log.selected, 2);
        app.log.selected = 0;
        app.repeat_search(false, &mut history);
        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "(TOP)");

        app.submit_search("missing".into(), true, &mut history);
        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "No match for `missing`");
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
        assert_eq!(app.log.selected, 1);
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
        assert!(!app.log.preview_visible);
        assert!(!app.log.preview_focused);
        app.log.preview_visible = true;
        app.screen = Screen::Help(HelpState::default());
        app.handle_key(Key::Enter, &mut history);
        assert!(!app.log.preview_focused);
        app.screen = Screen::Log;
        app.log.preview_focused = true;
        app.dispatch(Action::Quit, &mut history);
        assert!(!app.running);
    }

    #[test]
    fn show_navigation_keeps_log_selection_and_escape_has_two_stages() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(2),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.log.selected = 1;
        app.log.preview_offset = 1;
        app.log.preview_horizontal_offset = 2;
        let log_before = app.log.clone();
        app.dispatch(Action::ShowMode, &mut history);

        assert!(matches!(app.screen, Screen::Show(_)));
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(0));
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.status, "");

        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
        assert_eq!(app.log.selected, 1);
        app.handle_key(Key::Enter, &mut history);
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Diff);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 4);

        app.dispatch(Action::MoveDown, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 5);
        assert_eq!(app.log.selected, 1);
        app.handle_key(Key::Escape, &mut history);
        assert!(matches!(app.screen, Screen::Show(_)));
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log, log_before);
        assert_eq!(app.status, "");
    }

    #[test]
    fn scrolling_show_diff_keeps_explorer_selection_on_current_file() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::ShowMode, &mut history);
        app.handle_key(Key::Enter, &mut history);

        for _ in 0..3 {
            app.dispatch(Action::MoveDown, &mut history);
        }
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 4);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));

        app.dispatch(Action::MoveUp, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 3);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(0));
        app.dispatch(Action::PageDown, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 5);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
    }

    #[test]
    fn show_search_keeps_explorer_selection_on_the_matched_file() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::ShowMode, &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.handle_key(Key::Char('/'), &mut history);
        for key in "other".chars().map(Key::Char).chain([Key::Enter]) {
            app.handle_key(key, &mut history);
        }
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 5);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
    }

    #[test]
    fn show_page_navigation_and_explorer_scrolling_keep_log_state_independent() {
        let mut data = show_data();
        data.files = (0..25)
            .map(|index| ChangedFile {
                display: format!("M file-{index}-with-a-long-name.rs"),
                old_path: Some(format!("file-{index}.rs")),
                new_path: Some(format!("file-{index}.rs")),
            })
            .collect();
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(2),
            result: Ok(data),
        };
        app.initialize(&mut history).unwrap();
        app.log.selected = 1;
        app.dispatch(Action::ShowMode, &mut history);
        app.set_horizontal_viewport_width(4);

        app.dispatch(Action::PageDown, &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(10));
        assert_eq!(app.screen.show().unwrap().show_explorer_offset, 10);
        app.dispatch(Action::PageDown, &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(20));
        app.dispatch(Action::PageUp, &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(10));
        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(
            app.screen.show().unwrap().show_explorer_horizontal_offset,
            2
        );
        assert_eq!(app.screen.show().unwrap().show_diff_horizontal_offset, 0);
        assert_eq!(app.log.selected, 1);
    }

    #[test]
    fn show_jumps_to_added_deleted_and_copied_patch_headers() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(ShowData {
                files: vec![
                    ChangedFile {
                        display: "A added.rs".into(),
                        old_path: None,
                        new_path: Some("added.rs".into()),
                    },
                    ChangedFile {
                        display: "D deleted.rs".into(),
                        old_path: Some("deleted.rs".into()),
                        new_path: None,
                    },
                    ChangedFile {
                        display: "C100 source.rs copy.rs".into(),
                        old_path: Some("source.rs".into()),
                        new_path: Some("copy.rs".into()),
                    },
                ],
                diff: vec![
                    "diff --git a/added.rs b/added.rs".into(),
                    "diff --git a/deleted.rs b/deleted.rs".into(),
                    "diff --git a/source.rs b/copy.rs".into(),
                ],
                ..ShowData::default()
            }),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::ShowMode, &mut history);

        for (selected, offset) in [(0, 0), (1, 1), (2, 2)] {
            app.screen.show_mut().unwrap().show_selected = Some(selected);
            app.handle_key(Key::Enter, &mut history);
            assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Diff);
            assert_eq!(app.screen.show().unwrap().show_diff_offset, offset);
            app.handle_key(Key::Escape, &mut history);
        }
    }

    #[test]
    fn show_clears_a_stale_missing_patch_status_after_a_valid_jump() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(ShowData {
                files: vec![
                    ChangedFile {
                        display: "M missing.rs".into(),
                        old_path: Some("missing.rs".into()),
                        new_path: Some("missing.rs".into()),
                    },
                    ChangedFile {
                        display: "M valid.rs".into(),
                        old_path: Some("valid.rs".into()),
                        new_path: Some("valid.rs".into()),
                    },
                ],
                diff: vec!["diff --git a/valid.rs b/valid.rs".into()],
                ..ShowData::default()
            }),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::ShowMode, &mut history);
        app.handle_key(Key::Enter, &mut history);
        assert!(app.status.contains("Patch location unavailable"));
        app.handle_key(Key::Escape, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        app.handle_key(Key::Enter, &mut history);
        assert_eq!(app.status, "");
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 0);
    }

    #[test]
    fn show_search_is_independent_and_reopening_resets_show_state() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(2),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::ShowMode, &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.handle_key(Key::Char('/'), &mut history);
        for key in "needle".chars().map(Key::Char).chain([Key::Enter]) {
            app.handle_key(key, &mut history);
        }
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 3);
        assert_eq!(app.show_search_query(), Some("needle"));
        assert_eq!(app.log.selected, 0);
        app.screen
            .show_mut()
            .unwrap()
            .show_diff_lines
            .push("x".repeat(200));
        app.set_horizontal_viewport_width(4);
        app.dispatch(Action::ScrollRight, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_horizontal_offset, 2);
        assert_eq!(app.log.log_horizontal_offset, 0);
        app.dispatch(Action::SearchNext, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 5);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
        app.dispatch(Action::SearchNext, &mut history);
        assert_eq!(app.status, "(END)");
        app.dispatch(Action::SearchPrevious, &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 3);
        app.dispatch(Action::SearchPrevious, &mut history);
        assert_eq!(app.status, "(TOP)");
        app.handle_key(Key::Escape, &mut history);
        app.handle_key(Key::Escape, &mut history);
        app.dispatch(Action::MoveDown, &mut history);
        app.dispatch(Action::ShowMode, &mut history);
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(0));
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 0);
        assert_eq!(app.screen.show().unwrap().show_diff_horizontal_offset, 0);
        assert_eq!(app.show_search_query(), None);
    }

    #[test]
    fn show_failures_and_empty_commits_leave_a_recoverable_log() {
        let mut app = App::new(Config::default());
        let mut failing = ShowHistory {
            records: records(1),
            result: Err(GitError::Output("planned show failure".into())),
        };
        app.initialize(&mut failing).unwrap();
        app.dispatch(Action::ShowMode, &mut failing);
        assert_eq!(app.screen, Screen::Log);
        assert!(app.status.contains("planned show failure"));
        assert_eq!(app.log.selected, 0);

        let mut app = App::new(Config::default());
        let mut empty = ShowHistory {
            records: records(1),
            result: Ok(ShowData {
                metadata: vec!["commit id-0".into()],
                ..ShowData::default()
            }),
        };
        app.initialize(&mut empty).unwrap();
        app.dispatch(Action::ShowMode, &mut empty);
        app.handle_key(Key::Enter, &mut empty);
        assert!(matches!(app.screen, Screen::Show(_)));
        assert_eq!(app.screen.show().unwrap().show_selected, None);
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
        app.handle_key(Key::Escape, &mut empty);
        assert_eq!(app.screen, Screen::Log);

        let mut app = App::new(Config::default());
        let mut empty_log = ShowHistory {
            records: Vec::new(),
            result: Ok(ShowData::default()),
        };
        app.initialize(&mut empty_log).unwrap();
        app.dispatch(Action::ShowMode, &mut empty_log);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.status, "Could not open show: No selected commit");
    }
}
