#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Tmux(String),
    Shell(String),
    Popup(String),
    Palette(String),
    None,
}

impl Action {
    pub fn tmux(command: impl Into<String>) -> Self {
        Self::Tmux(command.into())
    }

    pub fn palette(name: impl Into<String>) -> Self {
        Self::Palette(name.into())
    }
}
