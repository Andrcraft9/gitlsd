//! Interactive terminal frontend.
//!
//! This module translates Crossterm events into shared keys, drives [`App`],
//! and renders log, preview, help, and show state with Ratatui. It also owns
//! setup and restoration of the terminal session.

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

use crate::app::{App, Screen, ShowFocus, ShowState};
use crate::config::Key;
use crate::git::HistorySource;

pub fn run(app: &mut App, source: &mut impl HistorySource) -> io::Result<()> {
    let mut session = TerminalSession::start()?;
    let mut log_state = ListState::default();
    let mut show_state = ListState::default();
    while app.is_running() {
        session
            .terminal
            .draw(|frame| render_with_states(frame, app, &mut log_state, &mut show_state))?;
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
            let panes = Layout::default()
                .direction(content_split_direction(chunks[0]))
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);
            match show.focus() {
                ShowFocus::Explorer => panes[0],
                ShowFocus::Diff => panes[1],
            }
        }
    };
    let marker_width = usize::from(
        (matches!(app.screen(), Screen::Log) && !app.log_state().preview_focused())
            || matches!(
                app.screen(),
                Screen::Show(show) if show.focus() == ShowFocus::Explorer
            ),
    ) * 2;
    usize::from(pane.width.saturating_sub(2)).saturating_sub(marker_width)
}

#[cfg(test)]
fn render(frame: &mut ratatui::Frame<'_>, app: &App, log_state: &mut ListState) {
    let mut show_state = ListState::default();
    render_with_states(frame, app, log_state, &mut show_state);
}

fn render_with_states(
    frame: &mut ratatui::Frame<'_>,
    app: &App,
    log_state: &mut ListState,
    show_state: &mut ListState,
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
                .block(Block::default().title(" gitlsd log ").borders(Borders::ALL))
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
                let preview = app
                    .log_state()
                    .preview_lines()
                    .iter()
                    .map(|line| highlighted_line(line, app.preview_search_query()))
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(preview)
                        .scroll((
                            log.preview_offset().try_into().unwrap_or(u16::MAX),
                            log.preview_horizontal_offset()
                                .try_into()
                                .unwrap_or(u16::MAX),
                        ))
                        .block(
                            Block::default()
                                .title(if log.preview_focused() {
                                    " preview (focused) "
                                } else {
                                    " preview "
                                })
                                .borders(Borders::ALL),
                        ),
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
        Screen::Show(show) => {
            render_show(frame, show, app.show_search_query(), chunks[0], show_state)
        }
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
) {
    let panes = Layout::default()
        .direction(content_split_direction(area))
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let explorer = panes[0];
    let metadata_height = show
        .metadata()
        .len()
        .saturating_add(2)
        .min(usize::from(explorer.height.saturating_sub(1)))
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

    let files = show
        .files()
        .iter()
        .map(|file| {
            ListItem::new(scrolled_line(
                &file.display,
                show.explorer_horizontal_offset(),
                None,
            ))
        })
        .collect::<Vec<_>>();
    show_state.select(show.selected());
    let files = List::new(files)
        .block(
            Block::default()
                .title(if show.focus() == ShowFocus::Explorer {
                    " files (focused) "
                } else {
                    " files "
                })
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(files, explorer_chunks[1], show_state);

    let diff = show
        .diff_lines()
        .iter()
        .map(|line| highlighted_line(line, search_query))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(diff)
            .scroll((
                show.diff_offset().try_into().unwrap_or(u16::MAX),
                show.diff_horizontal_offset().try_into().unwrap_or(u16::MAX),
            ))
            .block(
                Block::default()
                    .title(if show.focus() == ShowFocus::Diff {
                        " diff (focused) "
                    } else {
                        " diff "
                    })
                    .borders(Borders::ALL),
            ),
        panes[1],
    );
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;

    use crate::config::{Action, Config, Key};
    use crate::git::{ChangedFile, CommitRecord, GitError, HistorySource, ShowData};

    use super::*;

    struct FixtureHistory {
        records: Vec<CommitRecord>,
        preview: Vec<String>,
        show: ShowData,
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
            (KeyCode::PageUp, KeyModifiers::NONE, Some(Key::PageUp)),
            (KeyCode::PageDown, KeyModifiers::NONE, Some(Key::PageDown)),
            (KeyCode::Home, KeyModifiers::NONE, Some(Key::Home)),
            (KeyCode::End, KeyModifiers::NONE, Some(Key::End)),
            (KeyCode::Left, KeyModifiers::NONE, Some(Key::Left)),
            (KeyCode::Right, KeyModifiers::NONE, Some(Key::Right)),
            (KeyCode::Esc, KeyModifiers::NONE, Some(Key::Escape)),
            (KeyCode::Enter, KeyModifiers::NONE, Some(Key::Enter)),
            (KeyCode::Backspace, KeyModifiers::NONE, Some(Key::Backspace)),
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
        app.dispatch(Action::ShowMode, &mut history);
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
        app.dispatch(Action::TogglePreview, &mut history);
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("gitlsd log"));
        assert!(text.contains("abc1234 2026-09-05 Author"));
        assert!(text.contains("Rendered subject"));
        assert!(text.contains("Loaded 1 commits"));
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
        app.dispatch(Action::TogglePreview, &mut history);
        app.set_horizontal_viewport_width("visible-content".len());
        app.dispatch(Action::ScrollEnd, &mut history);
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
        app.dispatch(Action::TogglePreview, &mut history);
        app.set_horizontal_viewport_width(8);
        app.dispatch(Action::ScrollEnd, &mut history);
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
        app.dispatch(Action::ScrollEnd, &mut history);
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
            assert!(text.contains("focused"));

            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(ratatui::layout::Rect::new(0, 0, width, height - 1));
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
        app.dispatch(Action::ShowMode, &mut history);
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
            assert!(text.contains("files (focused)"));
            assert!(!text.contains("gitlsd log"));

            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(ratatui::layout::Rect::new(0, 0, width, height - 1));
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
        assert!(!text.contains("files (focused)"));
        assert!(text.contains("diff (focused)"));
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(ratatui::layout::Rect::new(0, 0, 80, 9));
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
        app.dispatch(Action::ShowMode, &mut history);
        app.set_horizontal_viewport_width("metadata".len());
        app.dispatch(Action::ScrollEnd, &mut history);
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
            app.dispatch(Action::MoveDown, &mut history);
        }
        app.dispatch(Action::Help, &mut history);
        app.dispatch(Action::PageDown, &mut history);
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
        app.dispatch(Action::Help, &mut history);
        app.set_horizontal_viewport_width(2);
        for _ in 0.."setting.".len() {
            app.dispatch(Action::ScrollRight, &mut history);
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
                app.dispatch(Action::MoveDown, &mut history);
            }
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let bottom_offset = log_state.offset();
            assert!(bottom_offset > 0);

            app.dispatch(Action::MoveUp, &mut history);
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            assert_eq!(log_state.offset(), bottom_offset);

            while app.log_state().selected() > log_state.offset() {
                app.dispatch(Action::MoveUp, &mut history);
                terminal
                    .draw(|frame| render(frame, &app, &mut log_state))
                    .unwrap();
            }
            let top_offset = log_state.offset();
            app.dispatch(Action::MoveUp, &mut history);
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            assert_eq!(log_state.offset(), top_offset - 1);
        }
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
