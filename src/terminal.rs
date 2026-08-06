use std::io::{self, IsTerminal, Stdout};

#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::style::force_color_output;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use crate::{Error, Result};

#[cfg(unix)]
static RECEIVED_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
const CLEANUP_SIGNALS: [libc::c_int; 4] =
    [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];

/// Temporarily converts terminating signals into an event-loop wakeup.
///
/// Dropping this guard restores the previous handlers and re-raises the first
/// received signal. Declare it before `TerminalSession` so Rust drops the
/// terminal (and restores termios) before the signal resumes its normal action.
pub struct SignalHandlers {
    #[cfg(unix)]
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl SignalHandlers {
    pub fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            RECEIVED_SIGNAL.store(0, Ordering::SeqCst);
            let mut handlers = Self {
                previous: Vec::with_capacity(CLEANUP_SIGNALS.len()),
            };

            for signal in CLEANUP_SIGNALS {
                let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
                action.sa_sigaction = record_signal as libc::sighandler_t;
                action.sa_flags = 0;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }

                let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
                if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                handlers.previous.push((signal, previous));
            }

            Ok(handlers)
        }

        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    pub fn received(&self) -> bool {
        #[cfg(unix)]
        {
            RECEIVED_SIGNAL.load(Ordering::SeqCst) != 0
        }

        #[cfg(not(unix))]
        {
            false
        }
    }
}

#[cfg(unix)]
extern "C" fn record_signal(signal: libc::c_int) {
    let _ = RECEIVED_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
}

impl Drop for SignalHandlers {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let received = RECEIVED_SIGNAL.swap(0, Ordering::SeqCst);
            for (signal, previous) in self.previous.drain(..).rev() {
                unsafe {
                    libc::sigaction(signal, &previous, std::ptr::null_mut());
                }
            }
            if received != 0 {
                unsafe {
                    libc::raise(received);
                }
            }
        }
    }
}

pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse_enabled: bool,
    #[cfg(unix)]
    original_termios: libc::termios,
}

impl TerminalSession {
    pub fn enter(mouse_enabled: bool) -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::NotInteractive);
        }

        // The palette has an explicit theme, so inherited NO_COLOR must not
        // suppress the colors selected by the user.
        force_color_output(true);
        #[cfg(unix)]
        let original_termios = current_termios().map_err(Error::Terminal)?;
        enable_raw_mode().map_err(Error::Terminal)?;

        let mut stdout = io::stdout();
        let setup_result = if mouse_enabled {
            execute!(stdout, Hide, EnableMouseCapture)
        } else {
            execute!(stdout, Hide)
        };

        if let Err(source) = setup_result {
            let _ = disable_raw_mode();
            #[cfg(unix)]
            restore_termios(&original_termios);
            let _ = execute!(stdout, Show, DisableMouseCapture);
            return Err(Error::Terminal(source));
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(source) => {
                best_effort_restore();
                #[cfg(unix)]
                restore_termios(&original_termios);
                return Err(Error::Terminal(source));
            }
        };

        Ok(Self {
            terminal,
            mouse_enabled,
            #[cfg(unix)]
            original_termios,
        })
    }

    pub fn draw<F>(&mut self, draw: F) -> Result<Rect>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal
            .draw(draw)
            .map(|frame| frame.area)
            .map_err(Error::Terminal)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        #[cfg(unix)]
        restore_termios(&self.original_termios);
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

#[cfg(unix)]
fn current_termios() -> io::Result<libc::termios> {
    let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) } == 0 {
        Ok(termios)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn restore_termios(termios: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, termios);
    }
}

pub fn best_effort_restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), Show, DisableMouseCapture);
}
