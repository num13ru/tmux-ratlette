use std::num::NonZeroU16;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{Error, Result};

#[derive(Debug, Parser)]
#[command(
    name = "tmux-ratlette",
    version,
    about = "Native command palette for tmux",
    subcommand_precedence_over_arg = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Filter the selected palette to one category.
    #[arg(long, global = true)]
    pub category: Option<String>,

    /// Override the tmux-palette configuration directory.
    #[arg(long, global = true, value_name = "PATH")]
    pub config_dir: Option<PathBuf>,

    /// Disable mouse capture in the interactive application.
    #[arg(long, global = true)]
    pub no_mouse: bool,

    /// Print additional diagnostics.
    #[arg(long, global = true)]
    pub debug: bool,

    /// Legacy measurement mode retained for the existing shell wrapper.
    #[arg(long, global = true, hide = true)]
    pub measure: bool,

    /// tmux client width used for popup measurement.
    #[arg(long = "client-width", alias = "cw", global = true)]
    pub client_width: Option<NonZeroU16>,

    /// tmux client height used for popup measurement.
    #[arg(long = "client-height", alias = "ch", global = true)]
    pub client_height: Option<NonZeroU16>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the main command palette.
    Commands,
    /// Find and focus a tmux pane.
    FindPane,
    /// Move the current pane to another window.
    MovePane,
    /// Preview and select a theme.
    Themes,
    /// Open a user-defined palette.
    Palette {
        /// Palette name below ~/.config/tmux-palette/palettes.
        name: String,
    },
    /// Print popup dimensions without starting the TUI.
    Measure {
        /// Built-in or user-defined palette name.
        #[arg(default_value = "commands")]
        palette: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub palette: String,
    pub mode: Mode,
    pub category: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub no_mouse: bool,
    pub debug: bool,
    pub client_width: Option<NonZeroU16>,
    pub client_height: Option<NonZeroU16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Interactive,
    Measure,
}

impl Cli {
    pub fn into_invocation(self) -> Result<Invocation> {
        let (palette, command_is_measure) = match self.command {
            None | Some(Command::Commands) => ("commands".to_owned(), false),
            Some(Command::FindPane) => ("find-pane".to_owned(), false),
            Some(Command::MovePane) => ("move-pane".to_owned(), false),
            Some(Command::Themes) => ("themes".to_owned(), false),
            Some(Command::Palette { name }) => (name, false),
            Some(Command::Measure { palette }) => (palette, true),
        };

        if self.measure && command_is_measure {
            return Err(Error::Cli(
                "use either the measure subcommand or the legacy --measure flag, not both"
                    .to_owned(),
            ));
        }

        if palette.trim().is_empty() {
            return Err(Error::Cli("palette name cannot be empty".to_owned()));
        }

        Ok(Invocation {
            palette,
            mode: if self.measure || command_is_measure {
                Mode::Measure
            } else {
                Mode::Interactive
            },
            category: self.category,
            config_dir: self.config_dir,
            no_mouse: self.no_mouse,
            debug: self.debug,
            client_width: self.client_width,
            client_height: self.client_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_interactive_commands_palette() {
        let invocation = Cli::try_parse_from(["tmux-ratlette"])
            .unwrap()
            .into_invocation()
            .unwrap();

        assert_eq!(invocation.palette, "commands");
        assert_eq!(invocation.mode, Mode::Interactive);
    }

    #[test]
    fn accepts_legacy_measurement_flags_after_palette() {
        let invocation = Cli::try_parse_from([
            "tmux-ratlette",
            "find-pane",
            "--measure",
            "--cw=140",
            "--ch=45",
        ])
        .unwrap()
        .into_invocation()
        .unwrap();

        assert_eq!(invocation.palette, "find-pane");
        assert_eq!(invocation.mode, Mode::Measure);
        assert_eq!(invocation.client_width.unwrap().get(), 140);
        assert_eq!(invocation.client_height.unwrap().get(), 45);
    }

    #[test]
    fn accepts_structured_measure_subcommand() {
        let invocation =
            Cli::try_parse_from(["tmux-ratlette", "measure", "themes", "--client-width=90"])
                .unwrap()
                .into_invocation()
                .unwrap();

        assert_eq!(invocation.palette, "themes");
        assert_eq!(invocation.mode, Mode::Measure);
    }

    #[test]
    fn rejects_zero_client_dimensions() {
        let error = Cli::try_parse_from(["tmux-ratlette", "--measure", "--cw=0"]).unwrap_err();

        assert!(error.to_string().contains("invalid value '0'"));
    }

    #[test]
    fn rejects_duplicate_measure_modes() {
        let error = Cli::try_parse_from(["tmux-ratlette", "measure", "--measure"])
            .unwrap()
            .into_invocation()
            .unwrap_err();

        assert!(error.to_string().contains("either the measure subcommand"));
    }
}
