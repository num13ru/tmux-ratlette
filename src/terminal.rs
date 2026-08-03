use std::io::{self, IsTerminal, Stdout};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::{Error, Result};

pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse_enabled: bool,
}

impl TerminalSession {
    pub fn enter(mouse_enabled: bool) -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::NotInteractive);
        }

        enable_raw_mode().map_err(Error::Terminal)?;

        let mut stdout = io::stdout();
        let setup_result = if mouse_enabled {
            execute!(stdout, Hide, EnableMouseCapture)
        } else {
            execute!(stdout, Hide)
        };

        if let Err(source) = setup_result {
            let _ = disable_raw_mode();
            let _ = execute!(stdout, Show, DisableMouseCapture);
            return Err(Error::Terminal(source));
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(source) => {
                best_effort_restore();
                return Err(Error::Terminal(source));
            }
        };

        Ok(Self {
            terminal,
            mouse_enabled,
        })
    }

    pub fn draw<F>(&mut self, draw: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal
            .draw(draw)
            .map(|_| ())
            .map_err(Error::Terminal)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        if self.mouse_enabled {
            let _ = execute!(self.terminal.backend_mut(), Show, DisableMouseCapture);
        } else {
            let _ = execute!(self.terminal.backend_mut(), Show);
        }
        let _ = self.terminal.show_cursor();
    }
}

pub fn install_cleanup_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        best_effort_restore();
        previous_hook(panic_info);
    }));
}

pub fn best_effort_restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), Show, DisableMouseCapture);
}
