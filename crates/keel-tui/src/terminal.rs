use crate::app::{App, TuiFilters};
use crate::ui;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use keel_core::KeelProject;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::Duration;

pub fn run_tui(project: KeelProject) -> Result<()> {
    run_tui_with_filters(project, TuiFilters::default())
}

pub fn run_tui_with_filter(project: KeelProject, filter: Option<String>) -> Result<()> {
    run_tui_with_filters(
        project,
        TuiFilters {
            text: filter.unwrap_or_default(),
            ..TuiFilters::default()
        },
    )
}

pub fn run_tui_with_filters(project: KeelProject, filters: TuiFilters) -> Result<()> {
    let mut app = App::load_with_filters(project, filters)?;
    let mut terminal = setup_terminal()?;
    let result = run_event_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> Result<TuiTerminal> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("failed to initialize terminal")
}

fn restore_terminal(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode().context("failed to disable terminal raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")
}

fn run_event_loop(terminal: &mut TuiTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal
            .draw(|frame| ui::render(frame, app))
            .context("failed to draw TUI frame")?;

        if !event::poll(Duration::from_millis(200)).context("failed to poll terminal events")? {
            continue;
        }

        let Event::Key(key) = event::read().context("failed to read terminal event")? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if app.filter_mode() {
            match key.code {
                KeyCode::Enter => app.finish_filter_edit(),
                KeyCode::Esc => app.clear_filter(),
                KeyCode::Backspace => app.pop_filter_char(),
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.push_filter_char(ch);
                }
                _ => {}
            }
            continue;
        }

        if app.help_visible() {
            match key.code {
                KeyCode::Char('?') => app.toggle_help(),
                KeyCode::Esc | KeyCode::Enter => app.close_help(),
                KeyCode::Char('q') => break,
                _ => {}
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
            KeyCode::Tab => app.next_tab(),
            KeyCode::BackTab => app.previous_tab(),
            KeyCode::Char('r') => app.refresh()?,
            KeyCode::Char('/') => app.begin_filter_edit(),
            KeyCode::Char('?') => app.toggle_help(),
            KeyCode::PageDown => app.scroll_down(15),
            KeyCode::PageUp => app.scroll_up(15),
            KeyCode::Home => app.scroll_home(),
            KeyCode::End => app.scroll_end(),
            _ => {}
        }
    }

    Ok(())
}
