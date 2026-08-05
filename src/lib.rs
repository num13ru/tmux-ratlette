pub mod app;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod fuzzy;
pub mod model;
pub mod palettes;
mod plugin_source;
pub mod terminal;
pub mod themes;
mod tmux;
mod user_config;

pub use error::{Error, Result};
