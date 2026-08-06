use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

const COMPAT_CONFIG_DIRECTORY: &str = "tmux-palette";

pub fn resolve_config_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    resolve_config_dir_from(
        explicit,
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn resolve_config_dir_from(
    explicit: Option<&Path>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.as_os_str().is_empty() {
            return Err(Error::EmptyConfigDirectory);
        }
        return Ok(path.to_path_buf());
    }

    if let Some(xdg) = non_empty(xdg_config_home) {
        return Ok(PathBuf::from(xdg).join(COMPAT_CONFIG_DIRECTORY));
    }

    if let Some(home) = non_empty(home) {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join(COMPAT_CONFIG_DIRECTORY));
    }

    dirs::config_dir()
        .map(|path| path.join(COMPAT_CONFIG_DIRECTORY))
        .ok_or(Error::ConfigDirectoryUnavailable)
}

fn non_empty(value: Option<&OsStr>) -> Option<OsString> {
    value.filter(|value| !value.is_empty()).map(OsStr::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_directory_has_highest_precedence() {
        let path = resolve_config_dir_from(
            Some(Path::new("/custom/config")),
            Some(OsStr::new("/xdg")),
            Some(OsStr::new("/home/test")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/custom/config"));
    }

    #[test]
    fn xdg_directory_preserves_legacy_config_name() {
        let path = resolve_config_dir_from(
            None,
            Some(OsStr::new("/xdg")),
            Some(OsStr::new("/home/test")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/xdg/tmux-palette"));
    }

    #[test]
    fn home_is_used_when_xdg_is_empty() {
        let path =
            resolve_config_dir_from(None, Some(OsStr::new("")), Some(OsStr::new("/home/test")))
                .unwrap();

        assert_eq!(path, PathBuf::from("/home/test/.config/tmux-palette"));
    }

    #[test]
    fn rejects_an_explicit_empty_path() {
        let error = resolve_config_dir_from(Some(Path::new("")), None, None).unwrap_err();

        assert!(matches!(error, Error::EmptyConfigDirectory));
    }
}
