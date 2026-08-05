mod commands;
pub(crate) mod find_pane;
mod move_pane;

use crate::model::Palette;

pub fn load(name: &str) -> Option<Palette> {
    match name {
        "commands" => Some(commands::palette()),
        "find-pane" => Some(find_pane::palette()),
        "move-pane" => Some(move_pane::palette()),
        _ => None,
    }
}
