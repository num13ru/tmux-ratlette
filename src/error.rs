use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid command-line arguments: {0}")]
    Cli(String),

    #[error(
        "could not resolve a configuration directory; set HOME, XDG_CONFIG_HOME, or --config-dir"
    )]
    ConfigDirectoryUnavailable,

    #[error("configuration directory cannot be an empty path")]
    EmptyConfigDirectory,

    #[error("tmux-ratlette must run in an interactive terminal")]
    NotInteractive,

    #[error("terminal operation failed: {0}")]
    Terminal(#[source] std::io::Error),

    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
