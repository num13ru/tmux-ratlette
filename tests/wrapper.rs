#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tmux-ratlette-wrapper-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write executable");
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make executable");
}

fn fake_tmux_script() -> &'static str {
    r##"#!/usr/bin/env bash
set -eu
case "$1" in
  display-message)
    if [ "${2:-}" = "-p" ]; then
      case "${3:-}" in
        '#{client_height}') echo "${FAKE_CLIENT_HEIGHT:-40}" ;;
        '#{client_width}') echo "${FAKE_CLIENT_WIDTH:-120}" ;;
      esac
    else
      printf 'message:%s\n' "${2:-}" >> "$FAKE_TMUX_LOG"
    fi
    ;;
  show-option) exit 0 ;;
  display-popup) printf '%s\n' "$@" > "$FAKE_TMUX_LOG" ;;
esac
"##
}

fn fake_palette_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -eu
for argument in "$@"; do
  if [ "$argument" = "--measure" ]; then
    if [ -n "${FAKE_MEASUREMENT:-}" ]; then
      printf '%s\n' "$FAKE_MEASUREMENT"
    else
      printf '10\t70\t2\tnone\tdefault\tdefault\n'
    fi
    exit 0
  fi
done
exit 2
"#
}

fn popup_argument<'a>(invocation: &'a str, flag: &str) -> Option<&'a str> {
    let mut arguments = invocation.lines();
    while let Some(argument) = arguments.next() {
        if argument == flag {
            return arguments.next();
        }
    }
    None
}

#[test]
fn wrapper_launches_configured_native_binary() {
    let temp = TempDirectory::new();
    let fake_tmux = temp.path().join("tmux");
    let fake_palette = temp.path().join("tmux-ratlette");
    let log = temp.path().join("tmux.log");
    write_executable(&fake_tmux, fake_tmux_script());
    write_executable(&fake_palette, fake_palette_script());

    let output = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/tmux-palette.sh"))
        .arg("commands")
        .env("TMUX_BIN", &fake_tmux)
        .env("TMUX_PALETTE_BIN", &fake_palette)
        .env("FAKE_TMUX_LOG", &log)
        .output()
        .expect("run wrapper");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fs::read_to_string(log).unwrap_or_else(|error| {
        panic!(
            "read fake tmux log: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(invocation.contains("display-popup"));
    assert!(invocation.contains(fake_palette.to_str().unwrap()));
    assert!(invocation.contains("exec"));
    let command = invocation.lines().last().unwrap();
    assert!(command.contains("TMUX_PALETTE_WRAPPER="));
    assert!(command.contains("/bin/tmux-palette.sh"));
    assert!(command.contains("TMUX_PALETTE_TMUX_BIN="));
    assert!(command.contains(fake_tmux.to_str().unwrap()));
}

#[test]
fn wrapper_uses_edge_to_edge_mobile_dimensions_and_padding() {
    let temp = TempDirectory::new();
    let fake_tmux = temp.path().join("tmux");
    let fake_palette = temp.path().join("tmux-ratlette");
    let log = temp.path().join("tmux.log");
    write_executable(&fake_tmux, fake_tmux_script());
    write_executable(&fake_palette, fake_palette_script());

    let output = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/tmux-palette.sh"))
        .arg("commands")
        .env("TMUX_BIN", &fake_tmux)
        .env("TMUX_PALETTE_BIN", &fake_palette)
        .env("FAKE_TMUX_LOG", &log)
        .env("FAKE_CLIENT_WIDTH", "60")
        .env("FAKE_CLIENT_HEIGHT", "20")
        .env(
            "FAKE_MEASUREMENT",
            "30\t60\t1\tnone\tbg=#2d2b55\tfg=#fad000,bg=default",
        )
        .output()
        .expect("run mobile wrapper");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fs::read_to_string(&log).unwrap();
    assert_eq!(popup_argument(&invocation, "-w"), Some("60"));
    assert_eq!(popup_argument(&invocation, "-h"), Some("20"));
    assert!(invocation.lines().any(|argument| argument == "-B"));
    let command = invocation.lines().last().unwrap();
    assert!(command.contains("TMUX_PALETTE_PADX=1"));
    assert!(command.contains("TMUX_PALETTE_BORDERED=0"));
}

#[test]
fn wrapper_passes_border_styles_and_marks_the_inner_layout_bordered() {
    let temp = TempDirectory::new();
    let fake_tmux = temp.path().join("tmux");
    let fake_palette = temp.path().join("tmux-ratlette");
    let log = temp.path().join("tmux.log");
    write_executable(&fake_tmux, fake_tmux_script());
    write_executable(&fake_palette, fake_palette_script());

    let output = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/tmux-palette.sh"))
        .arg("commands")
        .env("TMUX_BIN", &fake_tmux)
        .env("TMUX_PALETTE_BIN", &fake_palette)
        .env("FAKE_TMUX_LOG", &log)
        .env(
            "FAKE_MEASUREMENT",
            "28\t90\t3\trounded\tbg=#2d2b55\tfg=#fad000,bg=default",
        )
        .output()
        .expect("run bordered wrapper");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fs::read_to_string(&log).unwrap();
    assert_eq!(popup_argument(&invocation, "-b"), Some("rounded"));
    assert_eq!(popup_argument(&invocation, "-s"), Some("bg=#2d2b55"));
    assert_eq!(
        popup_argument(&invocation, "-S"),
        Some("fg=#fad000,bg=default")
    );
    assert_eq!(popup_argument(&invocation, "-w"), Some("90"));
    assert_eq!(popup_argument(&invocation, "-h"), Some("28"));
    assert!(!invocation.lines().any(|argument| argument == "-B"));
    let command = invocation.lines().last().unwrap();
    assert!(command.contains("TMUX_PALETTE_PADX=3"));
    assert!(command.contains("TMUX_PALETTE_BORDERED=1"));
}

#[test]
fn invalid_explicit_binary_fails_with_actionable_tmux_message() {
    let temp = TempDirectory::new();
    let fake_tmux = temp.path().join("tmux");
    let missing_palette = temp.path().join("missing-tmux-ratlette");
    let log = temp.path().join("tmux.log");
    write_executable(&fake_tmux, fake_tmux_script());

    let output = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/tmux-palette.sh"))
        .arg("--check")
        .env("TMUX_BIN", &fake_tmux)
        .env("TMUX_PALETTE_BIN", &missing_palette)
        .env("FAKE_TMUX_LOG", &log)
        .output()
        .expect("run wrapper check");

    assert!(!output.status.success());
    let message = fs::read_to_string(log).expect("read fake tmux log");
    assert!(message.contains("TMUX_PALETTE_BIN does not point to an executable"));
}
