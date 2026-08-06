use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Rgb(u8, u8, u8),
}

impl ThemeColor {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::Rgb(red, green, blue)
    }

    pub const fn ratatui(self) -> Color {
        match self {
            Self::Default => Color::Reset,
            Self::Black => Color::Black,
            Self::Red => Color::Red,
            Self::Green => Color::Green,
            Self::Yellow => Color::Yellow,
            Self::Blue => Color::Blue,
            Self::Magenta => Color::Magenta,
            Self::Cyan => Color::Cyan,
            Self::White => Color::Gray,
            Self::BrightBlack => Color::DarkGray,
            Self::BrightRed => Color::LightRed,
            Self::BrightGreen => Color::LightGreen,
            Self::BrightYellow => Color::LightYellow,
            Self::BrightBlue => Color::LightBlue,
            Self::BrightMagenta => Color::LightMagenta,
            Self::BrightCyan => Color::LightCyan,
            Self::BrightWhite => Color::White,
            Self::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "transparent" => Some(Self::Default),
            "black" => Some(Self::Black),
            "red" => Some(Self::Red),
            "green" => Some(Self::Green),
            "yellow" => Some(Self::Yellow),
            "blue" => Some(Self::Blue),
            "magenta" => Some(Self::Magenta),
            "cyan" => Some(Self::Cyan),
            "white" => Some(Self::White),
            "bright-black" => Some(Self::BrightBlack),
            "bright-red" => Some(Self::BrightRed),
            "bright-green" => Some(Self::BrightGreen),
            "bright-yellow" => Some(Self::BrightYellow),
            "bright-blue" => Some(Self::BrightBlue),
            "bright-magenta" => Some(Self::BrightMagenta),
            "bright-cyan" => Some(Self::BrightCyan),
            "bright-white" => Some(Self::BrightWhite),
            _ => parse_hex(value),
        }
    }

    pub fn tmux(self) -> String {
        match self {
            Self::Default => "default".to_owned(),
            Self::Black => "black".to_owned(),
            Self::Red => "red".to_owned(),
            Self::Green => "green".to_owned(),
            Self::Yellow => "yellow".to_owned(),
            Self::Blue => "blue".to_owned(),
            Self::Magenta => "magenta".to_owned(),
            Self::Cyan => "cyan".to_owned(),
            Self::White => "white".to_owned(),
            Self::BrightBlack => "brightblack".to_owned(),
            Self::BrightRed => "brightred".to_owned(),
            Self::BrightGreen => "brightgreen".to_owned(),
            Self::BrightYellow => "brightyellow".to_owned(),
            Self::BrightBlue => "brightblue".to_owned(),
            Self::BrightMagenta => "brightmagenta".to_owned(),
            Self::BrightCyan => "brightcyan".to_owned(),
            Self::BrightWhite => "brightwhite".to_owned(),
            Self::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        }
    }
}

fn parse_hex(value: &str) -> Option<ThemeColor> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(ThemeColor::Rgb(
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: ThemeColor,
    pub panel: ThemeColor,
    pub selected: ThemeColor,
    pub fg: ThemeColor,
    pub muted: ThemeColor,
    pub accent: ThemeColor,
    pub selected_fg: Option<ThemeColor>,
    pub title_fg: Option<ThemeColor>,
}

impl Theme {
    pub fn tmux_body_style(self) -> String {
        format!("bg={}", self.panel.tmux())
    }

    pub fn tmux_border_style(self) -> String {
        format!("fg={},bg=default", self.accent.tmux())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rgb_colors_for_ratatui_and_tmux() {
        let color = ThemeColor::rgb(45, 43, 85);

        assert_eq!(color.ratatui(), Color::Rgb(45, 43, 85));
        assert_eq!(color.tmux(), "#2d2b55");
    }

    #[test]
    fn maps_terminal_default_and_bright_ansi_colors() {
        assert_eq!(ThemeColor::Default.ratatui(), Color::Reset);
        assert_eq!(ThemeColor::Default.tmux(), "default");
        assert_eq!(ThemeColor::BrightBlack.ratatui(), Color::DarkGray);
        assert_eq!(ThemeColor::BrightBlack.tmux(), "brightblack");
    }

    #[test]
    fn parses_config_hex_transparency_and_ansi_names() {
        assert_eq!(
            ThemeColor::parse("#2d2b55"),
            Some(ThemeColor::Rgb(45, 43, 85))
        );
        assert_eq!(
            ThemeColor::parse("2D2B55"),
            Some(ThemeColor::Rgb(45, 43, 85))
        );
        assert_eq!(ThemeColor::parse("transparent"), Some(ThemeColor::Default));
        assert_eq!(
            ThemeColor::parse("bright-blue"),
            Some(ThemeColor::BrightBlue)
        );
        assert_eq!(ThemeColor::parse("#xyzxyz"), None);
    }
}
