use crate::model::{Theme, ThemeColor};

pub const SHADES_OF_PURPLE: Theme = Theme {
    bg: ThemeColor::rgb(30, 29, 64),
    panel: ThemeColor::rgb(45, 43, 85),
    selected: ThemeColor::rgb(80, 77, 122),
    fg: ThemeColor::rgb(255, 255, 255),
    muted: ThemeColor::rgb(165, 153, 233),
    accent: ThemeColor::rgb(250, 208, 0),
    selected_fg: None,
    title_fg: None,
};

pub const fn default_theme() -> Theme {
    SHADES_OF_PURPLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_the_typescript_shades_of_purple_theme() {
        let theme = default_theme();

        assert_eq!(theme.bg.tmux(), "#1e1d40");
        assert_eq!(theme.panel.tmux(), "#2d2b55");
        assert_eq!(theme.selected.tmux(), "#504d7a");
        assert_eq!(theme.fg.tmux(), "#ffffff");
        assert_eq!(theme.muted.tmux(), "#a599e9");
        assert_eq!(theme.accent.tmux(), "#fad000");
    }
}
