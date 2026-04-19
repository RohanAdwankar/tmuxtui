mod app;
mod managed_config;
mod tmux;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{app::App, managed_config::ManagedConfig, tmux::Tmux};

fn main() -> Result<()> {
    loop {
        let managed = ManagedConfig::bootstrap()?;
        let tmux = Tmux::new(managed);
        tmux.ensure_ready()?;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = run_app(&mut terminal, App::new(tmux.clone()));

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        match result? {
            Some(target) => tmux.attach(&target)?,
            None => return Ok(()),
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
) -> Result<Option<crate::tmux::TargetKind>> {
    app.run(terminal)
}
