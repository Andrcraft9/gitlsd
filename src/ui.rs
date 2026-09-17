//! Interactive terminal frontend.
//!
//! This module translates Crossterm events into shared keys, drives [`App`],
//! and renders log, preview, help, show, and status state with Ratatui. It also owns
//! setup, suspension, and restoration of the terminal session, including while
//! a configured editor owns the terminal. Per-document render caches keep Ratatui
//! styling local to terminal-visible rows across redraws.

use std::io::{self, Stdout, Write};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Screen, ShowFocus, ShowState, StatusFocus, StatusGroup, StatusState};
use crate::config::Key;
use crate::git::HistorySource;

pub fn run(app: &mut App, source: &mut impl HistorySource) -> io::Result<()> {
    let mut session = TerminalSession::start()?;
    let mut log_state = ListState::default();
    let mut show_state = ListState::default();
    let mut status_states = [ListState::default(), ListState::default()];
    let mut caches = RenderCaches::default();
    while app.is_running() {
        session.terminal.draw(|frame| {
            render_with_states(
                frame,
                app,
                &mut log_state,
                &mut show_state,
                &mut status_states,
                &mut caches,
            )
        })?;
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind != KeyEventKind::Release => {
                    if let Some(key) = translate_key(key_event) {
                        let size = session.terminal.size()?;
                        app.set_horizontal_viewport_width(active_content_width(
                            ratatui::layout::Rect::new(0, 0, size.width, size.height),
                            app,
                        ));
                        app.handle_key(key, source);
                        if let Some(request) = app.take_editor_request() {
                            session.suspend()?;
                            let result = request.run();
                            session.resume()?;
                            app.finish_editor(result, source);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = enter_terminal(&mut stdout) {
            let _ = disable_raw_mode();
            let _ = restore_terminal(&mut stdout);
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = disable_raw_mode();
                let mut stdout = io::stdout();
                let _ = restore_terminal(&mut stdout);
                Err(error)
            }
        }
    }

    fn suspend(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        restore_terminal(self.terminal.backend_mut())?;
        self.terminal.show_cursor()
    }

    fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        enter_terminal(self.terminal.backend_mut())?;
        self.terminal.clear()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = restore_terminal(self.terminal.backend_mut());
        let _ = self.terminal.show_cursor();
    }
}

fn enter_terminal(writer: &mut impl Write) -> io::Result<()> {
    execute!(writer, DisableMouseCapture, EnterAlternateScreen)
}

fn restore_terminal(writer: &mut impl Write) -> io::Result<()> {
    execute!(writer, DisableMouseCapture, LeaveAlternateScreen, Show)
}

fn content_split_direction(content_area: ratatui::layout::Rect) -> Direction {
    // Terminal cells are typically about twice as tall as they are wide, so
    // compare an approximate physical aspect ratio instead of raw cell counts.
    if content_area.width > content_area.height.saturating_mul(2) {
        Direction::Horizontal
    } else {
        Direction::Vertical
    }
}

fn active_content_width(area: ratatui::layout::Rect, app: &App) -> usize {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let pane = match app.screen() {
        Screen::Help(_) => chunks[0],
        Screen::Log if app.log_state().preview_visible() => {
            let panes = Layout::default()
                .direction(content_split_direction(chunks[0]))
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);
            if app.log_state().preview_focused() {
                panes[1]
            } else {
                panes[0]
            }
        }
        Screen::Log => chunks[0],
        Screen::Show(show) => {
            if show.diff_fullscreen() {
                return usize::from(chunks[0].width.saturating_sub(2));
            }
            let panes = Layout::default()
                .direction(content_split_direction(chunks[0]))
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);
            match show.focus() {
                ShowFocus::Explorer => panes[0],
                ShowFocus::Diff => panes[1],
            }
        }
        Screen::Status(status) => {
            if status.diff_fullscreen() {
                return usize::from(chunks[0].width.saturating_sub(2));
            }
            let panes = Layout::default()
                .direction(content_split_direction(chunks[0]))
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);
            match status.focus() {
                StatusFocus::Explorer => panes[0],
                StatusFocus::Diff => panes[1],
            }
        }
    };
    let marker_width = usize::from(
        (matches!(app.screen(), Screen::Log) && !app.log_state().preview_focused())
            || matches!(
                app.screen(),
                Screen::Show(show) if show.focus() == ShowFocus::Explorer
            )
            || matches!(
                app.screen(),
                Screen::Status(status) if status.focus() == StatusFocus::Explorer
            ),
    ) * 2;
    usize::from(pane.width.saturating_sub(2)).saturating_sub(marker_width)
}

#[cfg(test)]
fn render(frame: &mut ratatui::Frame<'_>, app: &App, log_state: &mut ListState) {
    let mut show_state = ListState::default();
    let mut status_states = [ListState::default(), ListState::default()];
    let mut caches = RenderCaches::default();
    render_with_states(
        frame,
        app,
        log_state,
        &mut show_state,
        &mut status_states,
        &mut caches,
    );
}

fn render_with_states(
    frame: &mut ratatui::Frame<'_>,
    app: &App,
    log_state: &mut ListState,
    show_state: &mut ListState,
    status_states: &mut [ListState; 2],
    caches: &mut RenderCaches,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.screen() {
        Screen::Log => {
            let log = app.log_state();
            let items: Vec<_> = log
                .records()
                .iter()
                .map(|record| {
                    ListItem::new(scrolled_line(
                        &record.display,
                        log.log_horizontal_offset(),
                        app.log_search_query(),
                    ))
                })
                .collect();
            let list = List::new(items)
                .block(pane_block(" gitlsd log ", !log.preview_focused()))
                .highlight_symbol("> ")
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            log_state.select(if log.records().is_empty() {
                None
            } else {
                Some(log.selected())
            });
            if log.preview_visible() {
                let panes = Layout::default()
                    .direction(content_split_direction(chunks[0]))
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(chunks[0]);
                frame.render_stateful_widget(list, panes[0], log_state);
                let preview = caches.preview.visible_lines(
                    app.log_state().preview_lines(),
                    app.log_state().preview_revision(),
                    app.preview_search_query(),
                    log.preview_offset(),
                    log.preview_horizontal_offset(),
                    paragraph_height(panes[1]),
                );
                frame.render_widget(
                    Paragraph::new(preview)
                        .scroll((0, 0))
                        .block(pane_block(" preview ", log.preview_focused())),
                    panes[1],
                );
            } else {
                frame.render_stateful_widget(list, chunks[0], log_state);
            }
        }
        Screen::Help(help) => {
            let text = app.help_lines().join("\n");
            frame.render_widget(
                Paragraph::new(text)
                    .scroll((
                        help.offset().try_into().unwrap_or(u16::MAX),
                        help.horizontal_offset().try_into().unwrap_or(u16::MAX),
                    ))
                    .block(Block::default().title(" help ").borders(Borders::ALL)),
                chunks[0],
            );
        }
        Screen::Show(show) => render_show(
            frame,
            show,
            app.show_search_query(),
            chunks[0],
            show_state,
            &mut caches.show_diff,
        ),
        Screen::Status(status) => render_status(
            frame,
            status,
            app.status_search_query(),
            chunks[0],
            status_states,
            caches,
        ),
    }

    let status = app.input_label().unwrap_or_else(|| app.status().to_owned());
    frame.render_widget(Paragraph::new(status), chunks[1]);
}

