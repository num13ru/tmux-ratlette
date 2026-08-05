use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

use crate::model::{Theme, ThemeColor};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct BundledTheme {
    pub slug: &'static str,
    pub name: &'static str,
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub slug: String,
    pub name: String,
    pub theme: Theme,
    pub user_defined: bool,
}

pub const SHADES_OF_PURPLE: Theme =
    rgb_theme(0x1e1d40, 0x2d2b55, 0x504d7a, 0xffffff, 0xa599e9, 0xfad000);

pub const BUNDLED_THEMES: &[BundledTheme] = &[
    BundledTheme {
        slug: "shades-of-purple",
        name: "Shades of Purple",
        theme: SHADES_OF_PURPLE,
    },
    BundledTheme {
        slug: "dracula",
        name: "Dracula",
        theme: rgb_theme(0x282a36, 0x45495d, 0x6a6f8f, 0xf8f8f2, 0xbdc3d8, 0xd6acff),
    },
    BundledTheme {
        slug: "tokyo-night",
        name: "Tokyo Night",
        theme: rgb_theme(0x1a1b26, 0x34354b, 0x53567a, 0xc0caf5, 0x99a0bf, 0x7aa2f7),
    },
    BundledTheme {
        slug: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        theme: rgb_theme(0x1e1e2e, 0x383857, 0x5a5a8b, 0xcdd6f4, 0xa6a9b9, 0x89b4fa),
    },
    BundledTheme {
        slug: "gruvbox-dark",
        name: "Gruvbox Dark",
        theme: rgb_theme(0x282828, 0x414141, 0x646464, 0xebdbb2, 0xb7ada4, 0x8ec07c),
    },
    BundledTheme {
        slug: "rose-pine",
        name: "Rosé Pine",
        theme: rgb_theme(0x191724, 0x3c3857, 0x645c8f, 0xe0def4, 0xb1aebf, 0x9ccfd8),
    },
    BundledTheme {
        slug: "nord",
        name: "Nord",
        theme: rgb_theme(0x2e3440, 0x3f4758, 0x5c677f, 0xd8dee9, 0xabb2c0, 0x88c0d0),
    },
    BundledTheme {
        slug: "solarized-dark",
        name: "Solarized Dark",
        theme: rgb_theme(0x002b36, 0x00333f, 0x00485b, 0x839496, 0x4a8897, 0x268bd2),
    },
    BundledTheme {
        slug: "kanagawa-wave",
        name: "Kanagawa Wave",
        theme: rgb_theme(0x1f1f28, 0x3a3a4b, 0x5c5c77, 0xdcd7ba, 0xb4aa6c, 0x7e9cd8),
    },
    BundledTheme {
        slug: "github-dark",
        name: "GitHub Dark",
        theme: rgb_theme(0x101216, 0x1e2129, 0x363c4a, 0x8b949e, 0x707a85, 0x6ca4f8),
    },
    BundledTheme {
        slug: "one-dark",
        name: "One Dark",
        theme: rgb_theme(0x21252b, 0x2f353d, 0x48505e, 0xabb2bf, 0x8691a3, 0x61afef),
    },
    BundledTheme {
        slug: "ayu-dark",
        name: "Ayu Dark",
        theme: rgb_theme(0x0b0e14, 0x242e41, 0x3f5072, 0xbfbdb6, 0x98958a, 0x53bdfa),
    },
    BundledTheme {
        slug: "terminal",
        name: "Terminal",
        theme: Theme {
            bg: ThemeColor::Default,
            panel: ThemeColor::Default,
            selected: ThemeColor::Default,
            fg: ThemeColor::Default,
            muted: ThemeColor::Default,
            accent: ThemeColor::Blue,
            selected_fg: Some(ThemeColor::Yellow),
            title_fg: Some(ThemeColor::Blue),
        },
    },
];

