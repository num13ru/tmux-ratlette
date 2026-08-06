use std::collections::HashSet;

use crate::fuzzy::multi_fuzzy_score;
use crate::model::{Action, FindPaneRow, Item, ItemData, Palette, PaletteFilter};
use crate::tmux;

const FIELD_SEPARATOR: char = '\u{1f}';
const PANE_FORMAT: &str = "#{session_name}\u{1f}#{window_index}\u{1f}#{pane_index}\u{1f}#{window_name}\u{1f}#{pane_title}\u{1f}#{pane_current_command}\u{1f}#{pane_current_path}\u{1f}#{pane_active}\u{1f}#{window_active}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pane {
    session: String,
    window_index: String,
    pane_index: String,
    window_name: String,
    pane_title: String,
    command: String,
    path: String,
    agent: String,
    pane_active: bool,
    window_active: bool,
    is_current: bool,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowGroup {
    index: String,
    name: String,
    panes: Vec<Pane>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionGroup {
    name: String,
    windows: Vec<WindowGroup>,
}

pub fn palette() -> Palette {
    match discover_panes() {
        Ok((current_pane, output)) => match palette_from_output(&current_pane, &output) {
            Ok(palette) => palette,
            Err(message) => error_palette(message),
        },
        Err(message) => error_palette(message),
    }
}

pub fn filter_indices(items: &[Item], query: &str) -> Vec<usize> {
    let parts = query.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return (0..items.len()).collect();
    }

    let mut sessions = HashSet::new();
    let mut windows = HashSet::new();
    let mut panes = HashSet::new();

    for item in items {
        let ItemData::FindPane(row) = &item.data else {
            continue;
        };
        let FindPaneRow::Pane {
            session,
            window_index,
            window_name,
            command,
            path,
            target,
            agent,
            ..
        } = row.as_ref()
        else {
            continue;
        };
        let haystack = [
            session.as_str(),
            item.title.as_str(),
            window_name.as_str(),
            command.as_str(),
            path.as_str(),
            target.as_str(),
            agent.as_str(),
        ]
        .join(" ");
        if multi_fuzzy_score(&haystack, &parts) > 0 {
            panes.insert(target.as_str());
            sessions.insert(session.as_str());
            windows.insert((session.as_str(), window_index.as_str()));
        }
    }

    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let visible = match &item.data {
                ItemData::FindPane(row) => match row.as_ref() {
                    FindPaneRow::Session { session, .. } => sessions.contains(session.as_str()),
                    FindPaneRow::Window {
                        session,
                        window_index,
                        ..
                    } => windows.contains(&(session.as_str(), window_index.as_str())),
                    FindPaneRow::Pane { target, .. } => panes.contains(target.as_str()),
                },
                ItemData::None | ItemData::Theme(_) => false,
            };
            visible.then_some(index)
        })
        .collect()
}

fn discover_panes() -> Result<(String, String), String> {
    let current_pane = tmux::display_current("#{session_name}:#{window_index}.#{pane_index}")?;
    if current_pane.is_empty() {
        return Err("tmux did not report the current pane".to_owned());
    }
    let panes = tmux::run(&["list-panes", "-a", "-F", PANE_FORMAT])?;
    Ok((current_pane, panes))
}

