use std::process::ExitCode;

use clap::Parser;
use tmux_ratlette::app;
use tmux_ratlette::cli::Cli;
use tmux_ratlette::terminal;

fn main() -> ExitCode {
    terminal::install_cleanup_panic_hook();

    let result = Cli::parse().into_invocation().and_then(app::run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tmux-ratlette: {error}");
            ExitCode::FAILURE
        }
    }
}
