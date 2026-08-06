# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/) and the structure from
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Native Rust application covering the Commands, Find Pane, Move Pane, Themes,
  and user-defined palettes.
- Rust support for the existing configuration files, including custom commands,
  hidden items, aliases, shortcuts, navigation, sizing, themes, and generated
  palette sources.
- Bounded generated-source execution with timeouts, output limits, descendant
  cleanup, and visible non-selectable failure rows.
- Rust formatting, Clippy, test, and release-build checks in CI.

### Changed

- The tmux wrapper and TPM entry point now resolve and launch the native
  `tmux-ratlette` executable.
- Search, rendering, mouse interaction, responsive sizing, theme preview, and
  deferred tmux dispatch are implemented natively while preserving the existing
  JSON configuration paths and schema.

### Removed

- Removed the legacy implementation and its runtime and development toolchain.

### Known limitations

- The full platform, terminal-emulator, SSH, nested-tmux, and signal-cleanup
  acceptance matrix is not complete yet.
- Installation currently builds the executable from a repository checkout;
  crates.io and prebuilt releases are not available yet.

## Legacy implementation releases

Versions `0.1.0` through `0.3.0` below describe the original
`eduwass/tmux-palette` implementation. They remain here as product history and
as context for users carrying forward existing configuration.

## [0.3.0] - 2026-06-12

- New bundled "Terminal" theme: transparent backgrounds and terminal-native ANSI colors, so the palette follows your terminal's own color scheme. Pick it via Switch Theme... or `{ "name": "terminal" }` in `theme.json`.
- Theme color fields now accept `transparent` (the terminal default) and named ANSI colors (`blue`, `bright-black`, etc.) alongside hex, in any built-in, `theme.json`, or custom theme.
- New optional theme fields: `selectedFg` (active-row highlight) and `titleFg` (header title color).

## [0.2.1] - 2026-05-14

- Fix: typing an auto-alias (e.g. `ns` for "New Session") now ranks the aliased item first instead of getting outranked by items that just happen to contain the query inside their category (e.g. "Detach" matching via "Sessio**ns**").

## [0.2.0] - 2026-05-14

- Filter input now has a visible blinking caret, tinted with the active theme's accent (and retinted live as you scroll the theme picker).
- Cursor movement in the filter: Left/Right, Home/End (Ctrl+A/E), plus Alt+Left/Right and Ctrl+Left/Right for word jumps.
- Editing shortcuts in the filter: Backspace, Delete, Alt+Backspace/Ctrl+W for word-delete, Ctrl+U to kill-to-start, Ctrl+K to kill-to-end.
- Text selection with Shift + any of the cursor-movement keys (char, word, line ends). Typing replaces the selection; Backspace/Delete remove it; Esc clears it. Rendered with the theme's selected colors.
- README: noted beta status and contribution scope while the basics stabilize.

## [0.1.1] - 2026-05-13

- Fix: New Session command now switches you to the new session (was silently creating it in the background and leaving you on the current one).
- Fix: Wrapper script works on macOS's bash 3.2.
- Find Pane: cursor starts on the current pane; other panes render muted so the current one reads as the visual anchor.

## [0.1.0] - 2026-05-13

Initial public release.

- Command palette for tmux panes, windows, sessions, and config reloads.
- Nested palettes for finding panes, moving panes, and switching themes.
- Custom user config under `~/.config/tmux-palette/`.
- Custom commands via `commands.json` and hidden built-ins via `hidden.json`.
- Custom palettes from JSON files, built-in items, categories, or shell commands.
- Plugin-style command sources that emit JSON or one item per line.
- Popup actions for terminal tools like `htop`, `btop`, `lazygit`, logs, and `fzf` scripts.
- Curated built-in themes with live preview and support for custom themes.
- Mobile/narrow-terminal fullscreen mode and configurable popup sizing/borders.
- TPM and manual install paths, plus optional guided onboarding prompt.
- Example palettes for GitHub PRs, GitHub Actions, git branches, Docker logs, npm scripts, and file picking.
- CI coverage for Bun tests, TypeScript, Fallow dead-code, and Fallow duplication checks.