fn palette_from_output(current_pane: &str, output: &str) -> Result<Palette, String> {
    let mut panes = Vec::new();
    for (line_number, line) in output.lines().filter(|line| !line.is_empty()).enumerate() {
        panes.push(parse_pane_line(line, current_pane).ok_or_else(|| {
            format!(
                "could not parse pane data from tmux at output line {}",
                line_number + 1
            )
        })?);
    }

    let current_session = current_pane.split(':').next().unwrap_or_default();
    let groups = group_panes(panes);
    let mut items = Vec::new();
    for session in groups {
        let all_panes = session
            .windows
            .iter()
            .flat_map(|window| window.panes.iter())
            .collect::<Vec<_>>();
        let focused = all_panes
            .iter()
            .copied()
            .find(|pane| pane.pane_active && pane.window_active)
            .or_else(|| all_panes.first().copied());
        let mut session_item = Item::new(
            &session.name,
            Action::tmux(format!("switch-client -t {}", tmux::quote(&session.name))),
        );
        session_item.selectable = false;
        session_item.data = ItemData::FindPane(Box::new(FindPaneRow::Session {
            session: session.name.clone(),
            count: all_panes.len(),
            path: focused.map_or_else(String::new, |pane| pane.path.clone()),
            is_current: session.name == current_session,
        }));
        items.push(session_item);

        let last_window = session.windows.len().saturating_sub(1);
        for (window_position, window) in session.windows.into_iter().enumerate() {
            let is_last_window = window_position == last_window;
            let window_prefix = format!("  {} ", if is_last_window { "└─" } else { "├─" });
            if window.panes.len() == 1 {
                if let Some(pane) = window.panes.into_iter().next() {
                    items.push(pane_item(pane, window_prefix));
                }
                continue;
            }

            let mut window_item = Item::new(
                &window.name,
                Action::tmux(format!(
                    "select-window -t {} \\; switch-client -t {}",
                    tmux::quote(&format!("{}:{}", session.name, window.index)),
                    tmux::quote(&session.name)
                )),
            );
            window_item.selectable = false;
            window_item.data = ItemData::FindPane(Box::new(FindPaneRow::Window {
                session: session.name.clone(),
                window_index: window.index.clone(),
                tree_prefix: window_prefix,
            }));
            items.push(window_item);

            let last_pane = window.panes.len().saturating_sub(1);
            let pane_prefix_base = if is_last_window { "      " } else { "  │   " };
            for (pane_position, pane) in window.panes.into_iter().enumerate() {
                let branch = if pane_position == last_pane {
                    "└─ "
                } else {
                    "├─ "
                };
                items.push(pane_item(pane, format!("{pane_prefix_base}{branch}")));
            }
        }
    }

    let initial_selected = items.iter().position(|item| match &item.data {
        ItemData::FindPane(row) => matches!(
            row.as_ref(),
            FindPaneRow::Pane {
                is_current: true,
                ..
            }
        ),
        ItemData::None | ItemData::Theme(_) => false,
    });
    let mut palette = Palette::new("find-pane", "Find Pane", items);
    palette.grouped = false;
    palette.empty_text = "No panes".to_owned();
    palette.filter = PaletteFilter::FindPaneTree;
    palette.initial_selected = initial_selected;
    Ok(palette)
}

fn error_palette(message: String) -> Palette {
    let mut palette = Palette::new("find-pane", "Find Pane", Vec::new());
    palette.grouped = false;
    palette.empty_text = format!("Could not load panes: {message}");
    palette.filter = PaletteFilter::FindPaneTree;
    palette
}

fn parse_pane_line(line: &str, current_pane: &str) -> Option<Pane> {
    let fields = line.split(FIELD_SEPARATOR).collect::<Vec<_>>();
    let [
        session,
        window_index,
        pane_index,
        window_name,
        pane_title,
        command,
        path,
        pane_active,
        window_active,
    ] = fields.as_slice()
    else {
        return None;
    };
    if session.is_empty() || window_index.is_empty() || pane_index.is_empty() {
        return None;
    }
    let target = format!("{session}:{window_index}.{pane_index}");
    let pane_title = if pane_title.is_empty() {
        format!("pane{pane_index}")
    } else {
        (*pane_title).to_owned()
    };
    Some(Pane {
        session: (*session).to_owned(),
        window_index: (*window_index).to_owned(),
        pane_index: (*pane_index).to_owned(),
        window_name: if window_name.is_empty() {
            format!("window{window_index}")
        } else {
            (*window_name).to_owned()
        },
        agent: detect_agent(command, &pane_title).to_owned(),
        pane_title,
        command: (*command).to_owned(),
        path: (*path).to_owned(),
        pane_active: *pane_active == "1",
        window_active: *window_active == "1",
        is_current: target == current_pane,
        target,
    })
}

fn group_panes(panes: Vec<Pane>) -> Vec<SessionGroup> {
    let mut sessions: Vec<SessionGroup> = Vec::new();
    for pane in panes {
        let session_position = sessions
            .iter()
            .position(|session| session.name == pane.session)
            .unwrap_or_else(|| {
                sessions.push(SessionGroup {
                    name: pane.session.clone(),
                    windows: Vec::new(),
                });
                sessions.len() - 1
            });
        let windows = &mut sessions[session_position].windows;
        let window_position = windows
            .iter()
            .position(|window| window.index == pane.window_index)
            .unwrap_or_else(|| {
                windows.push(WindowGroup {
                    index: pane.window_index.clone(),
                    name: pane.window_name.clone(),
                    panes: Vec::new(),
                });
                windows.len() - 1
            });
        windows[window_position].panes.push(pane);
    }
    sessions
}

fn pane_item(pane: Pane, tree_prefix: String) -> Item {
    let window_target = format!("{}:{}", pane.session, pane.window_index);
    let mut item = Item::new(
        &pane.pane_title,
        Action::tmux(format!(
            "select-pane -t {} \\; select-window -t {} \\; switch-client -t {}",
            tmux::quote(&pane.target),
            tmux::quote(&window_target),
            tmux::quote(&pane.session)
        )),
    );
    item.data = ItemData::FindPane(Box::new(FindPaneRow::Pane {
        session: pane.session,
        window_index: pane.window_index,
        pane_index: pane.pane_index,
        window_name: pane.window_name,
        tree_prefix,
        command: pane.command,
        path: pane.path,
        target: pane.target,
        agent: pane.agent,
        pane_active: pane.pane_active,
        is_current: pane.is_current,
    }));
    item
}