fn render_show(
    frame: &mut ratatui::Frame<'_>,
    show: &ShowState,
    search_query: Option<&str>,
    area: ratatui::layout::Rect,
    show_state: &mut ListState,
    cache: &mut DocumentCache,
) {
    if show.diff_fullscreen() {
        render_diff(
            frame,
            DiffRender {
                lines: show.diff_lines(),
                revision: show.diff_revision(),
                search_query,
                offset: show.diff_offset(),
                horizontal_offset: show.diff_horizontal_offset(),
            },
            area,
            cache,
        );
        return;
    }

    let panes = Layout::default()
        .direction(content_split_direction(area))
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let explorer = panes[0];
    let metadata_height = show
        .metadata()
        .len()
        .saturating_add(2)
        .min(usize::from(explorer.height / 2))
        .try_into()
        .unwrap_or(u16::MAX);
    let explorer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(metadata_height), Constraint::Min(1)])
        .split(explorer);

    let metadata = show
        .metadata()
        .iter()
        .map(|line| styled_line(line))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(metadata)
            .scroll((
                0,
                show.explorer_horizontal_offset()
                    .try_into()
                    .unwrap_or(u16::MAX),
            ))
            .block(Block::default().title(" commit ").borders(Borders::ALL)),
        explorer_chunks[0],
    );

    let (range, selected, list_offset) = list_window(
        show.files().len(),
        show.selected(),
        show_state.offset(),
        explorer_chunks[1],
    );
    let files = show.files()[range]
        .iter()
        .map(|file| {
            ListItem::new(scrolled_line(
                &file.display,
                show.explorer_horizontal_offset(),
                None,
            ))
        })
        .collect::<Vec<_>>();
    *show_state.offset_mut() = 0;
    show_state.select(selected);
    let files = List::new(files)
        .block(pane_block(" files ", show.focus() == ShowFocus::Explorer))
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(files, explorer_chunks[1], show_state);
    *show_state.offset_mut() = list_offset;

    let diff = cache.visible_lines(
        show.diff_lines(),
        show.diff_revision(),
        search_query,
        show.diff_offset(),
        show.diff_horizontal_offset(),
        paragraph_height(panes[1]),
    );
    frame.render_widget(
        Paragraph::new(diff)
            .scroll((0, 0))
            .block(pane_block(" diff ", show.focus() == ShowFocus::Diff)),
        panes[1],
    );
}

fn render_status(
    frame: &mut ratatui::Frame<'_>,
    status: &StatusState,
    search_query: Option<&str>,
    area: ratatui::layout::Rect,
    status_states: &mut [ListState; 2],
    caches: &mut RenderCaches,
) {
    if status.diff_fullscreen() {
        let cache = match status.group() {
            StatusGroup::Staged => &mut caches.staged_diff,
            StatusGroup::Unstaged => &mut caches.unstaged_diff,
        };
        render_diff(
            frame,
            DiffRender {
                lines: status.diff_lines(),
                revision: status.diff_revision(),
                search_query,
                offset: status.diff_offset(),
                horizontal_offset: status.diff_horizontal_offset(),
            },
            area,
            cache,
        );
        return;
    }

    let panes = Layout::default()
        .direction(content_split_direction(area))
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let explorer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(panes[0]);

    let staged_selected = (status.group() == StatusGroup::Staged)
        .then_some(status.staged_selected())
        .flatten();
    let (staged_range, staged_selected, staged_offset) = list_window(
        status.staged().len(),
        staged_selected,
        status_states[0].offset(),
        explorer_chunks[0],
    );
    let staged_items = status.staged()[staged_range]
        .iter()
        .map(|file| {
            ListItem::new(scrolled_line(
                &file.display,
                status.explorer_horizontal_offset(),
                None,
            ))
        })
        .collect::<Vec<_>>();
    *status_states[0].offset_mut() = 0;
    status_states[0].select(staged_selected);
    let staged = List::new(staged_items)
        .block(pane_block(
            " staged ",
            status.focus() == StatusFocus::Explorer && status.group() == StatusGroup::Staged,
        ))
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(staged, explorer_chunks[0], &mut status_states[0]);
    *status_states[0].offset_mut() = staged_offset;

    let unstaged_selected = (status.group() == StatusGroup::Unstaged)
        .then_some(status.unstaged_selected())
        .flatten();
    let (unstaged_range, unstaged_selected, unstaged_offset) = list_window(
        status.unstaged().len(),
        unstaged_selected,
        status_states[1].offset(),
        explorer_chunks[1],
    );
    let unstaged_items = status.unstaged()[unstaged_range]
        .iter()
        .map(|file| {
            ListItem::new(scrolled_line(
                &file.display,
                status.explorer_horizontal_offset(),
                None,
            ))
        })
        .collect::<Vec<_>>();
    *status_states[1].offset_mut() = 0;
    status_states[1].select(unstaged_selected);
    let unstaged = List::new(unstaged_items)
        .block(pane_block(
            " unstaged ",
            status.focus() == StatusFocus::Explorer && status.group() == StatusGroup::Unstaged,
        ))
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(unstaged, explorer_chunks[1], &mut status_states[1]);
    *status_states[1].offset_mut() = unstaged_offset;

    let cache = match status.group() {
        StatusGroup::Staged => &mut caches.staged_diff,
        StatusGroup::Unstaged => &mut caches.unstaged_diff,
    };
    let diff = cache.visible_lines(
        status.diff_lines(),
        status.diff_revision(),
        search_query,
        status.diff_offset(),
        status.diff_horizontal_offset(),
        paragraph_height(panes[1]),
    );
    frame.render_widget(
        Paragraph::new(diff)
            .scroll((0, 0))
            .block(pane_block(" diff ", status.focus() == StatusFocus::Diff)),
        panes[1],
    );
}

