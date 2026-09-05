use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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

use crate::app::{App, Screen};
use crate::config::Key;
use crate::git::HistorySource;

pub fn run(app: &mut App, source: &mut impl HistorySource) -> io::Result<()> {
    let mut session = TerminalSession::start()?;
    while app.running {
        session.terminal.draw(|frame| render(frame, app))?;
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key_event) = event::read()?
            && key_event.kind != KeyEventKind::Release
            && let Some(key) = translate_key(key_event)
        {
            app.handle_key(key, source);
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
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            let _ = execute!(stdout, LeaveAlternateScreen, Show);
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = disable_raw_mode();
                let mut stdout = io::stdout();
                let _ = execute!(stdout, LeaveAlternateScreen, Show);
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = self.terminal.show_cursor();
    }
}

fn render(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.screen {
        Screen::Log => {
            let items: Vec<_> = app
                .records
                .iter()
                .map(|record| ListItem::new(styled_line(&record.display)))
                .collect();
            let list = List::new(items)
                .block(Block::default().title(" gitlsd log ").borders(Borders::ALL))
                .highlight_symbol("> ")
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            let mut state = ListState::default().with_selected(if app.records.is_empty() {
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
                frame.render_stateful_widget(list, panes[0], &mut state);
                let preview = app
                    .preview_lines
                    .iter()
                    .map(|line| styled_line(line))
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(preview)
                        .scroll((app.preview_offset.try_into().unwrap_or(u16::MAX), 0))
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
                frame.render_stateful_widget(list, chunks[0], &mut state);
            }
        }
        Screen::Help => {
            let text = app.config.help_lines().join("\n");
            frame.render_widget(
                Paragraph::new(text)
                    .scroll((app.help_offset.try_into().unwrap_or(u16::MAX), 0))
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
    fn renders_log_selection_and_status() {
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "full-id".into(),
            display: "abc1234 2026-09-05 Author Rendered subject".into(),
        });
        app.status = "Ready".into();
        app.preview_visible = false;
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
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
    fn renders_preview_in_both_orientations_and_marks_focus() {
        let mut app = App::new(Config::default());
        app.records.push(CommitRecord {
            id: "id".into(),
            display: "log row".into(),
        });
        app.preview_lines = vec!["preview content".into()];
        app.preview_focused = true;
        for (width, height) in [(80, 10), (20, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("log row"));
            assert!(text.contains("preview content"));
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

        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("help"));
        assert!(text.contains("binding."));
        assert!(!text.contains("setting.log"));
        assert!(text.contains("Showing effective configuration"));
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
