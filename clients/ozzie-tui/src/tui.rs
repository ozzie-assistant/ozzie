use std::io::{self, Stdout};

use anyhow::Context;
use crossterm::{
    execute,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> anyhow::Result<Tui> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("create terminal")
}

pub fn restore(terminal: &mut Tui) -> anyhow::Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)
        .context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")
}
