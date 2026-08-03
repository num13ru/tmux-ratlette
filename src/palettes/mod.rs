mod commands;

use crate::model::Palette;

pub fn load(name: &str) -> Option<Palette> {
    match name {
        "commands" => Some(commands::palette()),
        _ => None,
    }
}
