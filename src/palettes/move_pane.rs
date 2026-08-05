use std::collections::HashSet;

use crate::model::{Action, Item, Palette};
use crate::tmux;

const FIELD_SEPARATOR: char = '\u{1f}';
const CURRENT_FORMAT: &str = "#{pane_id}\u{1f}#{session_name}:#{window_index}";
const WINDOW_FORMAT: &str = "#{session_name}\u{1f}#{window_index}\u{1f}#{window_name}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    session: String,
    index: String,
    name: String,
}

pub fn palette() -> Palette {
    match discover_targets() {
        Ok((current, sessions, windows)) => {
            match palette_from_output(&current, &sessions, &windows) {
                Ok(palette) => palette,
                Err(message) => error_palette(message),
            }
        }
        Err(message) => error_palette(message),
    }
}

fn discover_targets() -> Result<(String, String, String), String> {
    let current = tmux::display_current(CURRENT_FORMAT)?;
    if current.is_empty() {
        return Err("tmux did not report the source pane and window".to_owned());
    }
    let sessions = tmux::run(&["list-sessions", "-F", "#S"])?;
    let windows = tmux::run(&["list-windows", "-a", "-F", WINDOW_FORMAT])?;
    Ok((current, sessions, windows))
}

fn palette_from_output(
    current_output: &str,
    sessions_output: &str,
    windows_output: &str,
) -> Result<Palette, String> {
    let (pane_id, current_window) = parse_current(current_output)
        .ok_or_else(|| "could not parse the source pane and window from tmux".to_owned())?;
    let sessions = parse_sessions(sessions_output)?;
    let windows = parse_windows(windows_output)?;

    let mut items = Vec::new();
    for session in sessions {
        items.push(
            Item::new(
                "New window",
                Action::tmux(format!(
                    "break-pane -d -s {} -t {}",
                    tmux::quote(pane_id),
                    tmux::quote(&format!("{session}:"))
                )),
            )
            .icon("󰝰")
            .description(format!("in {session}")),
        );
    }
    for window in windows {
        let target = format!("{}:{}", window.session, window.index);
        if target == current_window {
            continue;
        }
        items.push(
            Item::new(
                window.name,
                Action::tmux(format!(
                    "join-pane -d -s {} -t {}",
                    tmux::quote(pane_id),
                    tmux::quote(&target)
                )),
            )
            .icon("󰖲")
            .description(format!("{} · {}", window.session, window.index)),
        );
    }

    let mut palette = Palette::new("move-pane", "Move Pane to...", items);
    palette.grouped = false;
    palette.empty_text = "No targets".to_owned();
    Ok(palette)
}

fn error_palette(message: String) -> Palette {
    let mut palette = Palette::new("move-pane", "Move Pane to...", Vec::new());
    palette.grouped = false;
    palette.empty_text = format!("Could not load targets: {message}");
    palette
}

fn parse_current(output: &str) -> Option<(&str, &str)> {
    let fields = output.split(FIELD_SEPARATOR).collect::<Vec<_>>();
    let [pane_id, current_window] = fields.as_slice() else {
        return None;
    };
    (!pane_id.is_empty() && !current_window.is_empty()).then_some((*pane_id, *current_window))
}

fn parse_sessions(output: &str) -> Result<Vec<&str>, String> {
    let mut seen = HashSet::new();
    let mut sessions = Vec::new();
    for (line_number, session) in output.lines().enumerate() {
        if session.is_empty() {
            continue;
        }
        if session.contains(FIELD_SEPARATOR) {
            return Err(format!(
                "could not parse tmux session data at output line {}",
                line_number + 1
            ));
        }
        if seen.insert(session) {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

fn parse_windows(output: &str) -> Result<Vec<Window>, String> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(line_number, line)| {
            parse_window(line).ok_or_else(|| {
                format!(
                    "could not parse tmux window data at output line {}",
                    line_number + 1
                )
            })
        })
        .collect()
}

fn parse_window(line: &str) -> Option<Window> {
    let fields = line.split(FIELD_SEPARATOR).collect::<Vec<_>>();
    let [session, index, name] = fields.as_slice() else {
        return None;
    };
    if session.is_empty() || index.is_empty() {
        return None;
    }
    Some(Window {
        session: (*session).to_owned(),
        index: (*index).to_owned(),
        name: if name.is_empty() {
            format!("window{index}")
        } else {
            (*name).to_owned()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window_line(session: &str, index: &str, name: &str) -> String {
        [session, index, name].join(&FIELD_SEPARATOR.to_string())
    }

    #[test]
    fn builds_new_window_and_join_targets_while_excluding_the_source_window() {
        let current = format!("%7{FIELD_SEPARATOR}work:1");
        let windows = [
            window_line("work", "0", "editor"),
            window_line("work", "1", "source"),
            window_line("other", "3", "logs"),
        ]
        .join("\n");

        let palette = palette_from_output(&current, "work\nother", &windows).unwrap();

        assert_eq!(palette.title, "Move Pane to...");
        assert!(!palette.grouped);
        assert_eq!(palette.empty_text, "No targets");
        assert_eq!(palette.items.len(), 4);
        assert_eq!(palette.items[0].title, "New window");
        assert_eq!(palette.items[0].description.as_deref(), Some("in work"));
        assert_eq!(
            palette.items[0].action,
            Action::tmux("break-pane -d -s '%7' -t 'work:'")
        );
        assert_eq!(palette.items[2].title, "editor");
        assert_eq!(palette.items[2].description.as_deref(), Some("work · 0"));
        assert_eq!(
            palette.items[3].action,
            Action::tmux("join-pane -d -s '%7' -t 'other:3'")
        );
        assert!(palette.items.iter().all(|item| item.title != "source"));
    }

    #[test]
    fn falls_back_for_an_empty_window_name() {
        let window = parse_window(&window_line("work", "4", "")).unwrap();

        assert_eq!(window.name, "window4");
    }

    #[test]
    fn rejects_malformed_current_and_window_output() {
        assert!(parse_current("").is_none());
        assert!(parse_current("%1").is_none());
        assert!(parse_current(&format!("{FIELD_SEPARATOR}work:0")).is_none());
        assert!(parse_window("broken").is_none());
        assert!(palette_from_output("broken", "work", "broken").is_err());
    }

    #[test]
    fn deduplicates_sessions_without_reordering_them() {
        assert_eq!(
            parse_sessions("work\nother\nwork").unwrap(),
            ["work", "other"]
        );
    }

    #[test]
    fn malformed_discovery_is_presented_as_an_explicit_empty_state() {
        let palette = error_palette("tmux list-windows failed: no server".to_owned());

        assert!(palette.items.is_empty());
        assert_eq!(
            palette.empty_text,
            "Could not load targets: tmux list-windows failed: no server"
        );
    }
}
