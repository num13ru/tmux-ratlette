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
        _ => None,
    }?;
    if name != "themes" {
        palette.theme = crate::themes::active_theme(config_dir);
    }
    Some(palette)
}