struct DiffRender<'a> {
    lines: &'a [String],
    revision: u64,
    search_query: Option<&'a str>,
    offset: usize,
    horizontal_offset: usize,
}

fn render_diff(
    frame: &mut ratatui::Frame<'_>,
    document: DiffRender<'_>,
    area: ratatui::layout::Rect,
    cache: &mut DocumentCache,
) {
    let diff = cache.visible_lines(
        document.lines,
        document.revision,
        document.search_query,
        document.offset,
        document.horizontal_offset,
        paragraph_height(area),
    );
    frame.render_widget(
        Paragraph::new(diff)
            .scroll((0, 0))
            .block(pane_block(" diff ", true)),
        area,
    );
}

fn pane_block(title: &'static str, focused: bool) -> Block<'static> {
    let block = Block::default().title(title).borders(Borders::ALL);
    if focused {
        block.border_style(Style::default().fg(Color::Cyan))
    } else {
        block
    }
}

const WINDOW_OVERSCAN: usize = 2;

#[derive(Default)]
struct RenderCaches {
    preview: DocumentCache,
    show_diff: DocumentCache,
    staged_diff: DocumentCache,
    unstaged_diff: DocumentCache,
}

/// UI-local cache for one independently loaded document and search query.
#[derive(Default)]
struct DocumentCache {
    revision: Option<u64>,
    query: Option<String>,
    lines: Vec<Option<Line<'static>>>,
    #[cfg(test)]
    parsed_lines: usize,
}

impl DocumentCache {
    fn visible_lines(
        &mut self,
        source: &[String],
        revision: u64,
        query: Option<&str>,
        offset: usize,
        horizontal_offset: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let query = query.filter(|query| !query.is_empty());
        if self.revision != Some(revision)
            || self.query.as_deref() != query
            || self.lines.len() != source.len()
        {
            self.revision = Some(revision);
            self.query = query.map(str::to_owned);
            self.lines = vec![None; source.len()];
        }

        if height == 0 {
            return Vec::new();
        }

        let start = offset.min(source.len());
        let end = start
            .saturating_add(height.saturating_add(WINDOW_OVERSCAN))
            .min(source.len());
        (start..end)
            .map(|index| {
                let line = self.lines[index].get_or_insert_with(|| {
                    #[cfg(test)]
                    {
                        self.parsed_lines += 1;
                    }
                    owned_line(highlighted_line(&source[index], query))
                });
                cropped_styled_line(line, horizontal_offset)
            })
            .collect()
    }
}

fn paragraph_height(area: ratatui::layout::Rect) -> usize {
    usize::from(area.height.saturating_sub(2))
}

fn list_window(
    length: usize,
    selected: Option<usize>,
    previous_offset: usize,
    area: ratatui::layout::Rect,
) -> (std::ops::Range<usize>, Option<usize>, usize) {
    let height = paragraph_height(area).max(1);
    let mut start = previous_offset.min(length.saturating_sub(height));
    if let Some(selected) = selected {
        if selected < start {
            start = selected;
        } else if selected >= start.saturating_add(height) {
            start = selected.saturating_add(1).saturating_sub(height);
        }
    }
    let end = start.saturating_add(height).min(length);
    (
        start..end,
        selected.map(|index| index.saturating_sub(start)),
        start,
    )
}

fn owned_line(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), span.style))
            .collect::<Vec<_>>(),
    )
}

fn cropped_styled_line(line: &Line<'_>, offset: usize) -> Line<'static> {
    if offset == 0 {
        return owned_line(line.clone());
    }
    let mut remaining = offset;
    let mut spans = Vec::new();
    for span in &line.spans {
        let mut content = String::new();
        for grapheme in span.content.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme);
            if remaining >= width {
                remaining -= width;
            } else if remaining > 0 {
                remaining = 0;
            } else {
                content.push_str(grapheme);
            }
        }
        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }
    }
    Line::from(spans)
}

