mod commands;
mod find_pane;
mod move_pane;
mod themes;

use std::path::Path;

use crate::model::Palette;

pub(crate) fn filter_find_pane(items: &[crate::model::Item], query: &str) -> Vec<usize> {
    find_pane::filter_indices(items, query)
}

pub fn load(name: &str, config_dir: Option<&Path>) -> Option<Palette> {
    let mut palette = match name {
        "commands" => Some(commands::palette()),
        "find-pane" => Some(find_pane::palette()),
        "move-pane" => Some(move_pane::palette()),
        "themes" => Some(themes::palette(config_dir)),
        _ => {
            let base_commands = commands::palette();
            crate::user_config::custom_palette(name, config_dir, &base_commands.items)
        }
    }?;
    let (theme, theme_warning) = crate::themes::active_theme_with_warning(config_dir);
    palette.theme = theme;
    if let Some(warning) = theme_warning {
        palette.warnings.push(warning);
    }
    crate::user_config::apply(&mut palette, config_dir, name == "commands");
    Some(palette)
}
