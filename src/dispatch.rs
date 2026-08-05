use std::path::Path;

use crate::model::Action;
use crate::{Error, Result};

pub fn write_action(action: &Action, path: &Path) -> Result<bool> {
    let Some(encoded) = encode_action(action) else {
        return Ok(false);
    };

    std::fs::write(path, encoded).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(true)
}

fn encode_action(action: &Action) -> Option<String> {
    match action {
        Action::Tmux(command) => Some(format!("tmux:{command}")),
        Action::Shell(command) => Some(format!("shell:{command}")),
        Action::Popup(_) | Action::Palette(_) | Action::ApplyTheme(_) | Action::None => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    fn temp_file() -> std::path::PathBuf {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tmux-ratlette-dispatch-test-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn writes_tmux_commands_for_the_wrapper() {
        let path = temp_file();

        assert!(write_action(&Action::tmux("split-window -h"), &path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "tmux:split-window -h");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn does_not_encode_unimplemented_nested_palettes() {
        let path = temp_file();

        assert!(!write_action(&Action::palette("themes"), &path).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn reports_the_dispatch_path_when_writing_fails() {
        let path = std::env::temp_dir();

        let error = write_action(&Action::tmux("display-panes"), &path).unwrap_err();

        assert!(error.to_string().contains(&path.display().to_string()));
    }
}