fn styled_line(text: &str) -> Line<'_> {
    let mut spans = Vec::new();
    let mut style = Style::default();
    let mut remainder = text;
    while let Some(start) = remainder.find("\x1b[") {
        spans.push(Span::styled(&remainder[..start], style));
        let sequence = &remainder[start + 2..];
        let Some(end) = sequence.find('m') else {
            break;
        };
        let values: Vec<u16> = sequence[..end]
            .split(';')
            .map(|value| value.parse().unwrap_or(0))
            .collect();
        let mut values = values.into_iter();
        while let Some(value) = values.next() {
            match value {
                0 => style = Style::default(),
                1 => style = style.add_modifier(Modifier::BOLD),
                2 => style = style.add_modifier(Modifier::DIM),
                3 => style = style.add_modifier(Modifier::ITALIC),
                4 => style = style.add_modifier(Modifier::UNDERLINED),
                5 => style = style.add_modifier(Modifier::SLOW_BLINK),
                7 => style = style.add_modifier(Modifier::REVERSED),
                8 => style = style.add_modifier(Modifier::HIDDEN),
                9 => style = style.add_modifier(Modifier::CROSSED_OUT),
                22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
                23 => style = style.remove_modifier(Modifier::ITALIC),
                24 => style = style.remove_modifier(Modifier::UNDERLINED),
                25 => style = style.remove_modifier(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
                27 => style = style.remove_modifier(Modifier::REVERSED),
                28 => style = style.remove_modifier(Modifier::HIDDEN),
                29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
                30..=37 => style = style.fg(Color::Indexed((value - 30) as u8)),
                40..=47 => style = style.bg(Color::Indexed((value - 40) as u8)),
                90..=97 => style = style.fg(Color::Indexed((value - 90 + 8) as u8)),
                100..=107 => style = style.bg(Color::Indexed((value - 100 + 8) as u8)),
                39 => style = style.fg(Color::Reset),
                49 => style = style.bg(Color::Reset),
                38 | 48 => {
                    let color = match values.next() {
                        Some(5) => values
                            .next()
                            .and_then(|n| u8::try_from(n).ok())
                            .map(Color::Indexed),
                        Some(2) => match (values.next(), values.next(), values.next()) {
                            (Some(r), Some(g), Some(b)) if r <= 255 && g <= 255 && b <= 255 => {
                                Some(Color::Rgb(r as u8, g as u8, b as u8))
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(color) = color {
                        style = if value == 38 {
                            style.fg(color)
                        } else {
                            style.bg(color)
                        };
                    }
                }
                _ => {}
            }
        }
        remainder = &sequence[end + 1..];
    }
    spans.push(Span::styled(remainder, style));
    Line::from(spans)
}

fn highlighted_line<'a>(text: &'a str, query: Option<&str>) -> Line<'a> {
    let mut line = styled_line(text);
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return line;
    };
    let plain = line.to_string();
    let ranges = case_insensitive_match_ranges(&plain, query);
    if ranges.is_empty() {
        return line;
    }

    let match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::HIDDEN | Modifier::REVERSED);
    let mut offset = 0;
    let mut spans = Vec::new();
    for span in line.spans.drain(..) {
        let span_start = offset;
        let span_end = span_start + span.content.len();
        let mut boundaries = vec![span_start, span_end];
        for range in &ranges {
            if range.start > span_start && range.start < span_end {
                boundaries.push(range.start);
            }
            if range.end > span_start && range.end < span_end {
                boundaries.push(range.end);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1];
            let content = &span.content[start - span_start..end - span_start];
            let style = if ranges
                .iter()
                .any(|range| start >= range.start && end <= range.end)
            {
                span.style.patch(match_style)
            } else {
                span.style
            };
            spans.push(Span::styled(content.to_owned(), style));
        }
        offset = span_end;
    }
    Line::from(spans)
}

fn case_insensitive_match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut folded = String::new();
    let mut source_ranges = Vec::new();
    for (start, character) in text.char_indices() {
        let source = start..start + character.len_utf8();
        for folded_character in character.to_lowercase() {
            let folded_start = folded.len();
            folded.push(folded_character);
            source_ranges.push((folded_start..folded.len(), source.clone()));
        }
    }

    let mut ranges = folded
        .char_indices()
        .filter(|(start, _)| folded[*start..].starts_with(&query))
        .filter_map(|(start, _)| {
            let end = start + query.len();
            let source_start = source_ranges
                .iter()
                .find(|(folded, _)| folded.start <= start && start < folded.end)?
                .1
                .start;
            let source_end = source_ranges
                .iter()
                .rev()
                .find(|(folded, _)| folded.start < end && end <= folded.end)?
                .1
                .end;
            Some(source_start..source_end)
        })
        .map(|range| {
            let start = text
                .grapheme_indices(true)
                .find(|(start, grapheme)| {
                    *start <= range.start && range.start < *start + grapheme.len()
                })
                .map(|(start, _)| start)
                .unwrap_or(range.start);
            let end = text
                .grapheme_indices(true)
                .find(|(start, grapheme)| {
                    *start < range.end && range.end <= *start + grapheme.len()
                })
                .map(|(start, grapheme)| start + grapheme.len())
                .unwrap_or(range.end);
            start..end
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| range.start);
    ranges.into_iter().fold(Vec::new(), |mut merged, range| {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
        merged
    })
}

fn scrolled_line<'a>(text: &'a str, offset: usize, query: Option<&str>) -> Line<'a> {
    if offset == 0 {
        return highlighted_line(text, query);
    }
    let line = highlighted_line(text, query);
    let mut remaining = offset;
    let mut spans = Vec::new();
    for span in line.spans {
        let mut content = String::new();
        for grapheme in span.content.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme);
            if remaining >= width {
                remaining -= width;
            } else if remaining > 0 {
                remaining = 0;
            } else {
                content.push_str(grapheme);
            }
        }
        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }
    }
    Line::from(spans)
}

