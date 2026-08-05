pub mod app;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod fuzzy;
pub mod model;
pub mod palettes;
pub mod terminal;
pub mod themes;
pub(crate) mod tmux;

pub use error::{Error, Result};