fn detect_agent<'a>(command: &'a str, title: &'a str) -> &'a str {
    if matches!(
        command,
        "claude" | "codex" | "aider" | "cursor-agent" | "opencode" | "gemini" | "ollama"
    ) {
        return command;
    }
    if title.starts_with("OC | ") || title.starts_with("OC|") {
        return "opencode";
    }
    let trimmed = title.trim_start();
    let mut characters = trimmed.chars();
    if characters
        .next()
        .is_some_and(|character| "*✳⠂⠐⠁⠉⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(character))
        && characters.next().is_some_and(char::is_whitespace)
    {
        return "claude";
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_line(fields: [&str; 9]) -> String {
        fields.join(&FIELD_SEPARATOR.to_string())
    }

    fn sample_output() -> String {
        [
            pane_line([
                "work",
                "0",
                "0",
                "editor",
                "code",
                "nvim",
                "/home/me/project",
                "1",
                "1",
            ]),
            pane_line([
                "work",
                "0",
                "1",
                "editor",
                "tests",
                "cargo",
                "/home/me/project",
                "0",
                "1",
            ]),
            pane_line([
                "other",
                "2",
                "0",
                "agent",
                "OC | task",
                "zsh",
                "/tmp",
                "1",
                "1",
            ]),
        ]
        .join("\n")
    }

    #[test]
    fn builds_tree_and_selects_current_pane() {
        let palette = palette_from_output("work:0.1", &sample_output()).unwrap();

        assert_eq!(palette.title, "Find Pane");
        assert!(!palette.grouped);
        assert_eq!(palette.filter, PaletteFilter::FindPaneTree);
        assert_eq!(palette.items.len(), 6);
        assert_eq!(palette.initial_selected, Some(3));
        assert_eq!(palette.items[3].title, "tests");
        assert_eq!(
            palette.items[3].action,
            Action::tmux(
                "select-pane -t 'work:0.1' \\; select-window -t 'work:0' \\; switch-client -t 'work'"
            )
        );
        assert!(!palette.items[0].selectable);
        assert!(matches!(
            &palette.items[1].data,
            ItemData::FindPane(row) if matches!(row.as_ref(), FindPaneRow::Window { .. })
        ));
        assert!(matches!(
            &palette.items[5].data,
            ItemData::FindPane(row)
                if matches!(row.as_ref(), FindPaneRow::Pane { agent, .. } if agent == "opencode")
        ));
    }

    #[test]
    fn tree_filter_keeps_matching_ancestors_in_source_order() {
        let palette = palette_from_output("work:0.0", &sample_output()).unwrap();
        let visible = filter_indices(&palette.items, "cargo project");
        let titles = visible
            .iter()
            .map(|index| palette.items[*index].title.as_str())
            .collect::<Vec<_>>();

        assert_eq!(titles, ["work", "editor", "tests"]);

        let visible = filter_indices(&palette.items, "agent");
        let titles = visible
            .iter()
            .map(|index| palette.items[*index].title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, ["other", "OC | task"]);
    }

    #[test]
    fn falls_back_for_empty_titles_and_names() {
        let line = pane_line(["s", "3", "4", "", "", "zsh", "", "0", "0"]);
        let pane = parse_pane_line(&line, "s:3.4").unwrap();

        assert_eq!(pane.window_name, "window3");
        assert_eq!(pane.pane_title, "pane4");
        assert!(pane.is_current);
    }

    #[test]
    fn rejects_missing_or_extra_fields() {
        assert!(parse_pane_line("broken", "s:0.0").is_none());
        let extra = format!(
            "{}{}extra",
            pane_line(["s", "0", "0", "w", "p", "zsh", "/tmp", "1", "1"]),
            FIELD_SEPARATOR
        );
        assert!(parse_pane_line(&extra, "s:0.0").is_none());
        assert!(palette_from_output("s:0.0", "broken").is_err());
    }

    #[test]
    fn malformed_discovery_is_presented_as_an_explicit_empty_state() {
        let palette = error_palette("tmux list-panes failed: no server".to_owned());

        assert!(palette.items.is_empty());
        assert_eq!(
            palette.empty_text,
            "Could not load panes: tmux list-panes failed: no server"
        );
    }

    #[test]
    fn detects_supported_agents_without_false_prefix_matches() {
        assert_eq!(detect_agent("codex", "shell"), "codex");
        assert_eq!(detect_agent("zsh", "  ✳ working"), "claude");
        assert_eq!(detect_agent("zsh", "OC|task"), "opencode");
        assert_eq!(detect_agent("codex-helper", "shell"), "");
    }
}