fn translate_key(event: KeyEvent) -> Option<Key> {
    if event.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL)
        && let KeyCode::Char(character) = event.code
    {
        return Some(Key::Ctrl(character));
    }
    match event.code {
        KeyCode::Char(character) => Some(Key::Char(character)),
        KeyCode::Up if event.modifiers == KeyModifiers::SHIFT => Some(Key::ShiftUp),
        KeyCode::Down if event.modifiers == KeyModifiers::SHIFT => Some(Key::ShiftDown),
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::Left => Some(Key::Left),
        KeyCode::Right => Some(Key::Right),
        KeyCode::Esc => Some(Key::Escape),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Tab => Some(Key::Tab),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;

    use crate::config::{
        Action, Config, GlobalAction, Key, NavigationAction, PreviewAction, ShowAction,
        StatusAction,
    };
    use crate::git::{
        ChangedFile, CommitRecord, GitError, HistorySource, ShowData, StatusData, StatusFile,
    };

    use super::*;

    struct FixtureHistory {
        records: Vec<CommitRecord>,
        preview: Vec<String>,
        show: ShowData,
        status: StatusData,
        next_status: Option<StatusData>,
    }

    impl HistorySource for FixtureHistory {
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

        fn load_show(&mut self, _id: &str) -> Result<ShowData, GitError> {
            Ok(self.show.clone())
        }

        fn load_status(&mut self) -> Result<StatusData, GitError> {
            Ok(self.status.clone())
        }

        fn toggle_stage(&mut self, _staged: bool, _file: &StatusFile) -> Result<(), GitError> {
            if let Some(status) = self.next_status.take() {
                self.status = status;
            }
            Ok(())
        }
    }

    fn app_with_history(
        records: Vec<CommitRecord>,
        preview: Vec<String>,
        show: ShowData,
    ) -> (App, FixtureHistory) {
        let mut app = App::new(Config::default());
        let mut history = FixtureHistory {
            records,
            preview,
            show,
            status: StatusData::default(),
            next_status: None,
        };
        app.initialize(&mut history).unwrap();
        (app, history)
    }

    fn show_data() -> ShowData {
        ShowData {
            metadata: vec!["commit full-id".into(), "Author: Test".into()],
            files: vec![ChangedFile {
                display: "M src/lib.rs".into(),
                old_path: Some("src/lib.rs".into()),
                new_path: Some("src/lib.rs".into()),
            }],
            diff: vec!["diff --git a/src/lib.rs b/src/lib.rs".into()],
        }
    }

    #[test]
    fn renders_git_sgr_as_styles_without_escape_text() {
        let line = styled_line("\x1b[31;1mred\x1b[m plain\x1b[38;2;1;2;3m rgb");
        assert_eq!(line.to_string(), "red plain rgb");
        assert_eq!(line.spans[1].style.fg, Some(Color::Indexed(1)));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            line.spans.last().unwrap().style.fg,
            Some(Color::Rgb(1, 2, 3))
        );
    }

    #[test]
    fn translates_supported_terminal_keys_and_ignores_unsupported_keys() {
        let cases = [
            (KeyCode::Char('j'), KeyModifiers::NONE, Some(Key::Char('j'))),
            (KeyCode::Up, KeyModifiers::NONE, Some(Key::Up)),
            (KeyCode::Down, KeyModifiers::NONE, Some(Key::Down)),
            (KeyCode::Up, KeyModifiers::SHIFT, Some(Key::ShiftUp)),
            (KeyCode::Down, KeyModifiers::SHIFT, Some(Key::ShiftDown)),
            (KeyCode::PageUp, KeyModifiers::NONE, Some(Key::PageUp)),
            (KeyCode::PageDown, KeyModifiers::NONE, Some(Key::PageDown)),
            (KeyCode::Home, KeyModifiers::NONE, Some(Key::Home)),
            (KeyCode::End, KeyModifiers::NONE, Some(Key::End)),
            (KeyCode::Left, KeyModifiers::NONE, Some(Key::Left)),
            (KeyCode::Right, KeyModifiers::NONE, Some(Key::Right)),
            (KeyCode::Esc, KeyModifiers::NONE, Some(Key::Escape)),
            (KeyCode::Enter, KeyModifiers::NONE, Some(Key::Enter)),
            (KeyCode::Backspace, KeyModifiers::NONE, Some(Key::Backspace)),
            (KeyCode::Tab, KeyModifiers::NONE, Some(Key::Tab)),
            (
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                Some(Key::Ctrl('c')),
            ),
            (KeyCode::Char('j'), KeyModifiers::ALT, None),
            (
                KeyCode::Char('c'),
                KeyModifiers::ALT | KeyModifiers::CONTROL,
                None,
            ),
            (KeyCode::F(1), KeyModifiers::NONE, None),
        ];
        for (code, modifiers, expected) in cases {
            assert_eq!(translate_key(KeyEvent::new(code, modifiers)), expected);
        }
    }

    #[test]
    fn terminal_lifecycle_disables_mouse_capture_without_enabling_it() {
        let mut output = Vec::new();
        enter_terminal(&mut output).unwrap();
        restore_terminal(&mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(
            output
                .starts_with("\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1049h")
        );
        assert!(output.contains("\x1b[?1049l\x1b[?25h"));
        assert!(!output.contains("\x1b[?1000h"));
        assert!(!output.contains("\x1b[?1002h"));
        assert!(!output.contains("\x1b[?1003h"));
        assert!(!output.contains("\x1b[?1015h"));
        assert!(!output.contains("\x1b[?1006h"));
    }

    #[test]
    fn preview_layout_uses_content_area_cell_aspect_ratio() {
        assert_eq!(
            content_split_direction(ratatui::layout::Rect::new(0, 0, 180, 100)),
            Direction::Vertical
        );
        assert_eq!(
            content_split_direction(ratatui::layout::Rect::new(0, 0, 200, 100)),
            Direction::Vertical
        );
        assert_eq!(
            content_split_direction(ratatui::layout::Rect::new(0, 0, 201, 100)),
            Direction::Horizontal
        );
    }

    #[test]
    fn active_width_tracks_each_show_pane_and_explorer_marker() {
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            Vec::new(),
            show_data(),
        );
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        for area in [
            ratatui::layout::Rect::new(0, 0, 201, 101),
            ratatui::layout::Rect::new(0, 0, 40, 101),
        ] {
            let content = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area)[0];
            let panes = Layout::default()
                .direction(content_split_direction(content))
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(content);

            let explorer_width = usize::from(panes[0].width.saturating_sub(2)).saturating_sub(2);
            assert_eq!(active_content_width(area, &app), explorer_width);

            app.handle_key(Key::Enter, &mut history);
            let diff_width = usize::from(panes[1].width.saturating_sub(2));
            assert_eq!(active_content_width(area, &app), diff_width);

            app.handle_key(Key::Enter, &mut history);
            assert_eq!(
                active_content_width(area, &app),
                usize::from(content.width.saturating_sub(2))
            );
            app.handle_key(Key::Escape, &mut history);
        }
    }

    #[test]
    fn renders_log_selection_and_status() {
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "abc1234 2026-09-05 Author Rendered subject".into(),
            }],
            Vec::new(),
            ShowData::default(),
        );
        app.dispatch(Action::Preview(PreviewAction::Toggle), &mut history);
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("gitlsd log"));
        assert!(text.contains("abc1234 2026-09-05 Author"));
        assert!(text.contains("Rendered subject"));
        assert!(!text.contains("Loaded"));
        assert!(
            terminal
                .backend()
                .buffer()
                .cell((1, 1))
                .expect("selected row")
                .style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn renders_log_at_horizontal_offset() {
        let line = "prefix-hidden visible-content";
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: line.into(),
            }],
            Vec::new(),
            ShowData::default(),
        );
        app.dispatch(Action::Preview(PreviewAction::Toggle), &mut history);
        app.set_horizontal_viewport_width("visible-content".len());
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("visible-content"));
        assert!(!text.contains("prefix-hidden"));
    }

    #[test]
    fn renders_log_at_grapheme_display_width_and_preserves_ansi_selected_style() {
        let line = "\x1b[31m界e\u{301} visible\x1b[m";
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: line.into(),
            }],
            Vec::new(),
            ShowData::default(),
        );
        app.dispatch(Action::Preview(PreviewAction::Toggle), &mut history);
        app.set_horizontal_viewport_width(8);
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("visible"));
        assert!(!text.contains("界e\u{301}"));
        let cell = terminal
            .backend()
            .buffer()
            .cell((3, 1))
            .expect("visible selected row");
        assert_eq!(cell.style().fg, Some(Color::Indexed(1)));
        assert!(cell.style().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn horizontal_end_skips_a_wide_grapheme_it_cannot_partially_show() {
        assert_eq!(scrolled_line("界a", 1, None).to_string(), "a");
    }

    #[test]
    fn highlights_every_case_insensitive_match_while_preserving_git_style() {
        let line = highlighted_line("\x1b[31mFix\x1b[m and fix", Some("fIx"));
        let highlighted = line
            .spans
            .iter()
            .filter(|span| span.style.bg == Some(Color::Yellow))
            .collect::<Vec<_>>();
        assert_eq!(highlighted.len(), 2);
        assert_eq!(highlighted[0].content, "Fix");
        assert_eq!(highlighted[0].style.fg, Some(Color::Black));
        assert_eq!(highlighted[1].content, "fix");
        assert!(
            highlighted
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn highlights_matches_crossing_ansi_boundaries_and_survives_scrolling() {
        let line = scrolled_line(
            "hidden \x1b[31mse\x1b[32march\x1b[m visible",
            7,
            Some("SEARCH"),
        );
        assert_eq!(line.to_string(), "search visible");
        let highlighted = line
            .spans
            .iter()
            .filter(|span| span.style.bg == Some(Color::Yellow))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(highlighted, "search");
    }

    #[test]
    fn highlights_overlapping_matches_and_whole_graphemes() {
        assert_eq!(case_insensitive_match_ranges("aaa", "aa"), vec![0..3]);

        let line = highlighted_line("e\u{301}lan", Some("\u{301}"));
        let highlighted = line
            .spans
            .iter()
            .find(|span| span.style.bg == Some(Color::Yellow))
            .expect("highlighted grapheme");
        assert_eq!(highlighted.content, "e\u{301}");
    }

    #[test]
    fn highlight_overrides_visibility_conflicts_from_git_styles() {
        let line = highlighted_line("\x1b[7;8mhidden\x1b[m", Some("hidden"));
        let style = line.spans[0].style;
        assert!(!style.add_modifier.contains(Modifier::HIDDEN));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(style.fg, Some(Color::Black));
        assert_eq!(style.bg, Some(Color::Yellow));
    }

    #[test]
    fn document_cache_parses_only_visible_rows_and_rekeys_on_change() {
        let source = (0..100)
            .map(|index| format!("row {index}"))
            .collect::<Vec<_>>();
        let mut cache = DocumentCache::default();

        let first = cache.visible_lines(&source, 1, None, 40, 0, 3);
        assert_eq!(first.len(), 5);
        assert_eq!(cache.parsed_lines, 5);
        cache.visible_lines(&source, 1, None, 40, 0, 3);
        assert_eq!(cache.parsed_lines, 5);
        cache.visible_lines(&source, 1, None, 41, 0, 3);
        assert_eq!(cache.parsed_lines, 6);
        cache.visible_lines(&source, 1, Some("row"), 41, 0, 3);
        assert_eq!(cache.parsed_lines, 11);
        cache.visible_lines(&source, 2, Some("row"), 41, 0, 3);
        assert_eq!(cache.parsed_lines, 16);
        assert!(
            cache
                .visible_lines(&source, 2, Some("row"), 41, 0, 0)
                .is_empty()
        );
    }

    #[test]
    fn document_cache_preserves_styling_when_cropping_highlighted_rows() {
        let source = vec!["hidden \x1b[31mse\x1b[32march\x1b[m visible".into()];
        let mut cache = DocumentCache::default();
        let line = cache
            .visible_lines(&source, 1, Some("SEARCH"), 0, 7, 1)
            .pop()
            .expect("visible line");
        assert_eq!(line.to_string(), "search visible");
        let highlighted = line
            .spans
            .iter()
            .filter(|span| span.style.bg == Some(Color::Yellow))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(highlighted, "search");
        assert_eq!(line.spans[0].style.fg, Some(Color::Black));
    }

    #[test]
    fn persistent_preview_cache_replaces_rows_after_reload() {
        let (mut app, mut history) = app_with_history(
            vec![
                CommitRecord {
                    id: "one".into(),
                    display: "one".into(),
                },
                CommitRecord {
                    id: "two".into(),
                    display: "two".into(),
                },
            ],
            vec!["first preview".into()],
            ShowData::default(),
        );
        let mut log_state = ListState::default();
        let mut show_state = ListState::default();
        let mut status_states = [ListState::default(), ListState::default()];
        let mut caches = RenderCaches::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal
            .draw(|frame| {
                render_with_states(
                    frame,
                    &app,
                    &mut log_state,
                    &mut show_state,
                    &mut status_states,
                    &mut caches,
                )
            })
            .unwrap();

        history.preview = vec!["second preview".into()];
        app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        terminal
            .draw(|frame| {
                render_with_states(
                    frame,
                    &app,
                    &mut log_state,
                    &mut show_state,
                    &mut status_states,
                    &mut caches,
                )
            })
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("second preview"));
        assert!(!text.contains("first preview"));
    }

    #[test]
    fn persistent_status_cache_replaces_rows_after_refresh() {
        let file = StatusFile {
            display: " M file.rs".into(),
            old_path: Some(b"file.rs".to_vec()),
            new_path: Some(b"file.rs".to_vec()),
        };
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "one".into(),
                display: "one".into(),
            }],
            Vec::new(),
            ShowData::default(),
        );
        history.status = StatusData {
            unstaged: vec![file.clone()],
            unstaged_diff: vec!["first status diff".into()],
            ..StatusData::default()
        };
        history.next_status = Some(StatusData {
            unstaged: vec![file],
            unstaged_diff: vec!["second status diff".into()],
            ..StatusData::default()
        });
        app.dispatch(Action::Status(StatusAction::Open), &mut history);
        let mut log_state = ListState::default();
        let mut show_state = ListState::default();
        let mut status_states = [ListState::default(), ListState::default()];
        let mut caches = RenderCaches::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal
            .draw(|frame| {
                render_with_states(
                    frame,
                    &app,
                    &mut log_state,
                    &mut show_state,
                    &mut status_states,
                    &mut caches,
                )
            })
            .unwrap();

        app.dispatch(Action::Status(StatusAction::ToggleStage), &mut history);
        terminal
            .draw(|frame| {
                render_with_states(
                    frame,
                    &app,
                    &mut log_state,
                    &mut show_state,
                    &mut status_states,
                    &mut caches,
                )
            })
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("second status diff"));
        assert!(!text.contains("first status diff"));
    }

    #[test]
    fn list_window_bounds_explorer_work_to_the_viewport() {
        let area = ratatui::layout::Rect::new(0, 0, 20, 8);
        let (range, selected, offset) = list_window(100, Some(99), 0, area);
        assert_eq!(range, 94..100);
        assert_eq!(selected, Some(5));
        assert_eq!(offset, 94);
    }

    #[test]
    fn renders_show_diff_at_a_large_usize_offset() {
        let mut show = show_data();
        show.diff = (0..70_000)
            .map(|index| format!("source row {index}"))
            .collect();
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            Vec::new(),
            show,
        );
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.handle_key(Key::Enter, &mut history);
        for _ in 0..6_553 {
            app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
        }
        for _ in 0..6 {
            app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        }

        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("source row 65536"));
        assert!(!text.contains("source row 65535"));
    }

    #[test]
    fn renders_preview_in_both_orientations_and_marks_focus() {
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "id".into(),
                display: "log row".into(),
            }],
            vec!["prefix-hidden preview content".into()],
            ShowData::default(),
        );
        app.handle_key(Key::Enter, &mut history);
        app.set_horizontal_viewport_width("preview content".len());
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        let mut log_state = ListState::default();
        for (width, height, direction) in [
            (80, 10, Direction::Horizontal),
            (30, 20, Direction::Vertical),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("log row"));
            assert!(text.contains("preview content"));
            assert!(!text.contains("prefix-hidden"));
            assert!(!text.contains("(focused)"));

            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(ratatui::layout::Rect::new(0, 0, width, height - 1));
            assert!(!border_has_fg(&terminal, panes[0], Color::Cyan));
            assert!(border_has_fg(&terminal, panes[1], Color::Cyan));
            assert_eq!(
                terminal
                    .backend()
                    .buffer()
                    .cell((panes[1].x + 2, panes[1].y))
                    .expect("preview title")
                    .symbol(),
                "p"
            );
        }
    }

    #[test]
    fn renders_show_explorer_and_diff_in_both_orientations() {
        let mut show = show_data();
        show.diff = vec!["\x1b[31mdiff --git a/src/lib.rs b/src/lib.rs\x1b[m".into()];
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            Vec::new(),
            show,
        );
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        let mut log_state = ListState::default();
        for (width, height, direction) in [
            (80, 10, Direction::Horizontal),
            (30, 20, Direction::Vertical),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("commit full-id"));
            assert!(text.contains("src/lib.rs"));
            assert!(text.contains("diff --git"));
            assert!(!text.contains("(focused)"));
            assert!(!text.contains("gitlsd log"));

            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(ratatui::layout::Rect::new(0, 0, width, height - 1));
            assert!(border_has_fg(&terminal, panes[0], Color::Cyan));
            assert!(!border_has_fg(&terminal, panes[1], Color::Cyan));
            assert_eq!(
                terminal
                    .backend()
                    .buffer()
                    .cell((panes[1].x + 2, panes[1].y))
                    .expect("diff title")
                    .symbol(),
                "d"
            );

            let explorer = panes[0];
            let selected_row_y = explorer.y + 5;
            assert!((explorer.x..explorer.x + explorer.width).any(|x| {
                terminal
                    .backend()
                    .buffer()
                    .cell((x, selected_row_y))
                    .is_some_and(|cell| cell.style().add_modifier.contains(Modifier::REVERSED))
            }));
        }

        app.handle_key(Key::Enter, &mut history);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("files "));
        assert!(!text.contains("(focused)"));
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(ratatui::layout::Rect::new(0, 0, 80, 9));
        assert!(!border_has_fg(&terminal, panes[0], Color::Cyan));
        assert!(border_has_fg(&terminal, panes[1], Color::Cyan));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((panes[1].x + 1, panes[1].y + 1))
                .expect("styled diff line")
                .style()
                .fg,
            Some(Color::Indexed(1))
        );

        app.handle_key(Key::Enter, &mut history);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(!text.contains("(focused)"));
        assert!(text.contains("diff --git"));
        assert!(!text.contains("commit full-id"));
        assert!(!text.contains(" files "));
        assert!(border_has_fg(
            &terminal,
            ratatui::layout::Rect::new(0, 0, 80, 9),
            Color::Cyan
        ));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((1, 1))
                .expect("fullscreen styled diff line")
                .style()
                .fg,
            Some(Color::Indexed(1))
        );

        app.handle_key(Key::Escape, &mut history);
        let show = app.screen().show().unwrap();
        assert_eq!(show.focus(), ShowFocus::Explorer);
        assert!(!show.diff_fullscreen());
    }

    #[test]
    fn long_show_metadata_does_not_hide_file_explorer() {
        let mut show = show_data();
        show.metadata = (0..20)
            .map(|index| format!("commit metadata line {index}"))
            .collect();
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            Vec::new(),
            show,
        );
        app.dispatch(Action::Show(ShowAction::Open), &mut history);

        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();

        let text = buffer_text(&terminal);
        assert!(text.contains("commit metadata line 0"));
        assert!(!text.contains("commit metadata line 2"));
        assert!(text.contains("src/lib.rs"));
        assert!(
            (0..40).any(|x| {
                terminal
                    .backend()
                    .buffer()
                    .cell((x, 5))
                    .is_some_and(|cell| cell.style().add_modifier.contains(Modifier::REVERSED))
            }),
            "selected file row should remain visible"
        );
    }

    #[test]
    fn renders_status_groups_and_diff_in_both_orientations() {
        let mut app = App::new(Config::default());
        let mut history = FixtureHistory {
            records: vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            preview: Vec::new(),
            show: ShowData::default(),
            status: StatusData {
                staged: vec![StatusFile {
                    display: "M  staged.rs".into(),
                    old_path: Some(b"staged.rs".to_vec()),
                    new_path: Some(b"staged.rs".to_vec()),
                }],
                unstaged: vec![StatusFile {
                    display: "?? untracked.rs".into(),
                    old_path: None,
                    new_path: Some(b"untracked.rs".to_vec()),
                }],
                staged_diff: vec!["\x1b[31mdiff --git a/staged.rs b/staged.rs\x1b[m".into()],
                unstaged_diff: vec!["untracked output".into()],
            },
            next_status: None,
        };
        app.initialize(&mut history).unwrap();
        app.dispatch(Action::Status(StatusAction::Open), &mut history);
        let mut log_state = ListState::default();
        for (width, height, direction) in [
            (80, 10, Direction::Horizontal),
            (30, 20, Direction::Vertical),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("staged"));
            assert!(text.contains("unstaged"));
            assert!(text.contains("untracked output"));
            assert!(!text.contains("(focused)"));
            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(ratatui::layout::Rect::new(0, 0, width, height - 1));
            assert!(border_has_fg(&terminal, panes[0], Color::Cyan));
            assert!(!border_has_fg(&terminal, panes[1], Color::Cyan));
            assert_eq!(
                terminal
                    .backend()
                    .buffer()
                    .cell((panes[1].x + 2, panes[1].y))
                    .expect("status diff title")
                    .symbol(),
                "d"
            );
        }

        app.handle_key(Key::Tab, &mut history);
        app.handle_key(Key::Enter, &mut history);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(!text.contains("(focused)"));
        assert!(text.contains("diff --git a/staged.rs b/staged.rs"));
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(ratatui::layout::Rect::new(0, 0, 80, 9));
        assert!(!border_has_fg(&terminal, panes[0], Color::Cyan));
        assert!(border_has_fg(&terminal, panes[1], Color::Cyan));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((panes[1].x + 1, panes[1].y + 1))
                .expect("styled status diff line")
                .style()
                .fg,
            Some(Color::Indexed(1))
        );

        app.handle_key(Key::Enter, &mut history);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(!text.contains("(focused)"));
        assert!(text.contains("diff --git a/staged.rs b/staged.rs"));
        assert!(!text.contains("unstaged"));
        assert!(border_has_fg(
            &terminal,
            ratatui::layout::Rect::new(0, 0, 80, 9),
            Color::Cyan
        ));

        app.handle_key(Key::Char('q'), &mut history);
        let status = app.screen().status().unwrap();
        assert_eq!(status.focus(), StatusFocus::Explorer);
        assert_eq!(status.group(), StatusGroup::Staged);
        assert!(!status.diff_fullscreen());
    }

    #[test]
    fn renders_scrolled_show_metadata() {
        let mut show = show_data();
        show.metadata = vec!["prefix-hidden metadata".into()];
        let (mut app, mut history) = app_with_history(
            vec![CommitRecord {
                id: "full-id".into(),
                display: "commit".into(),
            }],
            Vec::new(),
            show,
        );
        app.dispatch(Action::Show(ShowAction::Open), &mut history);
        app.set_horizontal_viewport_width("metadata".len());
        app.dispatch(
            Action::Navigation(NavigationAction::ScrollEnd),
            &mut history,
        );
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("metadata"));
        assert!(!text.contains("prefix-hidden"));
    }

    #[test]
    fn renders_scrolled_help_in_small_terminal_without_moving_log_selection() {
        let records = (0..5)
            .map(|index| CommitRecord {
                id: index.to_string(),
                display: format!("row {index}"),
            })
            .collect();
        let (mut app, mut history) = app_with_history(records, Vec::new(), ShowData::default());
        for _ in 0..4 {
            app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
        }
        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.dispatch(Action::Navigation(NavigationAction::PageDown), &mut history);
        assert_eq!(app.log_state().selected(), 4);

        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("help"));
        assert!(text.contains("binding."));
        assert!(!text.contains("setting.log"));
        assert!(text.contains("Showing effective configuration"));
    }

    #[test]
    fn renders_help_at_horizontal_offset() {
        let (mut app, mut history) = app_with_history(Vec::new(), Vec::new(), ShowData::default());
        app.dispatch(Action::Global(GlobalAction::Help), &mut history);
        app.set_horizontal_viewport_width(2);
        for _ in 0.."setting.".len() {
            app.dispatch(
                Action::Navigation(NavigationAction::ScrollRight),
                &mut history,
            );
        }
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("log=\"git\""));
        assert!(!text.contains("setting.log"));
    }

    #[test]
    fn keeps_log_cursor_in_view_while_reversing_direction() {
        let records: Vec<CommitRecord> = (0..24)
            .map(|index| CommitRecord {
                id: index.to_string(),
                display: format!("row {index}"),
            })
            .collect();

        for (width, height) in [(80, 10), (20, 30)] {
            let (mut app, mut history) =
                app_with_history(records.clone(), Vec::new(), ShowData::default());
            let mut log_state = ListState::default();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for _ in 0..18 {
                app.dispatch(Action::Navigation(NavigationAction::MoveDown), &mut history);
            }
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let bottom_offset = log_state.offset();
            assert!(bottom_offset > 0);

            app.dispatch(Action::Navigation(NavigationAction::MoveUp), &mut history);
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            assert_eq!(log_state.offset(), bottom_offset);

            while app.log_state().selected() > log_state.offset() {
                app.dispatch(Action::Navigation(NavigationAction::MoveUp), &mut history);
                terminal
                    .draw(|frame| render(frame, &app, &mut log_state))
                    .unwrap();
            }
            let top_offset = log_state.offset();
            app.dispatch(Action::Navigation(NavigationAction::MoveUp), &mut history);
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            assert_eq!(log_state.offset(), top_offset - 1);
        }
    }

    fn border_has_fg(
        terminal: &Terminal<TestBackend>,
        area: ratatui::layout::Rect,
        color: Color,
    ) -> bool {
        let right = area.x + area.width.saturating_sub(1);
        let bottom = area.y + area.height.saturating_sub(1);
        (area.x..=right).any(|x| {
            [area.y, bottom].into_iter().any(|y| {
                terminal
                    .backend()
                    .buffer()
                    .cell((x, y))
                    .is_some_and(|cell| cell.style().fg == Some(color))
            })
        }) || (area.y..=bottom).any(|y| {
            [area.x, right].into_iter().any(|x| {
                terminal
                    .backend()
                    .buffer()
                    .cell((x, y))
                    .is_some_and(|cell| cell.style().fg == Some(color))
            })
        })
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer.cell((x, y)).expect("inside buffer").symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
