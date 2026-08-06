use crate::model::{Action, Item, Palette};

pub fn palette() -> Palette {
    Palette::new(
        "commands",
        "Commands",
        vec![
            nested("󰍉", "Panes", "Find Pane", "find-pane", None),
            tmux(
                "",
                "Panes",
                "Split Horizontal",
                "split-window -h -c '#{pane_current_path}'",
                Some("side by side"),
            ),
            tmux(
                "",
                "Panes",
                "Split Vertical",
                "split-window -v -c '#{pane_current_path}'",
                Some("stacked"),
            ),
            tmux("󰅖", "Panes", "Close Pane", "kill-pane", None),
            tmux(
                "󰒉",
                "Panes",
                "Close Other Panes",
                "confirm-before -p 'kill all other panes? (y/n)' 'kill-pane -a'",
                None,
            ),
            tmux("󰁔", "Panes", "Next Pane", "select-pane -t +1", None),
            tmux("󰁍", "Panes", "Previous Pane", "select-pane -t -1", None),
            tmux("󰎠", "Panes", "Display Pane Numbers", "display-panes", None),
            tmux("󰓡", "Panes", "Cycle Pane Layout", "next-layout", None),
            tmux("󰁝", "Panes", "Swap Pane Up", "swap-pane -U", None),
            tmux("󰁅", "Panes", "Swap Pane Down", "swap-pane -D", None),
            tmux("󰍉", "Panes", "Zoom / Unzoom", "resize-pane -Z", None),
            tmux(
                "󰆏",
                "Panes",
                "Enter Copy Mode",
                "copy-mode",
                Some("scrollback / select"),
            ),
            tmux(
                "󰏫",
                "Panes",
                "Rename Pane",
                "command-prompt -I '#{pane_title}' 'select-pane -T \"%1\"'",
                None,
            ),
            nested("󰁁", "Panes", "Move Pane to...", "move-pane", None),
            tmux("󰘖", "Panes", "Break to New Window", "break-pane", None),
            tmux(
                "󰝰",
                "Windows",
                "New Window",
                "new-window -c '#{pane_current_path}'",
                None,
            ),
            tmux("󰁔", "Windows", "Next Window", "next-window", None),
            tmux("󰁍", "Windows", "Previous Window", "previous-window", None),
            tmux("󰋚", "Windows", "Last Window", "last-window", None),
            tmux(
                "󰏫",
                "Windows",
                "Rename Window",
                "command-prompt -I '#W' 'rename-window -- \"%%\"'",
                None,
            ),
            tmux(
                "󰅖",
                "Windows",
                "Close Window",
                "confirm-before -p 'kill window? (y/n)' kill-window",
                None,
            ),
            tmux("󱂬", "Sessions", "Choose Session", "choose-tree -Zs", None),
            tmux(
                "󰐕",
                "Sessions",
                "New Session",
                "command-prompt -p 'New session name:' 'new-session -d -s \"%1\" ; switch-client -t \"%1\"'",
                None,
            ),
            tmux(
                "󰏫",
                "Sessions",
                "Rename Session",
                "command-prompt -I '#S' 'rename-session -- \"%%\"'",
                None,
            ),
            tmux("󰁔", "Sessions", "Next Session", "switch-client -n", None),
            tmux(
                "󰁍",
                "Sessions",
                "Previous Session",
                "switch-client -p",
                None,
            ),
            tmux("󰍃", "Sessions", "Detach", "detach-client", None),
            tmux(
                "󰆴",
                "Sessions",
                "Kill Session",
                "confirm-before -p 'kill session #S? (y/n)' kill-session",
                None,
            ),
            tmux(
                "󰑓",
                "System",
                "Reload Config",
                "source-file ~/.tmux.conf ; display-message 'Config reloaded'",
                None,
            ),
            nested(
                "",
                "Appearance",
                "Switch Theme...",
                "themes",
                Some("browse + live-preview bundled themes"),
            ),
        ],
    )
}

fn tmux(icon: &str, category: &str, title: &str, command: &str, description: Option<&str>) -> Item {
    decorate(
        Item::new(title, Action::tmux(command)),
        icon,
        category,
        description,
    )
}

fn nested(
    icon: &str,
    category: &str,
    title: &str,
    palette: &str,
    description: Option<&str>,
) -> Item {
    decorate(
        Item::new(title, Action::palette(palette)),
        icon,
        category,
        description,
    )
}

fn decorate(item: Item, icon: &str, category: &str, description: Option<&str>) -> Item {
    let item = item.icon(icon).category(category);
    match description {
        Some(description) => item.description(description),
        None => item,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_every_legacy_command() {
        let palette = palette();

        assert_eq!(palette.items.len(), 31);
        assert_eq!(palette.items.first().unwrap().title, "Find Pane");
        assert_eq!(palette.items.last().unwrap().title, "Switch Theme...");
    }

    #[test]
    fn new_session_creates_and_switches_to_the_session() {
        let palette = palette();
        let item = palette
            .items
            .iter()
            .find(|item| item.title == "New Session")
            .unwrap();
        let Action::Tmux(command) = &item.action else {
            panic!("New Session should be a tmux action");
        };

        assert!(command.contains("new-session"));
        assert!(command.contains("switch-client"));
    }

    #[test]
    fn command_prompt_templates_do_not_reuse_double_percent() {
        let offenders = palette()
            .items
            .into_iter()
            .filter_map(|item| match item.action {
                Action::Tmux(command)
                    if command.contains("command-prompt") && command.matches("%%").count() > 1 =>
                {
                    Some(item.title)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(offenders.is_empty(), "offenders: {offenders:?}");
    }
}