const fn rgb(value: u32) -> ThemeColor {
    ThemeColor::rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

const fn rgb_theme(bg: u32, panel: u32, selected: u32, fg: u32, muted: u32, accent: u32) -> Theme {
    Theme {
        bg: rgb(bg),
        panel: rgb(panel),
        selected: rgb(selected),
        fg: rgb(fg),
        muted: rgb(muted),
        accent: rgb(accent),
        selected_fg: None,
        title_fg: None,
    }
}

pub const fn default_theme() -> Theme {
    SHADES_OF_PURPLE
}

pub fn list(config_dir: Option<&Path>) -> Vec<ThemeEntry> {
    let user = config_dir.map_or_else(Vec::new, load_user_themes);
    let user_slugs = user
        .iter()
        .map(|entry| entry.slug.clone())
        .collect::<HashSet<_>>();
    let mut entries = user;
    entries.extend(
        BUNDLED_THEMES
            .iter()
            .filter(|entry| !user_slugs.contains(entry.slug))
            .map(|entry| ThemeEntry {
                slug: entry.slug.to_owned(),
                name: entry.name.to_owned(),
                theme: entry.theme,
                user_defined: false,
            }),
    );
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    entries
}

pub fn active_theme(config_dir: Option<&Path>) -> Theme {
    active_theme_with_warning(config_dir).0
}

pub(crate) fn active_theme_with_warning(config_dir: Option<&Path>) -> (Theme, Option<String>) {
    let Some(directory) = config_dir else {
        return (default_theme(), None);
    };
    let path = directory.join("theme.json");
    match path.try_exists() {
        Ok(false) => return (default_theme(), None),
        Err(error) => {
            return (
                default_theme(),
                Some(format!(
                    "Config warning: could not inspect {}: {error}",
                    path.display()
                )),
            );
        }
        Ok(true) => {}
    }
    match read_active_theme(directory) {
        Ok(theme) => (theme, None),
        Err(error) => (default_theme(), Some(format!("Config warning: {error}"))),
    }
}

pub fn save_active_theme(config_dir: &Path, slug: &str) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|error| {
        format!(
            "could not create theme directory {}: {error}",
            config_dir.display()
        )
    })?;
    let destination = config_dir.join("theme.json");
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = config_dir.join(format!(".theme.json.tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
        let json = serde_json::to_string_pretty(&serde_json::json!({ "name": slug }))
            .map_err(|error| format!("could not encode selected theme: {error}"))?;
        file.write_all(json.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &destination).map_err(|error| {
            format!(
                "could not replace {} with {}: {error}",
                destination.display(),
                temporary.display()
            )
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn load_user_themes(config_dir: &Path) -> Vec<ThemeEntry> {
    let directory = config_dir.join("themes");
    let Ok(files) = fs::read_dir(directory) else {
        return Vec::new();
    };
    files
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                .then_some(path)
        })
        .filter_map(|path| {
            let slug = path.file_stem()?.to_str()?.to_owned();
            let raw = fs::read_to_string(&path).ok()?;
            let theme = serde_json::from_str::<RawTheme>(&raw)
                .ok()?
                .into_theme()
                .ok()?;
            Some(ThemeEntry {
                name: slug.clone(),
                slug,
                theme,
                user_defined: true,
            })
        })
        .collect()
}

fn read_active_theme(config_dir: &Path) -> Result<Theme, String> {
    let path = config_dir.join("theme.json");
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let file = serde_json::from_str::<RawThemeFile>(&raw)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
    let entries = list(Some(config_dir));
    let mut theme = match file.name.as_deref() {
        Some(slug) => entries
            .iter()
            .find(|entry| entry.slug == slug)
            .map(|entry| entry.theme)
            .ok_or_else(|| format!("unknown theme {slug:?} in {}", path.display()))?,
        None => default_theme(),
    };
    file.apply_to(&mut theme)
        .map_err(|error| format!("invalid theme in {}: {error}", path.display()))?;
    Ok(theme)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTheme {
    bg: String,
    panel: String,
    selected: String,
    fg: String,
    muted: String,
    accent: String,
    selected_fg: Option<String>,
    title_fg: Option<String>,
}

impl RawTheme {
    fn into_theme(self) -> Result<Theme, String> {
        Ok(Theme {
            bg: parse_color("bg", &self.bg)?,
            panel: parse_color("panel", &self.panel)?,
            selected: parse_color("selected", &self.selected)?,
            fg: parse_color("fg", &self.fg)?,
            muted: parse_color("muted", &self.muted)?,
            accent: parse_color("accent", &self.accent)?,
            selected_fg: self
                .selected_fg
                .as_deref()
                .map(|value| parse_color("selectedFg", value))
                .transpose()?,
            title_fg: self
                .title_fg
                .as_deref()
                .map(|value| parse_color("titleFg", value))
                .transpose()?,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThemeFile {
    name: Option<String>,
    bg: Option<String>,
    panel: Option<String>,
    selected: Option<String>,
    fg: Option<String>,
    muted: Option<String>,
    accent: Option<String>,
    selected_fg: Option<String>,
    title_fg: Option<String>,
}

impl RawThemeFile {
    fn apply_to(self, theme: &mut Theme) -> Result<(), String> {
        apply_color(&mut theme.bg, "bg", self.bg)?;
        apply_color(&mut theme.panel, "panel", self.panel)?;
        apply_color(&mut theme.selected, "selected", self.selected)?;
        apply_color(&mut theme.fg, "fg", self.fg)?;
        apply_color(&mut theme.muted, "muted", self.muted)?;
        apply_color(&mut theme.accent, "accent", self.accent)?;
        if let Some(value) = self.selected_fg {
            theme.selected_fg = Some(parse_color("selectedFg", &value)?);
        }
        if let Some(value) = self.title_fg {
            theme.title_fg = Some(parse_color("titleFg", &value)?);
        }
        Ok(())
    }
}

fn apply_color(target: &mut ThemeColor, field: &str, value: Option<String>) -> Result<(), String> {
    if let Some(value) = value {
        *target = parse_color(field, &value)?;
    }
    Ok(())
}

fn parse_color(field: &str, value: &str) -> Result<ThemeColor, String> {
    ThemeColor::parse(value).ok_or_else(|| format!("invalid {field} color {value:?}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tmux-ratlette-theme-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

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

    #[test]
    fn bundled_themes_match_the_typescript_catalog() {
        assert_eq!(BUNDLED_THEMES.len(), 13);
        assert_eq!(BUNDLED_THEMES[0].slug, "shades-of-purple");
        assert_eq!(BUNDLED_THEMES[12].slug, "terminal");
        assert_eq!(BUNDLED_THEMES[12].theme.panel, ThemeColor::Default);
    }

    #[test]
    fn user_theme_overrides_a_bundled_slug_and_entries_sort_by_name() {
        let directory = temp_directory("user");
        let themes = directory.join("themes");
        fs::create_dir(&themes).unwrap();
        fs::write(
            themes.join("dracula.json"),
            r##"{"bg":"#000000","panel":"#010101","selected":"#020202","fg":"#ffffff","muted":"#aaaaaa","accent":"#ff0000"}"##,
        )
        .unwrap();
        fs::write(themes.join("broken.json"), "not json").unwrap();

        let entries = list(Some(&directory));
        let dracula = entries
            .iter()
            .find(|entry| entry.slug == "dracula")
            .unwrap();

        assert_eq!(entries.len(), 13);
        assert!(dracula.user_defined);
        assert_eq!(dracula.theme.panel.tmux(), "#010101");
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].name.to_lowercase() <= pair[1].name.to_lowercase())
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn active_theme_resolves_name_and_partial_overrides() {
        let directory = temp_directory("active");
        fs::write(
            directory.join("theme.json"),
            r##"{"name":"tokyo-night","panel":"#010203"}"##,
        )
        .unwrap();

        let theme = active_theme(Some(&directory));

        assert_eq!(theme.bg.tmux(), "#1a1b26");
        assert_eq!(theme.panel.tmux(), "#010203");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_active_theme_falls_back_to_default() {
        let directory = temp_directory("fallback");
        fs::write(directory.join("theme.json"), r#"{"accent":"nope"}"#).unwrap();

        assert_eq!(active_theme(Some(&directory)), default_theme());
        let (theme, warning) = active_theme_with_warning(Some(&directory));
        assert_eq!(theme, default_theme());
        assert!(warning.unwrap().contains("theme.json"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saves_selection_atomically_as_compatible_json() {
        let directory = temp_directory("save");

        save_active_theme(&directory, "tokyo-night").unwrap();

        assert_eq!(
            fs::read_to_string(directory.join("theme.json")).unwrap(),
            "{\n  \"name\": \"tokyo-night\"\n}\n"
        );
        assert!(fs::read_dir(&directory).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
        fs::remove_dir_all(directory).unwrap();
    }
}
