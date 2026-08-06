use std::path::Path;

use crate::model::{Action, Item, ItemData, Palette, ThemeItem};
use crate::themes;

const CUSTOM_THEME_DOCS: &str = "https://github.com/eduwass/tmux-palette#custom-themes";

pub fn palette(config_dir: Option<&Path>) -> Palette {
    let mut items = themes::list(config_dir)
        .into_iter()
        .map(|entry| {
            let mut item = Item::new(&entry.name, Action::ApplyTheme(entry.slug.clone())).icon("●");
            if entry.user_defined {
                item.description = Some("custom".to_owned());
            }
            item.aliases.push(entry.slug.clone());
            item.data = ItemData::Theme(ThemeItem {
                slug: entry.slug,
                theme: entry.theme,
            });
            item
        })
        .collect::<Vec<_>>();
    let mut docs = Item::new(
        "Add custom theme...",
        Action::Shell(format!(
            "open '{}' || xdg-open '{}'",
            CUSTOM_THEME_DOCS, CUSTOM_THEME_DOCS
        )),
    )
    .icon("+")
    .description("Open setup instructions");
    docs.aliases = ["custom", "theme", "docs"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    items.push(docs);

    let mut palette = Palette::new("themes", "Themes", items);
    palette.grouped = false;
    palette.empty_text = "No themes found".to_owned();
    palette.theme = themes::active_theme(config_dir);
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sorted_bundled_theme_items_and_custom_docs_action() {
        let palette = palette(None);

        assert_eq!(palette.items.len(), 14);
        assert_eq!(palette.items[0].title, "Ayu Dark");
        assert_eq!(palette.items[0].aliases, ["ayu-dark"]);
        assert!(matches!(
            palette.items[0].action,
            Action::ApplyTheme(ref slug) if slug == "ayu-dark"
        ));
        assert_eq!(palette.items.last().unwrap().title, "Add custom theme...");
        assert!(matches!(
            palette.items.last().unwrap().action,
            Action::Shell(_)
        ));
    }
}
