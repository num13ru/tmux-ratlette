use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::model::{Action, Item, Palette, ThemeColor};

const DEFAULT_WIDTH: u16 = 90;
const DEFAULT_MAX_HEIGHT: u16 = 28;
const DEFAULT_PAD_X: u16 = 3;
const DEFAULT_MOBILE_WIDTH: u16 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NavigationConfig {
    pub wrap_at_list_ends: bool,
    pub vim_keys: bool,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        Self {
            wrap_at_list_ends: true,
            vim_keys: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum EscapeBehavior {
    #[default]
    Back,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SizingConfig {
    pub width: u16,
    pub max_height: u16,
    pub pad_x: u16,
    pub mobile_width: u16,
    pub border: String,
    pub body_style: Option<String>,
    pub border_style: Option<String>,
    pub popup_border: String,
    pub popup_body_style: Option<String>,
    pub popup_border_style: Option<String>,
    pub popup_width: String,
    pub popup_height: String,
    pub popup_pad_x: u16,
    pub popup_pad_y: u16,
    pub escape: EscapeBehavior,
}

impl Default for SizingConfig {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            max_height: DEFAULT_MAX_HEIGHT,
            pad_x: DEFAULT_PAD_X,
            mobile_width: DEFAULT_MOBILE_WIDTH,
            border: "none".to_owned(),
            body_style: None,
            border_style: None,
            popup_border: "none".to_owned(),
            popup_body_style: None,
            popup_border_style: None,
            popup_width: "80%".to_owned(),
            popup_height: "80%".to_owned(),
            popup_pad_x: 0,
            popup_pad_y: 0,
            escape: EscapeBehavior::Back,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RuntimeConfig {
    pub navigation: NavigationConfig,
    pub sizing: SizingConfig,
    pub warnings: Vec<String>,
}

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

pub(crate) fn runtime(config_dir: &Path) -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    match load_json::<RawNavigation>(&config_dir.join("navigation.json")) {
        Ok(Some(raw)) => config.navigation = raw.into_config(),
        Ok(None) => {}
        Err(error) => config.warnings.push(error),
    }
    match load_json::<RawSizing>(&config_dir.join("sizing.json")) {
        Ok(Some(raw)) => match raw.into_config() {
            Ok(sizing) => config.sizing = sizing,
            Err(error) => config
                .warnings
                .push(warning(&config_dir.join("sizing.json"), error)),
        },
        Ok(None) => {}
        Err(error) => config.warnings.push(error),
    }
    config
}

pub(crate) fn custom_palette(
    name: &str,
    config_dir: Option<&Path>,
    base_commands: &[Item],
) -> Option<Palette> {
    let config_dir = config_dir?;
    if !safe_palette_name(name) {
        return Some(custom_palette_error(
            name,
            warning(
                &config_dir.join("palettes"),
                format!("unsafe palette name {name:?}; expected one filename component"),
            ),
        ));
    }
    let path = config_dir.join("palettes").join(format!("{name}.json"));
    let raw = match load_json::<RawCustomPalette>(&path) {
        Ok(Some(raw)) => raw,
        Ok(None) => return None,
        Err(error) => return Some(custom_palette_error(name, error)),
    };
    match raw.into_palette(name, config_dir, base_commands) {
        Ok(palette) => Some(palette),
        Err(error) => Some(custom_palette_error(name, warning(&path, error))),
    }
}

fn safe_palette_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn custom_palette_error(name: &str, warning: String) -> Palette {
    let mut palette = Palette::new(name, name, Vec::new());
    palette.grouped = false;
    palette.empty_text = "Could not load palette configuration".to_owned();
    palette.warnings.push(warning);
    palette
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawNavigation {
    wrap_at_list_ends: Option<bool>,
    vim_keys: Option<bool>,
}

impl RawNavigation {
    fn into_config(self) -> NavigationConfig {
        let defaults = NavigationConfig::default();
        NavigationConfig {
            wrap_at_list_ends: self.wrap_at_list_ends.unwrap_or(defaults.wrap_at_list_ends),
            vim_keys: self.vim_keys.unwrap_or(defaults.vim_keys),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSizing {
    width: Option<u16>,
    max_height: Option<u16>,
    pad_x: Option<u16>,
    mobile_width: Option<u16>,
    border: Option<String>,
    body_style: Option<String>,
    border_style: Option<String>,
    popup_border: Option<String>,
    popup_body_style: Option<String>,
    popup_border_style: Option<String>,
    popup_width: Option<String>,
    popup_height: Option<String>,
    popup_pad_x: Option<u16>,
    popup_pad_y: Option<u16>,
    esc: Option<String>,
}

impl RawSizing {
    fn into_config(self) -> Result<SizingConfig, String> {
        let defaults = SizingConfig::default();
        let width = self.width.unwrap_or(defaults.width);
        let max_height = self.max_height.unwrap_or(defaults.max_height);
        if width == 0 {
            return Err("width must be greater than zero".to_owned());
        }
        if max_height == 0 {
            return Err("maxHeight must be greater than zero".to_owned());
        }
        let border =
            validated_border(self.border.as_deref().unwrap_or(&defaults.border), "border")?;
        let popup_border = validated_border(
            self.popup_border
                .as_deref()
                .unwrap_or(&defaults.popup_border),
            "popupBorder",
        )?;
        let body_style = validated_style(self.body_style, "bodyStyle")?;
        let border_style = validated_style(self.border_style, "borderStyle")?;
        let popup_body_style = validated_style(self.popup_body_style, "popupBodyStyle")?;
        let popup_border_style = validated_style(self.popup_border_style, "popupBorderStyle")?;
        let popup_width = validated_size(
            self.popup_width.as_deref().unwrap_or(&defaults.popup_width),
            "popupWidth",
        )?;
        let popup_height = validated_size(
            self.popup_height
                .as_deref()
                .unwrap_or(&defaults.popup_height),
            "popupHeight",
        )?;
        let escape = match self.esc.as_deref().unwrap_or("back") {
            "back" => EscapeBehavior::Back,
            "exit" => EscapeBehavior::Exit,
            value => return Err(format!("esc must be \"back\" or \"exit\", got {value:?}")),
        };
        Ok(SizingConfig {
            width,
            max_height,
            pad_x: self.pad_x.unwrap_or(defaults.pad_x),
            mobile_width: self.mobile_width.unwrap_or(defaults.mobile_width),
            border,
            body_style,
            border_style,
            popup_border,
            popup_body_style,
            popup_border_style,
            popup_width,
            popup_height,
            popup_pad_x: self.popup_pad_x.unwrap_or(defaults.popup_pad_x),
            popup_pad_y: self.popup_pad_y.unwrap_or(defaults.popup_pad_y),
            escape,
        })
    }
}

fn validated_border(value: &str, field: &str) -> Result<String, String> {
    match value {
        "none" | "single" | "double" | "heavy" | "rounded" | "padded" | "simple" => {
            Ok(value.to_owned())
        }
        _ => Err(format!(
            "{field} has unsupported value {value:?}; expected none, single, double, heavy, rounded, padded, or simple"
        )),
    }
}

fn validated_style(value: Option<String>, field: &str) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(format!("{field} must be a non-empty single-line string"));
    }
    Ok(Some(value))
}

fn validated_size(value: &str, field: &str) -> Result<String, String> {
    let number = value.strip_suffix('%').unwrap_or(value);
    let valid_number = number.parse::<u16>().is_ok_and(|number| number > 0);
    if !valid_number {
        return Err(format!(
            "{field} must be a positive cell count or percentage, got {value:?}"
        ));
    }
    Ok(value.to_owned())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCustomPalette {
    title: Option<String>,
    #[serde(default)]
    items: Vec<RawItem>,
    #[serde(default)]
    from: Vec<String>,
    from_category: Option<String>,
    command: Option<String>,
    grouped: Option<bool>,
    empty_text: Option<String>,
}

impl RawCustomPalette {
    fn into_palette(
        self,
        name: &str,
        config_dir: &Path,
        base_commands: &[Item],
    ) -> Result<Palette, String> {
        let command_config = load(config_dir);
        let mut all_main = base_commands.to_vec();
        all_main.extend(command_config.commands);
        let mut items = self
            .from
            .iter()
            .filter_map(|title| all_main.iter().find(|item| item.title == *title).cloned())
            .collect::<Vec<_>>();
        if let Some(category) = self.from_category.as_deref() {
            items.extend(
                all_main
                    .iter()
                    .filter(|item| item.category.as_deref() == Some(category))
                    .cloned(),
            );
        }
        items.extend(
            self.items
                .into_iter()
                .enumerate()
                .map(|(index, item)| item.into_item(index))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut palette = Palette::new(name, self.title.as_deref().unwrap_or(name), items);
        palette.grouped = self.grouped.unwrap_or(false);
        if let Some(empty_text) = self.empty_text {
            palette.empty_text = empty_text;
        }
        if let Some(command) = self.command.filter(|command| !command.is_empty()) {
            match crate::plugin_source::run(&command) {
                Ok(output) => palette.source_output = Some(output),
                Err(error) => palette.warnings.push(warning(
                    &config_dir.join("palettes").join(format!("{name}.json")),
                    error,
                )),
            }
        }
        Ok(palette)
    }
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

    #[test]
    fn runtime_config_loads_navigation_sizing_and_escape_overrides() {
        let directory = temp_directory();
        fs::write(
            directory.join("navigation.json"),
            r#"{"wrapAtListEnds":false,"vimKeys":true,"future":true}"#,
        )
        .unwrap();
        fs::write(
            directory.join("sizing.json"),
            r#"{"width":72,"maxHeight":16,"padX":2,"mobileWidth":0,"border":"rounded","bodyStyle":"bg=#010203","borderStyle":"fg=blue","popupBorder":"single","popupBodyStyle":"default","popupBorderStyle":"fg=red","popupWidth":"60%","popupHeight":"20","popupPadX":4,"popupPadY":2,"esc":"exit","future":true}"#,
        )
        .unwrap();

        let config = runtime(&directory);

        assert_eq!(
            config.navigation,
            NavigationConfig {
                wrap_at_list_ends: false,
                vim_keys: true,
            }
        );
        assert_eq!(config.sizing.width, 72);
        assert_eq!(config.sizing.max_height, 16);
        assert_eq!(config.sizing.pad_x, 2);
        assert_eq!(config.sizing.mobile_width, 0);
        assert_eq!(config.sizing.border, "rounded");
        assert_eq!(config.sizing.body_style.as_deref(), Some("bg=#010203"));
        assert_eq!(config.sizing.popup_border, "single");
        assert_eq!(config.sizing.popup_width, "60%");
        assert_eq!(config.sizing.popup_height, "20");
        assert_eq!(config.sizing.popup_pad_x, 4);
        assert_eq!(config.sizing.popup_pad_y, 2);
        assert_eq!(config.sizing.escape, EscapeBehavior::Exit);
        assert!(config.warnings.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_runtime_files_fall_back_independently_with_paths() {
        let directory = temp_directory();
        fs::write(directory.join("navigation.json"), "not json").unwrap();
        fs::write(
            directory.join("sizing.json"),
            r#"{"width":0,"border":"invalid"}"#,
        )
        .unwrap();

        let config = runtime(&directory);

        assert_eq!(config.navigation, NavigationConfig::default());
        assert_eq!(config.sizing, SizingConfig::default());
        assert_eq!(config.warnings.len(), 2);
        assert!(config.warnings[0].contains("navigation.json"));
        assert!(config.warnings[1].contains("sizing.json"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn static_custom_palette_resolves_commands_without_applying_hidden() {
        let directory = temp_directory();
        fs::create_dir(directory.join("palettes")).unwrap();
        fs::write(
            directory.join("commands.json"),
            r#"[{"title":"Custom","category":"Tools","action":{"shell":"echo custom"}}]"#,
        )
        .unwrap();
        fs::write(directory.join("hidden.json"), r#"["Hidden"]"#).unwrap();
        fs::write(directory.join("aliases.json"), r#"{"Inline":["in"]}"#).unwrap();
        fs::write(
            directory.join("palettes/favorites.json"),
            r#"{"title":"Favorites","from":["Hidden","Custom"],"fromCategory":"Tools","items":[{"title":"Inline","action":{"tmux":"display-message inline"}}],"grouped":true,"emptyText":"Nothing here","command":"printf dynamic","future":true}"#,
        )
        .unwrap();
        let base = base_palette();

        let mut palette = custom_palette("favorites", Some(&directory), &base.items).unwrap();
        apply(&mut palette, Some(&directory), false);

        assert_eq!(palette.title, "Favorites");
        assert!(palette.grouped);
        assert_eq!(palette.empty_text, "Nothing here");
        assert_eq!(
            palette
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Hidden", "Custom", "Custom", "Inline"]
        );
        assert_eq!(palette.items[3].aliases, ["in"]);
        assert_eq!(
            palette.source_output.as_deref(),
            Some(b"dynamic".as_slice())
        );
        assert!(palette.warnings.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn custom_palette_rejects_malformed_files_and_unsafe_names() {
        let directory = temp_directory();
        fs::create_dir(directory.join("palettes")).unwrap();
        fs::write(directory.join("palettes/broken.json"), "not json").unwrap();
        let base = base_palette();

        let malformed = custom_palette("broken", Some(&directory), &base.items).unwrap();
        let unsafe_name = custom_palette("../theme", Some(&directory), &base.items).unwrap();

        assert!(malformed.items.is_empty());
        assert!(malformed.warnings[0].contains("palettes/broken.json"));
        assert!(unsafe_name.items.is_empty());
        assert!(unsafe_name.warnings[0].contains("unsafe palette name"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn custom_palette_command_failures_warn_without_dropping_static_items() {
        let directory = temp_directory();
        fs::create_dir(directory.join("palettes")).unwrap();
        fs::write(
            directory.join("palettes/failing.json"),
            r#"{"command":"printf failed-source >&2; exit 9","items":[{"title":"Still here","action":{"shell":":"}}]}"#,
        )
        .unwrap();
        let base = base_palette();

        let palette = custom_palette("failing", Some(&directory), &base.items).unwrap();

        assert_eq!(palette.items.len(), 1);
        assert_eq!(palette.items[0].title, "Still here");
        assert!(palette.source_output.is_none());
        assert_eq!(palette.warnings.len(), 1);
        assert!(palette.warnings[0].contains("palettes/failing.json"));
        assert!(palette.warnings[0].contains("exit 9: failed-source"));
        fs::remove_dir_all(directory).unwrap();
    }
}
