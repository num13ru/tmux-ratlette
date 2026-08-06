#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupAction {
    pub command: String,
    pub width: Option<String>,
    pub height: Option<String>,
    pub pad_x: Option<u16>,
    pub pad_y: Option<u16>,
    pub border: Option<String>,
}

impl PopupAction {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            width: None,
            height: None,
            pad_x: None,
            pad_y: None,
            border: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Tmux(String),
    Shell(String),
    Popup(PopupAction),
    Palette(String),
    ApplyTheme(String),
    None,
}

impl Action {
    pub fn tmux(command: impl Into<String>) -> Self {
        Self::Tmux(command.into())
    }

    pub fn palette(name: impl Into<String>) -> Self {
        Self::Palette(name.into())
    }

    pub fn popup(command: impl Into<String>) -> Self {
        Self::Popup(PopupAction::new(command))
    }
}
