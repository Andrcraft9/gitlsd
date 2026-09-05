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
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
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
                .map(|record| {
                    ListItem::new(Line::from(format!(
                        "{} {} {:<18} {}",
                        record.short_id, record.date, record.author, record.subject
                    )))
                })
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
            frame.render_stateful_widget(list, chunks[0], &mut state);
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
            short_id: "abc1234".into(),
            author: "Author".into(),
            date: "2026-09-05".into(),
            subject: "Rendered subject".into(),
        });
        app.status = "Ready".into();
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
