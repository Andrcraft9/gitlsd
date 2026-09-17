//! Application state machine and behavior.
//!
//! [`App`] owns persistent log state plus enum-scoped help, show, and status state,
//! input modes, commands, shutdown intent, and loaded-diff indexes/revisions that
//! remain independent of terminal rendering. Editor actions become requests for
//! the active frontend so terminal lifecycle stays outside the state machine.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    Action, Config, GlobalAction, Key, NavigationAction, PreviewAction, SearchAction, ShowAction,
    StatusAction,
};
use crate::editor::EditorRequest;
use crate::git::{
    ChangedFile, CommitRecord, GitError, HistorySource, PatchIndex, StatusData, StatusFile,
    diff_line_number,
};
use unicode_width::UnicodeWidthStr;

/// The active screen and the state that exists only while that screen is open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Screen {
    Log,
    Help(HelpState),
    Show(ShowState),
    Status(StatusState),
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

    pub fn status(&self) -> Option<&StatusState> {
        match self {
            Self::Status(state) => Some(state),
            _ => None,
        }
    }

    fn status_mut(&mut self) -> Option<&mut StatusState> {
        match self {
            Self::Status(state) => Some(state),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShowFocus {
    Explorer,
    Diff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusFocus {
    Explorer,
    Diff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusGroup {
    Staged,
    Unstaged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivePane {
    Log,
    Preview,
    Help,
    Show(ShowFocus),
    Status(StatusFocus),
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
    preview_revision: u64,
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
    show_diff_fullscreen: bool,
    show_metadata: Vec<String>,
    show_files: Vec<ChangedFile>,
    show_diff_lines: Vec<String>,
    show_patch_index: PatchIndex,
    show_diff_revision: u64,
    show_selected: Option<usize>,
    show_explorer_offset: usize,
    show_explorer_horizontal_offset: usize,
    show_diff_offset: usize,
    show_diff_horizontal_offset: usize,
    last_show_search: Option<String>,
}

/// Explorer, diff, and search state that exists only while status is active.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusState {
    status_focus: StatusFocus,
    status_diff_fullscreen: bool,
    active_group: StatusGroup,
    staged: Vec<StatusFile>,
    unstaged: Vec<StatusFile>,
    staged_diff: Vec<String>,
    unstaged_diff: Vec<String>,
    staged_patch_index: PatchIndex,
    unstaged_patch_index: PatchIndex,
    staged_diff_revision: u64,
    unstaged_diff_revision: u64,
    staged_selected: Option<usize>,
    unstaged_selected: Option<usize>,
    staged_explorer_offset: usize,
    unstaged_explorer_offset: usize,
    staged_diff_offset: usize,
    unstaged_diff_offset: usize,
    staged_diff_horizontal_offset: usize,
    unstaged_diff_horizontal_offset: usize,
    explorer_horizontal_offset: usize,
    last_search: Option<String>,
}

fn next_document_revision() -> u64 {
    static NEXT_DOCUMENT_REVISION: AtomicU64 = AtomicU64::new(1);
    NEXT_DOCUMENT_REVISION.fetch_add(1, Ordering::Relaxed)
}

impl Default for ShowState {
    fn default() -> Self {
        Self {
            show_focus: ShowFocus::Explorer,
            show_diff_fullscreen: false,
            show_metadata: Vec::new(),
            show_files: Vec::new(),
            show_diff_lines: Vec::new(),
            show_patch_index: PatchIndex::default(),
            show_diff_revision: next_document_revision(),
            show_selected: None,
            show_explorer_offset: 0,
            show_explorer_horizontal_offset: 0,
            show_diff_offset: 0,
            show_diff_horizontal_offset: 0,
            last_show_search: None,
        }
    }
}

impl Default for StatusState {
    fn default() -> Self {
        Self {
            status_focus: StatusFocus::Explorer,
            status_diff_fullscreen: false,
            active_group: StatusGroup::Unstaged,
            staged: Vec::new(),
            unstaged: Vec::new(),
            staged_diff: Vec::new(),
            unstaged_diff: Vec::new(),
            staged_patch_index: PatchIndex::default(),
            unstaged_patch_index: PatchIndex::default(),
            staged_diff_revision: next_document_revision(),
            unstaged_diff_revision: next_document_revision(),
            staged_selected: None,
            unstaged_selected: None,
            staged_explorer_offset: 0,
            unstaged_explorer_offset: 0,
            staged_diff_offset: 0,
            unstaged_diff_offset: 0,
            staged_diff_horizontal_offset: 0,
            unstaged_diff_horizontal_offset: 0,
            explorer_horizontal_offset: 0,
            last_search: None,
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

    pub fn preview_revision(&self) -> u64 {
        self.preview_revision
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

    pub fn diff_fullscreen(&self) -> bool {
        self.show_diff_fullscreen
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

    pub fn diff_revision(&self) -> u64 {
        self.show_diff_revision
    }
}

impl StatusState {
    fn from_data(data: StatusData) -> Self {
        let staged_selected = (!data.staged.is_empty()).then_some(0);
        let unstaged_selected = (!data.unstaged.is_empty()).then_some(0);
        let staged_patch_index = PatchIndex::for_status_files(&data.staged_diff, &data.staged);
        let unstaged_patch_index =
            PatchIndex::for_status_files(&data.unstaged_diff, &data.unstaged);
        Self {
            staged: data.staged,
            unstaged: data.unstaged,
            staged_diff: data.staged_diff,
            unstaged_diff: data.unstaged_diff,
            staged_patch_index,
            unstaged_patch_index,
            staged_diff_revision: next_document_revision(),
            unstaged_diff_revision: next_document_revision(),
            staged_selected,
            unstaged_selected,
            ..Self::default()
        }
    }

    pub fn focus(&self) -> StatusFocus {
        self.status_focus
    }

    pub fn diff_fullscreen(&self) -> bool {
        self.status_diff_fullscreen
    }

    pub fn group(&self) -> StatusGroup {
        self.active_group
    }

    pub fn staged(&self) -> &[StatusFile] {
        &self.staged
    }

    pub fn unstaged(&self) -> &[StatusFile] {
        &self.unstaged
    }

    pub fn staged_diff(&self) -> &[String] {
        &self.staged_diff
    }

    pub fn unstaged_diff(&self) -> &[String] {
        &self.unstaged_diff
    }

    pub fn selected(&self) -> Option<usize> {
        match self.active_group {
            StatusGroup::Staged => self.staged_selected,
            StatusGroup::Unstaged => self.unstaged_selected,
        }
    }

    pub fn staged_selected(&self) -> Option<usize> {
        self.staged_selected
    }

    pub fn unstaged_selected(&self) -> Option<usize> {
        self.unstaged_selected
    }

    pub fn explorer_offset(&self) -> usize {
        match self.active_group {
            StatusGroup::Staged => self.staged_explorer_offset,
            StatusGroup::Unstaged => self.unstaged_explorer_offset,
        }
    }

    pub fn staged_explorer_offset(&self) -> usize {
        self.staged_explorer_offset
    }

    pub fn unstaged_explorer_offset(&self) -> usize {
        self.unstaged_explorer_offset
    }

    pub fn explorer_horizontal_offset(&self) -> usize {
        self.explorer_horizontal_offset
    }

    pub fn diff_offset(&self) -> usize {
        match self.active_group {
            StatusGroup::Staged => self.staged_diff_offset,
            StatusGroup::Unstaged => self.unstaged_diff_offset,
        }
    }

    pub fn diff_horizontal_offset(&self) -> usize {
        match self.active_group {
            StatusGroup::Staged => self.staged_diff_horizontal_offset,
            StatusGroup::Unstaged => self.unstaged_diff_horizontal_offset,
        }
    }

    pub fn diff_revision(&self) -> u64 {
        match self.active_group {
            StatusGroup::Staged => self.staged_diff_revision,
            StatusGroup::Unstaged => self.unstaged_diff_revision,
        }
    }

    pub fn diff_lines(&self) -> &[String] {
        match self.active_group {
            StatusGroup::Staged => &self.staged_diff,
            StatusGroup::Unstaged => &self.unstaged_diff,
        }
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
    editor_request: Option<EditorRequest>,
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
                preview_revision: next_document_revision(),
            },
            screen: Screen::Log,
            input: InputMode::Normal,
            status: String::new(),
            running: true,
            horizontal_viewport_width: 1,
            editor_request: None,
        }
    }

    pub fn initialize(&mut self, source: &mut impl HistorySource) -> Result<(), GitError> {
        match self.fetch_more(source) {
            Ok(()) => {}
            Err(GitError::Unborn) => {
                self.log.has_more = false;
            }
            Err(error) => return Err(error),
        }
        self.reload_preview(source);
        if self.log.records.is_empty() {
            self.status = "No commits found".into();
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key, source: &mut impl HistorySource) {
        if self.config.action_for(&key) == Some(Action::Global(GlobalAction::Quit)) {
            self.dispatch(Action::Global(GlobalAction::Quit), source);
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
                    self.submit_command(&command, source);
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
                    } else if matches!(
                        self.screen,
                        Screen::Show(ShowState {
                            show_focus: ShowFocus::Diff,
                            show_diff_fullscreen: false,
                            ..
                        })
                    ) {
                        self.screen.show_mut().unwrap().show_diff_fullscreen = true;
                    } else if matches!(
                        self.screen,
                        Screen::Status(StatusState {
                            status_focus: StatusFocus::Explorer,
                            ..
                        })
                    ) {
                        self.focus_status_diff();
                    } else if matches!(
                        self.screen,
                        Screen::Status(StatusState {
                            status_focus: StatusFocus::Diff,
                            status_diff_fullscreen: false,
                            ..
                        })
                    ) {
                        self.screen.status_mut().unwrap().status_diff_fullscreen = true;
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

    pub fn editor_request(&self) -> Option<&EditorRequest> {
        self.editor_request.as_ref()
    }

    pub fn take_editor_request(&mut self) -> Option<EditorRequest> {
        self.editor_request.take()
    }

    pub fn finish_editor(&mut self, result: Result<(), String>, source: &mut impl HistorySource) {
        if let Err(error) = result {
            self.status = format!("Could not open editor: {error}");
            return;
        }
        if matches!(self.screen, Screen::Status(_)) {
            let focus_diff = self
                .screen
                .status()
                .is_some_and(|status| status.status_focus == StatusFocus::Diff);
            match source.load_status() {
                Ok(data) => {
                    self.replace_status_data(data);
                    if focus_diff {
                        self.focus_status_diff();
                    }
                }
                Err(error) => {
                    self.status = format!("Could not refresh status after editor: {error}");
                }
            }
        } else {
            self.status.clear();
        }
    }

    pub fn dispatch(&mut self, action: Action, source: &mut impl HistorySource) {
        match action {
            Action::Navigation(action) => self.dispatch_navigation(action, source),
            Action::Search(action) => self.dispatch_search(action, source),
            Action::Global(action) => self.dispatch_global(action, source),
            Action::Preview(action) => self.dispatch_preview_action(action, source),
            Action::Show(action) => self.dispatch_show_action(action, source),
            Action::Status(action) => self.dispatch_status_action(action, source),
        }
    }

    fn dispatch_navigation(&mut self, action: NavigationAction, source: &mut impl HistorySource) {
        match self.active_pane() {
            ActivePane::Preview => self.dispatch_preview_navigation(action),
            ActivePane::Show(focus) => self.dispatch_show_navigation(action, focus),
            ActivePane::Status(focus) => self.dispatch_status_navigation(action, focus),
            ActivePane::Help => self.dispatch_help_navigation(action),
            ActivePane::Log => self.dispatch_log_navigation(action, source),
        }
    }

    fn dispatch_preview_navigation(&mut self, action: NavigationAction) {
        match action {
            NavigationAction::NextFile | NavigationAction::PreviousFile => {}
            NavigationAction::MoveDown => {
                self.log.preview_offset = (self.log.preview_offset + 1)
                    .min(self.log.preview_lines.len().saturating_sub(1));
            }
            NavigationAction::MoveUp => {
                self.log.preview_offset = self.log.preview_offset.saturating_sub(1);
            }
            NavigationAction::PageDown => {
                self.log.preview_offset = (self.log.preview_offset + 10)
                    .min(self.log.preview_lines.len().saturating_sub(1));
            }
            NavigationAction::PageUp => {
                self.log.preview_offset = self.log.preview_offset.saturating_sub(10);
            }
            NavigationAction::ScrollRight
            | NavigationAction::ScrollLeft
            | NavigationAction::ScrollStart
            | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
        }
    }

    fn dispatch_show_navigation(&mut self, action: NavigationAction, focus: ShowFocus) {
        match focus {
            ShowFocus::Explorer => match action {
                NavigationAction::NextFile | NavigationAction::PreviousFile => {}
                NavigationAction::MoveDown => self.move_show_down(),
                NavigationAction::MoveUp => self.move_show_up(),
                NavigationAction::PageDown => self.page_show_down(),
                NavigationAction::PageUp => self.page_show_up(),
                NavigationAction::ScrollRight
                | NavigationAction::ScrollLeft
                | NavigationAction::ScrollStart
                | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
            },
            ShowFocus::Diff => match action {
                NavigationAction::NextFile | NavigationAction::PreviousFile => {
                    let before = self.screen.show().unwrap().show_selected;
                    if action == NavigationAction::NextFile {
                        self.move_show_down();
                    } else {
                        self.move_show_up();
                    }
                    if self.screen.show().unwrap().show_selected != before {
                        self.focus_show_diff();
                    }
                }
                NavigationAction::MoveDown => self.move_show_diff(1, true),
                NavigationAction::MoveUp => self.move_show_diff(1, false),
                NavigationAction::PageDown => self.move_show_diff(10, true),
                NavigationAction::PageUp => self.move_show_diff(10, false),
                NavigationAction::ScrollRight
                | NavigationAction::ScrollLeft
                | NavigationAction::ScrollStart
                | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
            },
        }
    }

    fn dispatch_status_navigation(&mut self, action: NavigationAction, focus: StatusFocus) {
        match focus {
            StatusFocus::Explorer => match action {
                NavigationAction::NextFile | NavigationAction::PreviousFile => {}
                NavigationAction::MoveDown => self.move_status_down(),
                NavigationAction::MoveUp => self.move_status_up(),
                NavigationAction::PageDown => self.page_status_down(),
                NavigationAction::PageUp => self.page_status_up(),
                NavigationAction::ScrollRight
                | NavigationAction::ScrollLeft
                | NavigationAction::ScrollStart
                | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
            },
            StatusFocus::Diff => match action {
                NavigationAction::NextFile | NavigationAction::PreviousFile => {
                    let before = self.screen.status().unwrap().selected();
                    if action == NavigationAction::NextFile {
                        self.move_status_down();
                    } else {
                        self.move_status_up();
                    }
                    if self.screen.status().unwrap().selected() != before {
                        self.focus_status_diff();
                    }
                }
                NavigationAction::MoveDown => self.move_status_diff(1, true),
                NavigationAction::MoveUp => self.move_status_diff(1, false),
                NavigationAction::PageDown => self.move_status_diff(10, true),
                NavigationAction::PageUp => self.move_status_diff(10, false),
                NavigationAction::ScrollRight
                | NavigationAction::ScrollLeft
                | NavigationAction::ScrollStart
                | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
            },
        }
    }

    fn dispatch_help_navigation(&mut self, action: NavigationAction) {
        let last_line = self.config.help_lines().len().saturating_sub(1);
        match action {
            NavigationAction::NextFile | NavigationAction::PreviousFile => {}
            NavigationAction::MoveDown => {
                let help = self.screen.help_mut().unwrap();
                help.offset = (help.offset + 1).min(last_line);
            }
            NavigationAction::MoveUp => {
                let help = self.screen.help_mut().unwrap();
                help.offset = help.offset.saturating_sub(1);
            }
            NavigationAction::PageDown => {
                let help = self.screen.help_mut().unwrap();
                help.offset = (help.offset + 10).min(last_line);
            }
            NavigationAction::PageUp => {
                let help = self.screen.help_mut().unwrap();
                help.offset = help.offset.saturating_sub(10);
            }
            NavigationAction::ScrollRight
            | NavigationAction::ScrollLeft
            | NavigationAction::ScrollStart
            | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
        }
    }

    fn dispatch_log_navigation(
        &mut self,
        action: NavigationAction,
        source: &mut impl HistorySource,
    ) {
        match action {
            NavigationAction::NextFile | NavigationAction::PreviousFile => {}
            NavigationAction::MoveDown => self.move_down(source),
            NavigationAction::MoveUp => self.move_up(source),
            NavigationAction::PageDown => {
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
            NavigationAction::PageUp => {
                let before = self.log.selected;
                self.log.selected = self.log.selected.saturating_sub(10);
                if self.log.selected != before {
                    self.reload_preview(source);
                }
            }
            NavigationAction::ScrollRight
            | NavigationAction::ScrollLeft
            | NavigationAction::ScrollStart
            | NavigationAction::ScrollEnd => self.scroll_horizontal(action),
        }
    }

    fn dispatch_search(&mut self, action: SearchAction, source: &mut impl HistorySource) {
        match self.active_pane() {
            ActivePane::Preview => self.dispatch_preview_search(action),
            ActivePane::Show(focus) => self.dispatch_show_search(action, focus),
            ActivePane::Status(focus) => self.dispatch_status_search(action, focus),
            ActivePane::Help | ActivePane::Log => self.dispatch_log_search(action, source),
        }
    }

    fn dispatch_preview_search(&mut self, action: SearchAction) {
        match action {
            SearchAction::Start => self.input = InputMode::Search(String::new()),
            SearchAction::Next => self.repeat_preview_search(true),
            SearchAction::Previous => self.repeat_preview_search(false),
        }
    }

    fn dispatch_show_search(&mut self, action: SearchAction, focus: ShowFocus) {
        match (focus, action) {
            (ShowFocus::Explorer, SearchAction::Start) => {
                self.status = "Focus the show diff to search".into();
            }
            (ShowFocus::Explorer, SearchAction::Next | SearchAction::Previous) => {}
            (ShowFocus::Diff, SearchAction::Start) => {
                self.input = InputMode::Search(String::new());
            }
            (ShowFocus::Diff, SearchAction::Next) => self.repeat_show_search(true),
            (ShowFocus::Diff, SearchAction::Previous) => self.repeat_show_search(false),
        }
    }

    fn dispatch_status_search(&mut self, action: SearchAction, focus: StatusFocus) {
        match (focus, action) {
            (StatusFocus::Explorer, SearchAction::Start) => {
                self.status = "Focus the status diff to search".into();
            }
            (StatusFocus::Explorer, SearchAction::Next | SearchAction::Previous) => {}
            (StatusFocus::Diff, SearchAction::Start) => {
                self.input = InputMode::Search(String::new());
            }
            (StatusFocus::Diff, SearchAction::Next) => self.repeat_status_search(true),
            (StatusFocus::Diff, SearchAction::Previous) => self.repeat_status_search(false),
        }
    }

    fn dispatch_log_search(&mut self, action: SearchAction, source: &mut impl HistorySource) {
        match action {
            SearchAction::Start => {
                self.screen = Screen::Log;
                self.input = InputMode::Search(String::new());
            }
            SearchAction::Next => self.repeat_search(true, source),
            SearchAction::Previous => self.repeat_search(false, source),
        }
    }

    fn dispatch_global(&mut self, action: GlobalAction, source: &mut impl HistorySource) {
        match action {
            GlobalAction::Command => {
                if matches!(
                    self.active_pane(),
                    ActivePane::Preview | ActivePane::Show(_) | ActivePane::Status(_)
                ) {
                    return;
                }
                self.input = InputMode::Command(String::new());
            }
            GlobalAction::Help => {
                if matches!(
                    self.active_pane(),
                    ActivePane::Preview | ActivePane::Show(_) | ActivePane::Status(_)
                ) {
                    return;
                }
                self.screen = Screen::Help(HelpState::default());
                self.status = "Showing effective configuration".into();
            }
            GlobalAction::OpenEditor => self.request_editor(source),
            GlobalAction::Back => {
                if self.clear_active_search() {
                    self.status.clear();
                    return;
                }
                match self.active_pane() {
                    ActivePane::Preview => {
                        self.log.preview_focused = false;
                        self.input = InputMode::Normal;
                    }
                    ActivePane::Show(ShowFocus::Explorer) => {
                        self.screen = Screen::Log;
                        self.input = InputMode::Normal;
                        self.status.clear();
                    }
                    ActivePane::Show(ShowFocus::Diff) => {
                        let show = self.screen.show_mut().unwrap();
                        show.show_diff_fullscreen = false;
                        show.show_focus = ShowFocus::Explorer;
                        self.input = InputMode::Normal;
                    }
                    ActivePane::Status(StatusFocus::Explorer) => {
                        self.screen = Screen::Log;
                        self.input = InputMode::Normal;
                        self.status.clear();
                    }
                    ActivePane::Status(StatusFocus::Diff) => {
                        let status = self.screen.status_mut().unwrap();
                        status.status_diff_fullscreen = false;
                        status.status_focus = StatusFocus::Explorer;
                        self.input = InputMode::Normal;
                    }
                    ActivePane::Help => {
                        self.screen = Screen::Log;
                        self.input = InputMode::Normal;
                    }
                    ActivePane::Log => {
                        self.running = false;
                        self.status = "Quit requested".into();
                    }
                }
            }
            GlobalAction::Quit => {
                self.running = false;
                self.status = "Quit requested".into();
            }
        }
    }

    fn request_editor(&mut self, source: &mut impl HistorySource) {
        let target = match &self.screen {
            Screen::Show(show) => show.show_selected.and_then(|selected| {
                show.show_files.get(selected).and_then(|file| {
                    file.worktree_path().map(|path| {
                        let line = (show.show_focus == ShowFocus::Diff)
                            .then(|| diff_line_number(&show.show_diff_lines, show.show_diff_offset))
                            .flatten();
                        (path.to_path_buf(), line)
                    })
                })
            }),
            Screen::Status(status) => {
                let files = match status.active_group {
                    StatusGroup::Staged => &status.staged,
                    StatusGroup::Unstaged => &status.unstaged,
                };
                status.selected().and_then(|selected| {
                    files.get(selected).and_then(|file| {
                        file.worktree_path().map(|path| {
                            let line = (status.status_focus == StatusFocus::Diff)
                                .then(|| {
                                    diff_line_number(status.diff_lines(), status.diff_offset())
                                })
                                .flatten();
                            (path, line)
                        })
                    })
                })
            }
            Screen::Log | Screen::Help(_) => None,
        };
        let Some((path, line)) = target else {
            self.status = "Could not open editor: no worktree file is selected".into();
            return;
        };
        let directory = match source.repository_root() {
            Ok(directory) => directory,
            Err(error) => {
                self.status = format!("Could not open editor: {error}");
                return;
            }
        };
        if !directory.join(&path).is_file() {
            self.status = "Could not open editor: selected file is absent from the worktree".into();
            return;
        }
        self.editor_request = Some(EditorRequest::new(
            &self.config.editor_command,
            directory,
            &path,
            line,
        ));
        self.status.clear();
    }

    fn clear_active_search(&mut self) -> bool {
        match self.active_pane() {
            ActivePane::Log => self.log.last_search.take().is_some(),
            ActivePane::Preview => self.log.last_preview_search.take().is_some(),
            ActivePane::Show(ShowFocus::Diff) => self
                .screen
                .show_mut()
                .unwrap()
                .last_show_search
                .take()
                .is_some(),
            ActivePane::Status(StatusFocus::Diff) => self
                .screen
                .status_mut()
                .unwrap()
                .last_search
                .take()
                .is_some(),
            ActivePane::Help
            | ActivePane::Show(ShowFocus::Explorer)
            | ActivePane::Status(StatusFocus::Explorer) => false,
        }
    }

    fn active_pane(&self) -> ActivePane {
        if self.log.preview_focused && matches!(self.screen, Screen::Log) {
            return ActivePane::Preview;
        }
        match &self.screen {
            Screen::Log => ActivePane::Log,
            Screen::Help(_) => ActivePane::Help,
            Screen::Show(show) => ActivePane::Show(show.show_focus),
            Screen::Status(status) => ActivePane::Status(status.status_focus),
        }
    }

    fn dispatch_preview_action(&mut self, action: PreviewAction, source: &mut impl HistorySource) {
        match action {
            PreviewAction::Toggle => {
                self.log.preview_visible = !self.log.preview_visible;
                self.log.preview_focused = false;
                if self.log.preview_visible {
                    self.reload_preview(source);
                }
            }
        }
    }

    fn dispatch_show_action(&mut self, action: ShowAction, source: &mut impl HistorySource) {
        match action {
            ShowAction::Open => self.open_show(source),
        }
    }

    fn dispatch_status_action(&mut self, action: StatusAction, source: &mut impl HistorySource) {
        match action {
            StatusAction::Open => {
                if matches!(self.screen, Screen::Status(_)) {
                    self.screen = Screen::Log;
                    self.input = InputMode::Normal;
                    self.status.clear();
                } else {
                    self.open_status(source);
                }
            }
            StatusAction::SwitchGroup => self.switch_status_group(),
            StatusAction::ToggleStage => self.mutate_status_file(source, false),
            StatusAction::Revert => self.mutate_status_file(source, true),
        }
    }

    fn scroll_horizontal(&mut self, action: NavigationAction) {
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
                Screen::Status(status) => match status.status_focus {
                    StatusFocus::Explorer => &mut status.explorer_horizontal_offset,
                    StatusFocus::Diff => match status.active_group {
                        StatusGroup::Staged => &mut status.staged_diff_horizontal_offset,
                        StatusGroup::Unstaged => &mut status.unstaged_diff_horizontal_offset,
                    },
                },
                Screen::Log => &mut self.log.log_horizontal_offset,
            }
        };
        *offset = match action {
            NavigationAction::ScrollStart => 0,
            NavigationAction::ScrollEnd => maximum,
            NavigationAction::ScrollLeft => offset.saturating_sub(step),
            NavigationAction::ScrollRight => offset.saturating_add(step).min(maximum),
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
            Screen::Status(status) if status.status_focus == StatusFocus::Diff => status
                .diff_lines()
                .iter()
                .map(|line| UnicodeWidthStr::width(crate::git::safe_text(line, false).as_str()))
                .max()
                .unwrap_or(0),
            Screen::Status(status) => status
                .staged()
                .iter()
                .chain(status.unstaged().iter())
                .map(|file| {
                    UnicodeWidthStr::width(crate::git::safe_text(&file.display, false).as_str())
                })
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

    fn move_show_diff(&mut self, amount: usize, forward: bool) {
        {
            let show = self.screen.show_mut().unwrap();
            show.show_diff_offset = if forward {
                (show.show_diff_offset + amount).min(show.show_diff_lines.len().saturating_sub(1))
            } else {
                show.show_diff_offset.saturating_sub(amount)
            };
        }
        self.sync_show_selection_to_diff();
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

    fn status_files(&self, group: StatusGroup) -> &[StatusFile] {
        let status = self.screen.status().expect("status screen is active");
        match group {
            StatusGroup::Staged => status.staged(),
            StatusGroup::Unstaged => status.unstaged(),
        }
    }

    fn status_selected_file(&self) -> Option<StatusFile> {
        let status = self.screen.status()?;
        let files = self.status_files(status.active_group);
        status
            .selected()
            .and_then(|index| files.get(index))
            .cloned()
    }

    fn move_status_down(&mut self) {
        let status = self.screen.status_mut().unwrap();
        match status.active_group {
            StatusGroup::Staged => {
                if let Some(selected) = status.staged_selected {
                    status.staged_selected =
                        Some((selected + 1).min(status.staged.len().saturating_sub(1)));
                    status.staged_explorer_offset = status.staged_selected.unwrap_or(0);
                }
            }
            StatusGroup::Unstaged => {
                if let Some(selected) = status.unstaged_selected {
                    status.unstaged_selected =
                        Some((selected + 1).min(status.unstaged.len().saturating_sub(1)));
                    status.unstaged_explorer_offset = status.unstaged_selected.unwrap_or(0);
                }
            }
        }
    }

    fn move_status_up(&mut self) {
        let status = self.screen.status_mut().unwrap();
        match status.active_group {
            StatusGroup::Staged => {
                if let Some(selected) = status.staged_selected {
                    status.staged_selected = Some(selected.saturating_sub(1));
                    status.staged_explorer_offset = status.staged_selected.unwrap_or(0);
                }
            }
            StatusGroup::Unstaged => {
                if let Some(selected) = status.unstaged_selected {
                    status.unstaged_selected = Some(selected.saturating_sub(1));
                    status.unstaged_explorer_offset = status.unstaged_selected.unwrap_or(0);
                }
            }
        }
    }

    fn page_status_down(&mut self) {
        let status = self.screen.status_mut().unwrap();
        match status.active_group {
            StatusGroup::Staged => {
                if let Some(selected) = status.staged_selected {
                    status.staged_selected =
                        Some((selected + 10).min(status.staged.len().saturating_sub(1)));
                    status.staged_explorer_offset = status.staged_selected.unwrap_or(0);
                }
            }
            StatusGroup::Unstaged => {
                if let Some(selected) = status.unstaged_selected {
                    status.unstaged_selected =
                        Some((selected + 10).min(status.unstaged.len().saturating_sub(1)));
                    status.unstaged_explorer_offset = status.unstaged_selected.unwrap_or(0);
                }
            }
        }
    }

    fn page_status_up(&mut self) {
        let status = self.screen.status_mut().unwrap();
        match status.active_group {
            StatusGroup::Staged => {
                if let Some(selected) = status.staged_selected {
                    status.staged_selected = Some(selected.saturating_sub(10));
                    status.staged_explorer_offset = status.staged_selected.unwrap_or(0);
                }
            }
            StatusGroup::Unstaged => {
                if let Some(selected) = status.unstaged_selected {
                    status.unstaged_selected = Some(selected.saturating_sub(10));
                    status.unstaged_explorer_offset = status.unstaged_selected.unwrap_or(0);
                }
            }
        }
    }

    fn move_status_diff(&mut self, amount: usize, forward: bool) {
        let status = self.screen.status_mut().unwrap();
        let group = status.active_group;
        let length = match group {
            StatusGroup::Staged => status.staged_diff.len(),
            StatusGroup::Unstaged => status.unstaged_diff.len(),
        };
        let current = match group {
            StatusGroup::Staged => status.staged_diff_offset,
            StatusGroup::Unstaged => status.unstaged_diff_offset,
        };
        let next = if forward {
            (current + amount).min(length.saturating_sub(1))
        } else {
            current.saturating_sub(amount)
        };
        match group {
            StatusGroup::Staged => status.staged_diff_offset = next,
            StatusGroup::Unstaged => status.unstaged_diff_offset = next,
        }
        self.sync_status_selection_to_diff();
    }

    fn focus_status_diff(&mut self) {
        let Some(file) = self.status_selected_file() else {
            return;
        };
        let status = self.screen.status_mut().unwrap();
        status.status_focus = StatusFocus::Diff;
        status.staged_diff_horizontal_offset = 0;
        status.unstaged_diff_horizontal_offset = 0;
        let selected = status.selected();
        let offset = selected.and_then(|selected| match status.active_group {
            StatusGroup::Staged => status.staged_patch_index.offset_for(selected),
            StatusGroup::Unstaged => status.unstaged_patch_index.offset_for(selected),
        });
        match offset {
            Some(offset) => {
                match status.active_group {
                    StatusGroup::Staged => status.staged_diff_offset = offset,
                    StatusGroup::Unstaged => status.unstaged_diff_offset = offset,
                }
                self.status.clear();
            }
            None => {
                match status.active_group {
                    StatusGroup::Staged => status.staged_diff_offset = 0,
                    StatusGroup::Unstaged => status.unstaged_diff_offset = 0,
                }
                self.status = format!(
                    "Patch location unavailable for {}",
                    crate::git::safe_text(&file.display, false)
                );
            }
        }
    }

    fn sync_status_selection_to_diff(&mut self) {
        let status = self.screen.status_mut().unwrap();
        let group = status.active_group;
        let offset = status.diff_offset();
        let selected = match group {
            StatusGroup::Staged => status.staged_patch_index.file_at(offset),
            StatusGroup::Unstaged => status.unstaged_patch_index.file_at(offset),
        };
        if let Some(selected) = selected {
            match group {
                StatusGroup::Staged => {
                    status.staged_selected = Some(selected);
                    status.staged_explorer_offset = selected;
                }
                StatusGroup::Unstaged => {
                    status.unstaged_selected = Some(selected);
                    status.unstaged_explorer_offset = selected;
                }
            }
        }
    }

    fn switch_status_group(&mut self) {
        if let ActivePane::Status(StatusFocus::Explorer) = self.active_pane() {
            let status = self.screen.status_mut().unwrap();
            status.active_group = match status.active_group {
                StatusGroup::Staged => StatusGroup::Unstaged,
                StatusGroup::Unstaged => StatusGroup::Staged,
            };
        }
    }

    fn open_status(&mut self, source: &mut impl HistorySource) {
        match source.load_status() {
            Ok(data) => {
                self.screen = Screen::Status(StatusState::from_data(data));
                self.input = InputMode::Normal;
                self.status.clear();
            }
            Err(error) => self.status = format!("Could not load status: {error}"),
        }
    }

    fn replace_status_data(&mut self, data: StatusData) {
        let status = self.screen.status_mut().unwrap();
        let staged_previous = status
            .staged_selected
            .and_then(|index| status.staged.get(index))
            .cloned();
        let unstaged_previous = status
            .unstaged_selected
            .and_then(|index| status.unstaged.get(index))
            .cloned();
        let staged_index = status.staged_selected;
        let unstaged_index = status.unstaged_selected;

        status.staged = data.staged;
        status.unstaged = data.unstaged;
        status.staged_diff = data.staged_diff;
        status.unstaged_diff = data.unstaged_diff;
        status.staged_patch_index =
            PatchIndex::for_status_files(&status.staged_diff, &status.staged);
        status.unstaged_patch_index =
            PatchIndex::for_status_files(&status.unstaged_diff, &status.unstaged);
        status.staged_diff_revision = next_document_revision();
        status.unstaged_diff_revision = next_document_revision();
        status.staged_selected =
            refreshed_selection(&status.staged, staged_previous.as_ref(), staged_index);
        status.unstaged_selected =
            refreshed_selection(&status.unstaged, unstaged_previous.as_ref(), unstaged_index);
        status.staged_explorer_offset = status
            .staged_selected
            .map_or(0, |index| index.min(status.staged_explorer_offset));
        status.unstaged_explorer_offset = status
            .unstaged_selected
            .map_or(0, |index| index.min(status.unstaged_explorer_offset));
        status.staged_diff_offset = 0;
        status.unstaged_diff_offset = 0;
        status.staged_diff_horizontal_offset = 0;
        status.unstaged_diff_horizontal_offset = 0;
    }

    fn mutate_status_file(&mut self, source: &mut impl HistorySource, revert: bool) {
        let Some(status) = self.screen.status() else {
            return;
        };
        let group = status.active_group;
        let Some(previous_file) = self.status_selected_file() else {
            return;
        };
        let data = match source.load_status() {
            Ok(data) => data,
            Err(error) => {
                self.status = format!("Could not refresh status: {error}");
                return;
            }
        };
        self.replace_status_data(data);
        let Some(refreshed_index) = self
            .status_files(group)
            .iter()
            .position(|file| same_status_paths(file, &previous_file))
        else {
            self.status = "Selected status file changed during refresh".into();
            return;
        };
        if let Some(status) = self.screen.status_mut() {
            match group {
                StatusGroup::Staged => status.staged_selected = Some(refreshed_index),
                StatusGroup::Unstaged => status.unstaged_selected = Some(refreshed_index),
            }
        }
        let file = self.status_files(group)[refreshed_index].clone();
        let result = if revert {
            source.revert_file(group == StatusGroup::Staged, &file)
        } else {
            source.toggle_stage(group == StatusGroup::Staged, &file)
        };
        if let Err(error) = result {
            let action = if revert {
                "revert file"
            } else {
                "toggle stage"
            };
            self.status = format!("Could not {action}: {error}");
            return;
        }
        match source.load_status() {
            Ok(data) => {
                let focus_diff = self
                    .screen
                    .status()
                    .is_some_and(|status| status.status_focus == StatusFocus::Diff);
                self.replace_status_data(data);
                if focus_diff {
                    self.focus_status_diff();
                } else {
                    self.status.clear();
                }
            }
            Err(error) => self.status = format!("Could not refresh status: {error}"),
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
        if let Screen::Status(status) = &mut self.screen {
            if status.status_focus == StatusFocus::Diff {
                status.last_search = Some(query);
                self.search_status_diff(forward, true);
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
        self.log.preview_revision = next_document_revision();
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
                let patch_index = PatchIndex::for_changed_files(&data.diff, &data.files);
                self.screen = Screen::Show(ShowState {
                    show_metadata: data.metadata,
                    show_selected: (!data.files.is_empty()).then_some(0),
                    show_files: data.files,
                    show_diff_lines: data.diff,
                    show_patch_index: patch_index,
                    show_diff_revision: next_document_revision(),
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
        let status = if let Some(offset) = show.show_patch_index.offset_for(selected) {
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
        if let Some(selected) = show.show_patch_index.file_at(show.show_diff_offset) {
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

    fn repeat_status_search(&mut self, forward: bool) {
        self.search_status_diff(forward, false);
    }

    fn search_status_diff(&mut self, forward: bool, wrap: bool) {
        let status = self.screen.status_mut().unwrap();
        let Some(query) = status.last_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        let query_lower = query.to_lowercase();
        let length = status.diff_lines().len();
        if length == 0 {
            self.status = format!("No match for `{query}`");
            return;
        }
        let matches = |index: &usize| {
            crate::git::safe_text(&status.diff_lines()[*index], false)
                .to_lowercase()
                .contains(&query_lower)
        };
        let offset = status.diff_offset();
        let index = if forward {
            let later = (offset + 1..length).find(&matches);
            later.or_else(|| {
                wrap.then(|| (0..=offset.min(length - 1)).find(&matches))
                    .flatten()
            })
        } else {
            let earlier = (0..offset).rev().find(&matches);
            earlier.or_else(|| {
                wrap.then(|| (offset..length).rev().find(&matches))
                    .flatten()
            })
        };
        if let Some(index) = index {
            match status.active_group {
                StatusGroup::Staged => status.staged_diff_offset = index,
                StatusGroup::Unstaged => status.unstaged_diff_offset = index,
            }
            self.sync_status_selection_to_diff();
            self.status = format!("Match for `{query}`");
        } else {
            let has_match = !wrap && (0..length).any(|index| matches(&index));
            self.status = if has_match {
                if forward { "(END)" } else { "(TOP)" }.into()
            } else {
                format!("No match for `{query}`")
            };
        }
    }

    fn submit_command(&mut self, command: &str, source: &mut impl HistorySource) {
        let command = command.trim();
        let mut parts = command.split_whitespace();
        match parts.next() {
            Some("help" | "h") if parts.next().is_none() => {
                self.screen = Screen::Help(HelpState::default());
                self.status = "Showing effective configuration".into();
            }
            Some("quit" | "q") if parts.next().is_none() => {
                self.running = false;
                self.status = "Quit requested".into();
            }
            Some("goto" | "gt") => match (parts.next(), parts.next()) {
                (Some(commit), None) => self.goto_commit(commit, source),
                _ => self.status = "Usage: goto <commit>".into(),
            },
            None => self.status = "Command is empty".into(),
            Some(_) => self.status = format!("Unknown command: {command}"),
        }
    }

    fn goto_commit(&mut self, commit: &str, source: &mut impl HistorySource) {
        if !matches!(self.screen, Screen::Log) {
            self.status = "goto is only available in log mode".into();
            return;
        }

        let matches = |record: &CommitRecord| {
            record.id.len() >= commit.len()
                && record.id[..commit.len()].eq_ignore_ascii_case(commit)
        };
        let mut found = self.log.records.iter().position(&matches);
        while found.is_none() && self.log.has_more {
            let previous_length = self.log.records.len();
            if let Err(error) = self.fetch_more(source) {
                self.status = format!("Could not find commit `{commit}`: {error}");
                return;
            }
            found = self.log.records[previous_length..]
                .iter()
                .position(&matches)
                .map(|index| previous_length + index);
        }

        if let Some(index) = found {
            self.log.selected = index;
            self.reload_preview(source);
            self.status = format!("Jumped to commit `{commit}`");
        } else {
            self.status = format!("Commit not found: {commit}");
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

    pub fn status_search_query(&self) -> Option<&str> {
        self.screen
            .status()
            .and_then(|status| status.last_search.as_deref())
    }
}

fn matches_record(record: &CommitRecord, query: &str) -> bool {
    crate::git::safe_text(&record.display, false)
        .to_lowercase()
        .contains(query)
}

fn refreshed_selection(
    files: &[StatusFile],
    previous: Option<&StatusFile>,
    previous_index: Option<usize>,
) -> Option<usize> {
    if files.is_empty() {
        return None;
    }
    previous
        .and_then(|file| {
            files
                .iter()
                .position(|candidate| same_status_paths(candidate, file))
        })
        .or_else(|| previous_index.map(|index| index.min(files.len() - 1)))
        .or(Some(0))
}

fn same_status_paths(left: &StatusFile, right: &StatusFile) -> bool {
    left.old_path == right.old_path && left.new_path == right.new_path
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.submit_command("wat", &mut history);
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

        let mut history = FakeHistory {
            records: records(0),
            fail_at: None,
        };
        app.submit_command("help", &mut history);

        assert_eq!(app.screen, Screen::Help(HelpState::default()));
    }

    #[test]
    fn goto_command_loads_and_selects_a_commit_from_later_history() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(6),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();

        app.submit_command("goto id-4", &mut history);

        assert_eq!(app.log.selected, 4);
        assert_eq!(app.log.records.len(), 6);
        assert_eq!(app.status, "Jumped to commit `id-4`");
    }

    #[test]
    fn goto_alias_accepts_an_id_prefix() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: vec![CommitRecord {
                id: "abcdef123456".into(),
                display: "Subject".into(),
            }],
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();

        app.submit_command("gt ABCDEF", &mut history);

        assert_eq!(app.log.selected, 0);
        assert_eq!(app.status, "Jumped to commit `ABCDEF`");
    }

    #[test]
    fn goto_reports_usage_and_preserves_selection_when_commit_is_missing() {
        let config = Config {
            batch_size: 2,
            ..Config::default()
        };
        let mut app = App::new(config);
        let mut history = FakeHistory {
            records: records(3),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.log.selected = 1;

        app.submit_command("goto", &mut history);
        assert_eq!(app.status, "Usage: goto <commit>");

        app.submit_command("goto missing", &mut history);
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.log.records.len(), 3);
        assert_eq!(app.status, "Commit not found: missing");
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
    fn configured_quit_bindings_win_during_text_entry() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.input = InputMode::Search("query".into());
        app.handle_key(Key::Char('q'), &mut history);
        assert_eq!(app.input, InputMode::Search("queryq".into()));

        let mut config = Config::default();
        config
            .bindings
            .insert(Key::Char('x'), Action::Global(GlobalAction::Quit));
        for key in [Key::Char('x'), Key::Ctrl('c')] {
            let mut app = App::new(config.clone());
            let mut history = FakeHistory {
                records: records(1),
                fail_at: None,
            };
            app.initialize(&mut history).unwrap();
            app.input = InputMode::Search("query".into());
            app.handle_key(key.clone(), &mut history);
            assert!(!app.running);

            let mut app = App::new(config.clone());
            app.input = InputMode::Command("help".into());
            app.handle_key(key, &mut history);
            assert!(!app.running);
        }
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
        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        assert_eq!(app.screen.help().unwrap().offset, 1);
        assert_eq!(app.log.selected, 1);
        app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
        assert!(app.screen.help().unwrap().offset > 1);
        assert_eq!(app.log.selected, 1);
        app.dispatch(Action::Navigation(NavigationAction::PageUp), &mut history);
        assert_eq!(app.screen.help().unwrap().offset, 1);
        app.dispatch(Action::Global(GlobalAction::Back), &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log, log_before);
    }

    #[test]
    fn search_from_help_keeps_the_existing_log_fallback() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();

        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.dispatch(Action::Search(SearchAction::Start), &mut history);

        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.input_label(), Some("/".into()));
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

        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 2);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollLeft),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 2);
        assert_eq!(app.log.selected, 1);

        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 2);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollLeft),
            &mut history,
        );
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollLeft),
            &mut history,
        );
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 0);
        assert_eq!(app.log.selected, 1);

        app.dispatch(Action::Global(GlobalAction::Back), &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.log.preview_lines = vec!["abcdef".into()];
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.log.preview_horizontal_offset, 2);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollLeft),
            &mut history,
        );
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

        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 3);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollStart),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 0);

        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.set_horizontal_viewport_width(8);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        assert!(app.screen.help().unwrap().horizontal_offset > 0);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollStart),
            &mut history,
        );
        assert_eq!(app.screen.help().unwrap().horizontal_offset, 0);

        app.dispatch(Action::Global(GlobalAction::Back), &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.log.preview_lines = vec!["abcdef".into()];
        app.set_horizontal_viewport_width(3);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        assert_eq!(app.log.preview_horizontal_offset, 3);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollStart),
            &mut history,
        );
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

        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 5);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.log.log_horizontal_offset, 10);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollLeft),
            &mut history,
        );
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
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        assert_eq!(app.log.preview_horizontal_offset, 0);

        app.screen = Screen::Help(HelpState {
            horizontal_offset: 4,
            ..HelpState::default()
        });
        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        assert!(app.log.preview_focused);
        assert_eq!(app.log.preview_offset, 1);
        assert_eq!(app.log.selected, 0);

        app.handle_key(Key::Char('/'), &mut history);
        for key in "needle".chars().map(Key::Char).chain([Key::Enter]) {
            app.handle_key(key, &mut history);
        }
        assert_eq!(app.log.preview_offset, 1);
        app.dispatch(Action::Global(GlobalAction::Back), &mut history);
        app.dispatch(Action::Global(GlobalAction::Back), &mut history);
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Preview(PreviewAction::Toggle), &mut history);
        assert!(!app.log.preview_visible);
        assert!(!app.log.preview_focused);
        app.log.preview_visible = true;
        app.screen = Screen::Help(HelpState::default());
        app.handle_key(Key::Enter, &mut history);
        assert!(!app.log.preview_focused);
        app.screen = Screen::Log;
        app.log.preview_focused = true;
        app.dispatch(Action::Global(GlobalAction::Quit), &mut history);
        assert!(!app.running);
    }

    #[test]
    fn global_actions_fall_through_from_focused_screen_dispatchers() {
        let mut preview_app = App::new(Config::default());
        let mut preview_history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        preview_app.initialize(&mut preview_history).unwrap();
        preview_app.handle_key(Key::Enter, &mut preview_history);
        preview_app.dispatch(Action::Show(ShowAction::Open), &mut preview_history);
        assert!(matches!(preview_app.screen, Screen::Show(_)));

        let mut show_app = App::new(Config::default());
        let mut show_history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        show_app.initialize(&mut show_history).unwrap();
        show_app.dispatch(Action::Show(ShowAction::Open), &mut show_history);
        show_app.dispatch(Action::Preview(PreviewAction::Toggle), &mut show_history);
        assert!(!show_app.log.preview_visible);
        assert!(matches!(show_app.screen, Screen::Show(_)));
        show_app.dispatch(Action::Show(ShowAction::Open), &mut show_history);
        assert!(matches!(show_app.screen, Screen::Show(_)));
        show_app.dispatch(Action::Global(GlobalAction::Quit), &mut show_history);
        assert!(!show_app.running);

        let mut help_app = App::new(Config::default());
        let mut help_history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        help_app.initialize(&mut help_history).unwrap();
        help_app.dispatch(Action::Global(GlobalAction::Help), &mut help_history);
        help_app.dispatch(Action::Preview(PreviewAction::Toggle), &mut help_history);
        assert!(!help_app.log.preview_visible);
        assert!(matches!(help_app.screen, Screen::Help(_)));
        help_app.dispatch(Action::Global(GlobalAction::Quit), &mut help_history);
        assert!(!help_app.running);

        let mut help_show_app = App::new(Config::default());
        let mut help_show_history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        help_show_app.initialize(&mut help_show_history).unwrap();
        help_show_app.dispatch(Action::Global(GlobalAction::Help), &mut help_show_history);
        help_show_app.dispatch(Action::Show(ShowAction::Open), &mut help_show_history);
        assert!(matches!(help_show_app.screen, Screen::Show(_)));
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
        app.dispatch(Action::Show(ShowAction::Open), &mut history);

        assert!(matches!(app.screen, Screen::Show(_)));
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(0));
        assert_eq!(app.log.selected, 1);
        assert_eq!(app.status, "");

        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
        assert_eq!(app.log.selected, 1);
        app.handle_key(Key::Enter, &mut history);
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Diff);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 4);

        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
    fn q_and_escape_back_out_or_quit_from_the_same_focused_pane() {
        for key in [Key::Char('q'), Key::Escape] {
            let mut app = App::new(Config::default());
            let mut history = ShowHistory {
                records: records(1),
                result: Ok(show_data()),
            };
            app.initialize(&mut history).unwrap();

            app.handle_key(Key::Enter, &mut history);
            app.handle_key(key.clone(), &mut history);
            assert!(!app.log.preview_focused);
            assert!(app.running);

            app.dispatch(Action::Show(ShowAction::Open), &mut history);
            app.handle_key(Key::Enter, &mut history);
            app.handle_key(key.clone(), &mut history);
            assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Explorer);
            assert!(app.running);

            app.handle_key(key.clone(), &mut history);
            assert_eq!(app.screen, Screen::Log);
            assert!(app.running);

            app.handle_key(key, &mut history);
            assert!(!app.running);
            assert_eq!(app.status, "Quit requested");
        }
    }

    #[test]
    fn back_clears_the_active_search_before_navigating() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();

        app.log.last_search = Some("log".into());
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.log_search_query(), None);
        assert!(app.running);

        app.handle_key(Key::Enter, &mut history);
        app.log.last_preview_search = Some("preview".into());
        app.handle_key(Key::Char('q'), &mut history);
        assert_eq!(app.preview_search_query(), None);
        assert!(app.log.preview_focused);
        app.handle_key(Key::Char('q'), &mut history);

        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.handle_key(Key::Enter, &mut history);
        app.screen.show_mut().unwrap().last_show_search = Some("show".into());
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.show_search_query(), None);
        assert_eq!(app.screen.show().unwrap().show_focus, ShowFocus::Diff);

        app.handle_key(Key::Escape, &mut history);
        app.handle_key(Key::Escape, &mut history);
        app.screen = Screen::Status(StatusState {
            status_focus: StatusFocus::Diff,
            last_search: Some("status".into()),
            ..StatusState::default()
        });
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.status_search_query(), None);
        assert_eq!(app.screen.status().unwrap().status_focus, StatusFocus::Diff);
    }

    #[test]
    fn escape_cancels_search_input_without_clearing_a_submitted_search() {
        let mut app = App::new(Config::default());
        let mut history = FakeHistory {
            records: records(1),
            fail_at: None,
        };
        app.initialize(&mut history).unwrap();
        app.log.last_search = Some("saved".into());

        app.handle_key(Key::Char('/'), &mut history);
        app.handle_key(Key::Char('n'), &mut history);
        app.handle_key(Key::Escape, &mut history);

        assert_eq!(app.input_label(), None);
        assert_eq!(app.log_search_query(), Some("saved"));
    }

    #[test]
    fn back_ignores_searches_in_inactive_panes() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.log.last_search = Some("log".into());

        app.handle_key(Key::Enter, &mut history);
        app.handle_key(Key::Escape, &mut history);
        assert!(!app.log.preview_focused);
        assert_eq!(app.log_search_query(), Some("log"));

        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log_search_query(), Some("log"));

        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log_search_query(), Some("log"));

        app.screen = Screen::Status(StatusState::default());
        app.handle_key(Key::Escape, &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.log_search_query(), Some("log"));
    }

    #[test]
    fn scrolling_show_diff_keeps_explorer_selection_on_current_file() {
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(1),
            result: Ok(show_data()),
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.handle_key(Key::Enter, &mut history);

        for _ in 0..3 {
            app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        }
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 4);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));

        app.dispatch(Action::Navigation(NavigationAction::MoveUp), &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 3);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(0));
        app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
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
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
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
                old_path: Some(format!("file-{index}.rs").into()),
                new_path: Some(format!("file-{index}.rs").into()),
            })
            .collect();
        let mut app = App::new(Config::default());
        let mut history = ShowHistory {
            records: records(2),
            result: Ok(data),
        };
        app.initialize(&mut history).unwrap();
        app.log.selected = 1;
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.set_horizontal_viewport_width(4);

        app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(10));
        assert_eq!(app.screen.show().unwrap().show_explorer_offset, 10);
        app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(20));
        app.dispatch(Action::Navigation(NavigationAction::PageUp), &mut history);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(10));
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
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
        app.dispatch(Action::Show(ShowAction::Open), &mut history);

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
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.handle_key(Key::Enter, &mut history);
        assert!(app.status.contains("Patch location unavailable"));
        app.handle_key(Key::Escape, &mut history);
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
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
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
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
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollRight),
            &mut history,
        );
        assert_eq!(app.screen.show().unwrap().show_diff_horizontal_offset, 2);
        assert_eq!(app.log.log_horizontal_offset, 0);
        app.dispatch(Action::Search(SearchAction::Next), &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 5);
        assert_eq!(app.screen.show().unwrap().show_selected, Some(1));
        app.dispatch(Action::Search(SearchAction::Next), &mut history);
        assert_eq!(app.status, "(END)");
        app.dispatch(Action::Search(SearchAction::Previous), &mut history);
        assert_eq!(app.screen.show().unwrap().show_diff_offset, 3);
        app.dispatch(Action::Search(SearchAction::Previous), &mut history);
        assert_eq!(app.status, "(TOP)");
        app.handle_key(Key::Escape, &mut history);
        app.handle_key(Key::Escape, &mut history);
        app.handle_key(Key::Escape, &mut history);
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
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
        app.dispatch(Action::Show(ShowAction::Open), &mut failing);
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
        app.dispatch(Action::Show(ShowAction::Open), &mut empty);
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
        app.dispatch(Action::Show(ShowAction::Open), &mut empty_log);
        assert_eq!(app.screen, Screen::Log);
        assert_eq!(app.status, "Could not open show: No selected commit");
    }

    #[test]
    fn editor_requests_follow_show_and_status_focus_and_diff_lines() {
        struct RootHistory;

        impl HistorySource for RootHistory {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(Vec::new())
            }

            fn repository_root(&mut self) -> Result<std::path::PathBuf, GitError> {
                Ok(env!("CARGO_MANIFEST_DIR").into())
            }
        }

        let config = Config {
            editor_command: vec![
                "code".into(),
                "-g".into(),
                "--goto".into(),
                "file:line".into(),
            ],
            ..Config::default()
        };
        let diff = vec![
            "diff --git a/src/lib.rs b/src/lib.rs".into(),
            "@@ -10,2 +20,3 @@".into(),
            " context".into(),
            "+added".into(),
        ];
        let file = ChangedFile {
            display: "M src/lib.rs".into(),
            old_path: Some("src/lib.rs".into()),
            new_path: Some("src/lib.rs".into()),
        };
        let mut app = App::new(config);
        let mut history = RootHistory;
        app.screen = Screen::Show(ShowState {
            show_files: vec![file.clone()],
            show_diff_lines: diff.clone(),
            show_patch_index: PatchIndex::for_changed_files(&diff, &[file]),
            show_selected: Some(0),
            ..ShowState::default()
        });

        app.dispatch(Action::Global(GlobalAction::OpenEditor), &mut history);
        assert_editor_request(app.editor_request().unwrap(), "src/lib.rs:1");
        app.take_editor_request();
        {
            let show = app.screen.show_mut().unwrap();
            show.show_focus = ShowFocus::Diff;
            show.show_diff_offset = 3;
        }
        app.dispatch(Action::Global(GlobalAction::OpenEditor), &mut history);
        assert_editor_request(app.editor_request().unwrap(), "src/lib.rs:21");
        app.take_editor_request();

        app.screen = Screen::Status(StatusState::from_data(StatusData {
            unstaged: vec![StatusFile {
                display: " M src/lib.rs".into(),
                old_path: Some(b"src/lib.rs".to_vec()),
                new_path: Some(b"src/lib.rs".to_vec()),
            }],
            unstaged_diff: diff,
            ..StatusData::default()
        }));
        app.screen.status_mut().unwrap().active_group = StatusGroup::Unstaged;
        app.dispatch(Action::Global(GlobalAction::OpenEditor), &mut history);
        assert_editor_request(app.editor_request().unwrap(), "src/lib.rs:1");
        app.take_editor_request();
        {
            let status = app.screen.status_mut().unwrap();
            status.status_focus = StatusFocus::Diff;
            status.unstaged_diff_offset = 3;
        }
        app.dispatch(Action::Global(GlobalAction::OpenEditor), &mut history);
        assert_editor_request(app.editor_request().unwrap(), "src/lib.rs:21");
        app.take_editor_request();

        let status = app.screen.status_mut().unwrap();
        status.status_focus = StatusFocus::Explorer;
        status.unstaged[0].new_path = Some(b"missing.rs".to_vec());
        app.dispatch(Action::Global(GlobalAction::OpenEditor), &mut history);
        assert!(app.editor_request().is_none());
        assert!(app.status.contains("absent from the worktree"));
    }

    fn assert_editor_request(request: &EditorRequest, target: &str) {
        assert_eq!(
            request.directory(),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        );
        let command = request
            .command()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(command, ["code", "-g", "--goto", target]);
    }

    #[test]
    fn successful_editor_return_refreshes_status_data() {
        struct RefreshHistory;

        impl HistorySource for RefreshHistory {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(Vec::new())
            }

            fn load_status(&mut self) -> Result<StatusData, GitError> {
                Ok(StatusData {
                    unstaged: vec![StatusFile {
                        display: " M refreshed.rs".into(),
                        old_path: Some(b"refreshed.rs".to_vec()),
                        new_path: Some(b"refreshed.rs".to_vec()),
                    }],
                    unstaged_diff: vec![
                        "diff --git a/refreshed.rs b/refreshed.rs".into(),
                        "@@ -1 +1 @@".into(),
                        "+refreshed".into(),
                    ],
                    ..StatusData::default()
                })
            }
        }

        let mut app = App::new(Config::default());
        app.screen = Screen::Status(StatusState::from_data(StatusData {
            unstaged: vec![StatusFile {
                display: " M stale.rs".into(),
                old_path: Some(b"stale.rs".to_vec()),
                new_path: Some(b"stale.rs".to_vec()),
            }],
            unstaged_diff: vec!["stale".into()],
            ..StatusData::default()
        }));
        app.finish_editor(Ok(()), &mut RefreshHistory);

        let status = app.screen.status().unwrap();
        assert_eq!(status.unstaged()[0].display, " M refreshed.rs");
        assert_eq!(status.unstaged_diff().last().unwrap(), "+refreshed");
    }

    #[test]
    fn failed_status_refresh_does_not_mutate_a_stale_selection() {
        struct StatusRefreshFailure {
            mutations: usize,
        }

        impl HistorySource for StatusRefreshFailure {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(records(1))
            }

            fn load_status(&mut self) -> Result<StatusData, GitError> {
                Err(GitError::Status {
                    stage: "discovery".into(),
                    reason: "planned refresh failure".into(),
                })
            }

            fn toggle_stage(&mut self, _staged: bool, _file: &StatusFile) -> Result<(), GitError> {
                self.mutations += 1;
                Ok(())
            }
        }

        let mut app = App::new(Config::default());
        let mut history = StatusRefreshFailure { mutations: 0 };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::Status(StatusAction::Open), &mut history);
        assert_eq!(app.screen, Screen::Log);
        assert!(app.status.contains("planned refresh failure"));
        app.screen = Screen::Status(StatusState::from_data(StatusData {
            unstaged: vec![StatusFile {
                display: " M stale.txt".into(),
                old_path: Some(b"stale.txt".to_vec()),
                new_path: Some(b"stale.txt".to_vec()),
            }],
            ..StatusData::default()
        }));

        app.dispatch(Action::Status(StatusAction::ToggleStage), &mut history);

        assert_eq!(history.mutations, 0);
        assert!(app.status.contains("planned refresh failure"));
        assert_eq!(app.screen.status().unwrap().selected(), Some(0));
    }

    #[test]
    fn changed_status_selection_during_refresh_does_not_mutate_another_file() {
        struct StatusSelectionChanged {
            mutations: usize,
        }

        impl HistorySource for StatusSelectionChanged {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(records(1))
            }

            fn load_status(&mut self) -> Result<StatusData, GitError> {
                Ok(StatusData {
                    unstaged: vec![StatusFile {
                        display: " M replacement.txt".into(),
                        old_path: Some(b"replacement.txt".to_vec()),
                        new_path: Some(b"replacement.txt".to_vec()),
                    }],
                    ..StatusData::default()
                })
            }

            fn toggle_stage(&mut self, _staged: bool, _file: &StatusFile) -> Result<(), GitError> {
                self.mutations += 1;
                Ok(())
            }
        }

        let mut app = App::new(Config::default());
        let mut history = StatusSelectionChanged { mutations: 0 };
        app.initialize(&mut history).unwrap();
        app.screen = Screen::Status(StatusState::from_data(StatusData {
            unstaged: vec![StatusFile {
                display: " M selected.txt".into(),
                old_path: Some(b"selected.txt".to_vec()),
                new_path: Some(b"selected.txt".to_vec()),
            }],
            ..StatusData::default()
        }));

        app.dispatch(Action::Status(StatusAction::ToggleStage), &mut history);

        assert_eq!(history.mutations, 0);
        assert_eq!(app.status, "Selected status file changed during refresh");
        assert_eq!(app.screen.status().unwrap().selected(), Some(0));
    }
}
