//! Interactive terminal frontend.
//!
//! This module translates Crossterm events into shared keys, drives [`App`],
//! and renders its state with Ratatui. It also owns setup and restoration of
//! the terminal session.

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

use crate::app::{App, Screen};
use crate::config::Key;
use crate::git::HistorySource;

pub fn run(app: &mut App, source: &mut impl HistorySource) -> io::Result<()> {
    let mut session = TerminalSession::start()?;
    let mut log_state = ListState::default();
    while app.running {
        session
            .terminal
            .draw(|frame| render(frame, app, &mut log_state))?;
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

fn active_content_width(area: ratatui::layout::Rect, app: &App) -> usize {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let pane = match app.screen {
        Screen::Help => chunks[0],
        Screen::Log if app.preview_visible => {
            let direction = if chunks[0].width > chunks[0].height {
                Direction::Horizontal
            } else {
                Direction::Vertical
            };
            let panes = Layout::default()
                .direction(direction)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);
            if app.preview_focused {
                panes[1]
            } else {
                panes[0]
            }
        }
        Screen::Log => chunks[0],
    };
    let marker_width = usize::from(app.screen == Screen::Log && !app.preview_focused) * 2;
    usize::from(pane.width.saturating_sub(2)).saturating_sub(marker_width)
}

fn render(frame: &mut ratatui::Frame<'_>, app: &App, log_state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.screen {
        Screen::Log => {
            let items: Vec<_> = app
                .records
                .iter()
                .map(|record| {
                    ListItem::new(scrolled_line(
                        &record.display,
                        app.log_horizontal_offset,
                        app.log_search_query(),
                    ))
                })
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" gitlsd log ").borders(Borders::ALL))
                .highlight_symbol("> ")
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            log_state.select(if app.records.is_empty() {
                None
            } else {
                Some(app.selected)
            });
            if app.preview_visible {
                let direction = if chunks[0].width > chunks[0].height {
                    Direction::Horizontal
                } else {
                    Direction::Vertical
                };
                let panes = Layout::default()
                    .direction(direction)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(chunks[0]);
                frame.render_stateful_widget(list, panes[0], log_state);
                let preview = app
                    .preview_lines
                    .iter()
                    .map(|line| highlighted_line(line, app.preview_search_query()))
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(preview)
                        .scroll((
                            app.preview_offset.try_into().unwrap_or(u16::MAX),
                            app.preview_horizontal_offset.try_into().unwrap_or(u16::MAX),
                        ))
                        .block(
                            Block::default()
                                .title(if app.preview_focused {
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
        Screen::Help => {
            let text = app.config.help_lines().join("\n");
            frame.render_widget(
                Paragraph::new(text)
                    .scroll((
                        app.help_offset.try_into().unwrap_or(u16::MAX),
                        app.help_horizontal_offset.try_into().unwrap_or(u16::MAX),
                    ))
                    .block(Block::default().title(" help ").borders(Borders::ALL)),
                chunks[0],
            );
        }
    }

    let status = app.input_label().unwrap_or_else(|| app.status.clone());
    frame.render_widget(Paragraph::new(status), chunks[1]);
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

    use crate::config::{Action, Config};
    use crate::git::{CommitRecord, GitError};

    use super::*;

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
    fn renders_log_selection_and_status() {
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "full-id".into(),
            display: "abc1234 2026-09-05 Author Rendered subject".into(),
        });
        app.status = "Ready".into();
        app.preview_visible = false;
        let mut log_state = ListState::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &mut log_state))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("gitlsd log"));
        assert!(text.contains("abc1234 2026-09-05 Author"));
        assert!(text.contains("Rendered subject"));
        assert!(text.contains("Ready"));
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
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "full-id".into(),
            display: "prefix-hidden visible-content".into(),
        });
        app.preview_visible = false;
        app.log_horizontal_offset = "prefix-hidden ".len();
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
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "full-id".into(),
            display: "\x1b[31m界e\u{301} visible\x1b[m".into(),
        });
        app.preview_visible = false;
        app.log_horizontal_offset = 3;
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
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "id".into(),
            display: "log row".into(),
        });
        app.preview_lines = vec!["prefix-hidden preview content".into()];
        app.preview_focused = true;
        app.preview_horizontal_offset = "prefix-hidden ".len();
        let mut log_state = ListState::default();
        for (width, height) in [(80, 10), (20, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, &mut log_state))
                .unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("log row"));
            assert!(text.contains("preview content"));
            assert!(!text.contains("prefix-hidden"));
            assert!(text.contains("focused"));
        }
    }

    #[test]
    fn renders_scrolled_help_in_small_terminal_without_moving_log_selection() {
        struct Empty;

        impl HistorySource for Empty {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(Vec::new())
            }
        }

        let mut app = App::new(Config::default());
        app.selected = 4;
        let mut history = Empty;
        app.dispatch(Action::Help, &mut history);
        app.dispatch(Action::PageDown, &mut history);
        assert_eq!(app.selected, 4);

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
        struct Empty;

        impl HistorySource for Empty {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(Vec::new())
            }
        }

        let mut app = App::new(Config::default());
        let mut history = Empty;
        app.dispatch(Action::Help, &mut history);
        app.help_horizontal_offset = "setting.".len();
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
        struct Empty;

        impl HistorySource for Empty {
            fn load(
                &mut self,
                _offset: usize,
                _limit: usize,
            ) -> Result<Vec<CommitRecord>, GitError> {
                Ok(Vec::new())
            }
        }

        let mut app = App::new(Config::default());
        app.records = (0..24)
            .map(|index| CommitRecord {
                id: index.to_string(),
                display: format!("row {index}"),
            })
            .collect();
        let mut history = Empty;

        for (width, height) in [(80, 10), (20, 30)] {
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

            while app.selected > log_state.offset() {
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

            app.selected = 0;
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
