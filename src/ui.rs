use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEvent, MouseEventKind,
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

use crate::app::{App, InputMode, Screen};
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
                Event::Mouse(mouse_event) if matches!(&app.input, InputMode::Normal) => {
                    if let Some(action) = translate_mouse(mouse_event) {
                        let size = session.terminal.size()?;
                        app.set_horizontal_viewport_width(active_content_width(
                            ratatui::layout::Rect::new(0, 0, size.width, size.height),
                            app,
                        ));
                        app.dispatch(action, source);
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
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen, Show);
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = disable_raw_mode();
                let mut stdout = io::stdout();
                let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen, Show);
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
        let _ = self.terminal.show_cursor();
    }
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
                    ListItem::new(scrolled_line(&record.display, app.log_horizontal_offset))
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
                    .map(|line| styled_line(line))
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

fn scrolled_line(text: &str, offset: usize) -> Line<'_> {
    if offset == 0 {
        return styled_line(text);
    }
    let line = styled_line(text);
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

fn translate_mouse(event: MouseEvent) -> Option<crate::config::Action> {
    match event.kind {
        MouseEventKind::ScrollLeft => Some(crate::config::Action::ScrollLeft),
        MouseEventKind::ScrollRight => Some(crate::config::Action::ScrollRight),
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
    fn translates_horizontal_mouse_events() {
        let event = |kind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            translate_mouse(event(MouseEventKind::ScrollLeft)),
            Some(Action::ScrollLeft)
        );
        assert_eq!(
            translate_mouse(event(MouseEventKind::ScrollRight)),
            Some(Action::ScrollRight)
        );
        assert_eq!(translate_mouse(event(MouseEventKind::ScrollUp)), None);
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
        assert_eq!(scrolled_line("界a", 1).to_string(), "a");
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
