use std::path::Path;

use crate::model::{Action, PopupAction, Theme};
use crate::tmux;
use crate::user_config::SizingConfig;
use crate::{Error, Result};

pub(crate) struct PopupContext<'a> {
    pub sizing: &'a SizingConfig,
    pub theme: Theme,
    pub tmux_binary: &'a str,
    pub wrapper: &'a str,
    pub relaunch_arguments: &'a [String],
}

pub fn write_action(action: &Action, path: &Path) -> Result<bool> {
    let Some(encoded) = encode_action(action) else {
        return Ok(false);
    };

    std::fs::write(path, encoded).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(true)
}

fn encode_action(action: &Action) -> Option<String> {
    match action {
        Action::Tmux(command) => Some(format!("tmux:{command}")),
        Action::Shell(command) => Some(format!("shell:{command}")),
        Action::Popup(_) | Action::Palette(_) | Action::ApplyTheme(_) | Action::None => None,
    }
}

pub(crate) fn popup_shell_command(
    action: &PopupAction,
    context: PopupContext<'_>,
) -> std::result::Result<String, String> {
    if action.command.trim().is_empty() {
        return Err("popup action command cannot be empty".to_owned());
    }
    if context.tmux_binary.is_empty() {
        return Err("tmux executable path is empty".to_owned());
    }
    if context.wrapper.is_empty() {
        return Err("palette wrapper path is empty".to_owned());
    }

    let sizing = context.sizing;
    let pad_x = action.pad_x.unwrap_or(sizing.popup_pad_x);
    let pad_y = action.pad_y.unwrap_or(sizing.popup_pad_y);
    let width = popup_dimension(
        action.width.as_deref().unwrap_or(&sizing.popup_width),
        "client_width",
        pad_x,
        context.tmux_binary,
    )?;
    let height = popup_dimension(
        action.height.as_deref().unwrap_or(&sizing.popup_height),
        "client_height",
        pad_y,
        context.tmux_binary,
    )?;
    let border = action.border.as_deref().unwrap_or(&sizing.popup_border);
    if !matches!(
        border,
        "none" | "single" | "double" | "heavy" | "rounded" | "padded" | "simple"
    ) {
        return Err(format!("unsupported popup border {border:?}"));
    }
    let body_style = sizing
        .popup_body_style
        .clone()
        .unwrap_or_else(|| context.theme.tmux_body_style());
    let border_style = sizing
        .popup_border_style
        .clone()
        .unwrap_or_else(|| context.theme.tmux_border_style());
    let tmux = tmux::quote(context.tmux_binary);
    let popup_flags = if border == "none" {
        format!("-B -s {}", tmux::quote(&body_style))
    } else {
        format!(
            "-b {} -s {} -S {}",
            tmux::quote(border),
            tmux::quote(&body_style),
            tmux::quote(&border_style)
        )
    };
    if context.relaunch_arguments.is_empty() {
        return Err("palette relaunch arguments are empty".to_owned());
    }
    let relaunch = std::iter::once(tmux::quote(context.wrapper))
        .chain(
            context
                .relaunch_arguments
                .iter()
                .map(|argument| tmux::quote(argument)),
        )
        .collect::<Vec<_>>()
        .join(" ");

    Ok(format!(
        "{tmux} display-popup -E {popup_flags} -h {height} -w {width} {}; {tmux} run-shell -b {}",
        tmux::quote(&action.command),
        tmux::quote(&relaunch)
    ))
}

