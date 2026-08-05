use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::model::{Action, Item, Palette, ThemeColor};

#[derive(Debug, Default)]
struct CommandConfig {
    commands: Vec<Item>,
    hidden: HashSet<String>,
    aliases: HashMap<String, Vec<String>>,
    shortcuts: HashMap<String, String>,
    warnings: Vec<String>,
}

pub fn apply(palette: &mut Palette, config_dir: Option<&Path>, is_commands: bool) {
    let Some(config_dir) = config_dir else {
        return;
    };
    let config = load(config_dir);
    if is_commands {
        palette.items.extend(config.commands);
        palette
            .items
            .retain(|item| !config.hidden.contains(&item.title));
    }
    for item in &mut palette.items {
        if item.shortcut.is_none()
            && let Some(shortcut) = config.shortcuts.get(&item.title)
        {
            item.shortcut = Some(shortcut.clone());
        }
        if let Some(aliases) = config.aliases.get(&item.title) {
            item.aliases.extend(aliases.iter().cloned());
        }
    }
    palette.warnings.extend(config.warnings);
}

fn load(config_dir: &Path) -> CommandConfig {
    let mut config = CommandConfig::default();
    match load_json::<Vec<RawItem>>(&config_dir.join("commands.json")) {
        Ok(Some(items)) => match items
            .into_iter()
            .enumerate()
            .map(|(index, item)| item.into_item(index))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(items) => config.commands = items,
            Err(error) => config
                .warnings
                .push(warning(&config_dir.join("commands.json"), error)),
        },
        Ok(None) => {}
        Err(error) => config.warnings.push(error),
    }
    load_optional(
        &config_dir.join("hidden.json"),
        &mut config.hidden,
        &mut config.warnings,
    );
    load_optional(
        &config_dir.join("aliases.json"),
        &mut config.aliases,
        &mut config.warnings,
    );
    load_optional(
        &config_dir.join("shortcuts.json"),
        &mut config.shortcuts,
        &mut config.warnings,
    );
    config
}

fn load_optional<T>(path: &Path, target: &mut T, warnings: &mut Vec<String>)
where
    T: DeserializeOwned,
{
    match load_json(path) {
        Ok(Some(value)) => *target = value,
        Ok(None) => {}
        Err(error) => warnings.push(error),
    }
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(warning(path, format!("could not read file: {error}"))),
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| warning(path, format!("invalid JSON: {error}")))
}

fn warning(path: &Path, detail: impl std::fmt::Display) -> String {
    format!("Config warning in {}: {detail}", path.display())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    icon: Option<String>,
    icon_color: Option<String>,
    title: String,
    description: Option<String>,
    shortcut: Option<String>,
    category: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    action: RawAction,
}

impl RawItem {
    fn into_item(self, index: usize) -> Result<Item, String> {
        if self.title.trim().is_empty() {
            return Err(format!("item {index} has an empty title"));
        }
        if let Some(color) = self.icon_color.as_deref()
            && ThemeColor::parse(color).is_none()
        {
            return Err(format!(
                "item {index} has invalid iconColor {color:?}; expected a hex, ANSI name, or transparent"
            ));
        }
        let mut item = Item::new(&self.title, self.action.into_action(index)?);
        item.icon = self.icon;
        item.icon_color = self.icon_color;
        item.description = self.description;
        item.shortcut = self.shortcut;
        item.category = self.category;
        item.aliases = self.aliases;
        Ok(item)
    }
}

#[derive(Debug, Deserialize)]
struct RawAction {
    tmux: Option<String>,
    shell: Option<String>,
    popup: Option<String>,
    palette: Option<String>,
}

impl RawAction {
    fn into_action(self, index: usize) -> Result<Action, String> {
        let actions = [
            self.tmux.map(Action::Tmux),
            self.shell.map(Action::Shell),
            self.popup.map(Action::Popup),
            self.palette.map(Action::Palette),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let [action] = actions.as_slice() else {
            return Err(format!(
                "item {index} action must contain exactly one of tmux, shell, popup, or palette"
            ));
        };
        let empty = match action {
            Action::Tmux(value)
            | Action::Shell(value)
            | Action::Popup(value)
            | Action::Palette(value) => value.trim().is_empty(),
            Action::ApplyTheme(_) | Action::None => false,
        };
        if empty {
            return Err(format!("item {index} action cannot be empty"));
        }
        Ok(action.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temp_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tmux-ratlette-user-config-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn base_palette() -> Palette {
        let mut first = Item::new("First", Action::None);
        first.aliases.push("built-in".to_owned());
        first.shortcut = Some("built-in-key".to_owned());
        Palette::new(
            "commands",
            "Commands",
            vec![first, Item::new("Hidden", Action::None)],
        )
    }

    #[test]
    fn merges_commands_hidden_aliases_and_shortcuts_with_legacy_precedence() {
        let directory = temp_directory();
        fs::write(
            directory.join("commands.json"),
            r#"[{"title":"Custom","icon":"+","category":"Tools","unknown":true,"action":{"shell":"echo hi","ignored":1}}]"#,
        )
        .unwrap();
        fs::write(directory.join("hidden.json"), r#"["Hidden"]"#).unwrap();
        fs::write(
            directory.join("aliases.json"),
            r#"{"First":["one"],"Custom":["c"]}"#,
        )
        .unwrap();
        fs::write(
            directory.join("shortcuts.json"),
            r#"{"First":"override","Custom":"C-c"}"#,
        )
        .unwrap();
        let mut palette = base_palette();

        apply(&mut palette, Some(&directory), true);

        assert_eq!(
            palette
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Custom"]
        );
        assert_eq!(palette.items[0].aliases, ["built-in", "one"]);
        assert_eq!(palette.items[0].shortcut.as_deref(), Some("built-in-key"));
        assert_eq!(palette.items[1].aliases, ["c"]);
        assert_eq!(palette.items[1].shortcut.as_deref(), Some("C-c"));
        assert!(matches!(palette.items[1].action, Action::Shell(ref value) if value == "echo hi"));
        assert!(palette.warnings.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_files_warn_independently_and_preserve_other_valid_files() {
        let directory = temp_directory();
        fs::write(directory.join("commands.json"), "not json").unwrap();
        fs::write(directory.join("hidden.json"), r#"["Hidden"]"#).unwrap();
        fs::write(directory.join("aliases.json"), r#"{"First":"wrong"}"#).unwrap();
        let mut palette = base_palette();

        apply(&mut palette, Some(&directory), true);

        assert_eq!(palette.items.len(), 1);
        assert_eq!(palette.items[0].title, "First");
        assert_eq!(palette.warnings.len(), 2);
        assert!(palette.warnings[0].contains("commands.json"));
        assert!(palette.warnings[1].contains("aliases.json"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn semantic_command_errors_reject_only_commands_json() {
        let directory = temp_directory();
        fs::write(
            directory.join("commands.json"),
            r#"[{"title":"Broken","action":{"tmux":"x","shell":"y"}}]"#,
        )
        .unwrap();
        let mut palette = base_palette();

        apply(&mut palette, Some(&directory), true);

        assert_eq!(palette.items.len(), 2);
        assert_eq!(palette.warnings.len(), 1);
        assert!(palette.warnings[0].contains("exactly one"));
        fs::remove_dir_all(directory).unwrap();
    }
}