fn popup_dimension(
    specification: &str,
    axis: &str,
    padding: u16,
    tmux_binary: &str,
) -> std::result::Result<String, String> {
    let (number, percentage) = specification
        .strip_suffix('%')
        .map_or((specification, false), |number| (number, true));
    let number = number.parse::<u16>().map_err(|_| {
        format!("popup size must be a positive cell count or percentage, got {specification:?}")
    })?;
    if number == 0 {
        return Err(format!(
            "popup size must be a positive cell count or percentage, got {specification:?}"
        ));
    }
    let padding = u32::from(padding) * 2;
    if !percentage {
        return Ok(u32::from(number).saturating_sub(padding).max(1).to_string());
    }

    let query = format!(
        "{} display-message -p {}",
        tmux::quote(tmux_binary),
        tmux::quote(&format!("#{{{axis}}}"))
    );
    Ok(format!(
        "$(popup_base=$({query} 2>/dev/null); if ! [ \"$popup_base\" -ge 1 ] 2>/dev/null; then popup_base=1; fi; popup_size=$((popup_base * {number} / 100 - {padding})); if [ \"$popup_size\" -gt 0 ]; then printf '%s' \"$popup_size\"; else printf '1'; fi)"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    fn temp_file() -> std::path::PathBuf {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tmux-ratlette-dispatch-test-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn writes_tmux_commands_for_the_wrapper() {
        let path = temp_file();

        assert!(write_action(&Action::tmux("split-window -h"), &path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "tmux:split-window -h");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn builds_popup_command_with_overrides_styles_and_quoted_relaunch() {
        let sizing = SizingConfig {
            popup_border: "single".to_owned(),
            popup_body_style: Some("bg=#010203".to_owned()),
            popup_border_style: Some("fg=red".to_owned()),
            popup_width: "80%".to_owned(),
            popup_height: "80%".to_owned(),
            popup_pad_x: 1,
            popup_pad_y: 1,
            ..SizingConfig::default()
        };
        let action = PopupAction {
            command: "printf \"it's ready\"".to_owned(),
            width: Some("50%".to_owned()),
            height: Some("10".to_owned()),
            pad_x: Some(4),
            pad_y: Some(2),
            border: Some("rounded".to_owned()),
        };

        let command = popup_shell_command(
            &action,
            PopupContext {
                sizing: &sizing,
                theme: crate::themes::default_theme(),
                tmux_binary: "/opt/tmux bin",
                wrapper: "/tmp/palette's wrapper",
                relaunch_arguments: &[
                    "team tools".to_owned(),
                    "--config-dir=/tmp/config's".to_owned(),
                    "--no-mouse".to_owned(),
                ],
            },
        )
        .unwrap();

        assert!(command.starts_with("'/opt/tmux bin' display-popup -E"));
        assert!(command.contains("-b 'rounded'"));
        assert!(command.contains("-s 'bg=#010203'"));
        assert!(command.contains("-S 'fg=red'"));
        assert!(command.contains("#{client_width}"));
        assert!(command.contains("popup_base * 50 / 100 - 8"));
        assert!(command.contains("-h 6"));
        assert!(command.contains("'printf \"it'\\''s ready\"'"));
        assert!(command.contains("run-shell -b"));
        assert!(command.contains("/tmp/palette"));
        assert!(command.contains("s wrapper"));
        assert!(command.contains("team tools"));
        assert!(command.contains("config"));
        assert!(command.contains("--no-mouse"));
        assert!(
            std::process::Command::new("sh")
                .args(["-n", "-c", &command])
                .status()
                .unwrap()
                .success(),
            "{command}"
        );
    }

    #[test]
    fn popup_dimensions_clamp_fixed_sizes_and_reject_invalid_values() {
        assert_eq!(
            popup_dimension("5", "client_width", 9, "tmux").unwrap(),
            "1"
        );
        assert!(popup_dimension("0", "client_width", 0, "tmux").is_err());
        assert!(popup_dimension("wide", "client_width", 0, "tmux").is_err());

        let expression = popup_dimension("80%", "client_width", 0, "false").unwrap();
        let check = format!("value={expression}; [ \"$value\" = 1 ]");
        assert!(
            std::process::Command::new("sh")
                .args(["-c", &check])
                .status()
                .unwrap()
                .success(),
            "{check}"
        );
    }

    #[test]
    fn does_not_encode_unimplemented_nested_palettes() {
        let path = temp_file();

        assert!(!write_action(&Action::palette("themes"), &path).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn reports_the_dispatch_path_when_writing_fails() {
        let path = std::env::temp_dir();

        let error = write_action(&Action::tmux("display-panes"), &path).unwrap_err();

        assert!(error.to_string().contains(&path.display().to_string()));
    }
}
