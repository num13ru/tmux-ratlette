# tmux-ratlette Architecture Plan

## 1. Objective

Fork `eduwass/tmux-palette` and replace the Bun/TypeScript runtime with a native Rust executable while preserving the existing user experience and configuration format.

The Rust version should provide:

- one native `tmux-palette` executable
- no Bun, Node.js, `node_modules`, or runtime package installation
- compatibility with existing JSON configuration where practical
- the same built-in palettes:
  - commands
  - find-pane
  - move-pane
  - themes
- tmux popup integration
- keyboard, mouse, filtering, scrolling, theming, and nested palettes
- shell-generated palette items
- macOS and Linux support

The existing implementation runs `src/cli.ts` through Bun for both popup measurement and the interactive application. The TPM entry point also checks for Bun and runs `bun install`, despite there being no declared runtime package dependencies.

---

## 2. Distribution Strategy

### Alpha

Users clone the repository and build locally:

```bash
git clone https://github.com/<fork-owner>/tmux-palette
cd tmux-palette
cargo build --release
```

Result:

```text
target/release/tmux-palette
```

The tmux plugin entry point invokes that local binary.

Alpha installation requirements:

- Rust toolchain
- Cargo
- tmux 3.4 or newer
- a Unix-like environment

No automated binary downloads are required during alpha.

### Beta

Publish the executable crate to crates.io:

```bash
cargo install tmux-palette
```

The installed binary becomes:

```text
~/.cargo/bin/tmux-palette
```

The TPM plugin should locate the executable through `PATH`.

Beta requirements:

- Cargo only during installation
- no repository-local build
- no Bun or Node.js

A later stable release may add precompiled GitHub binaries, but that is outside the alpha and beta scope.

---

## 3. Repository Structure

Use a single Rust package initially.

```text
tmux-palette/
├── Cargo.toml
├── Cargo.lock
├── LICENSE
├── README.md
├── CHANGELOG.md
├── tmux-palette.tmux
├── bin/
│   └── tmux-palette.sh
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── app.rs
│   ├── event.rs
│   ├── terminal.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── commands.rs
│   │   ├── palettes.rs
│   │   ├── themes.rs
│   │   └── sizing.rs
│   ├── model/
│   │   ├── mod.rs
│   │   ├── action.rs
│   │   ├── item.rs
│   │   ├── palette.rs
│   │   └── theme.rs
│   ├── palette/
│   │   ├── mod.rs
│   │   ├── commands.rs
│   │   ├── find_pane.rs
│   │   ├── move_pane.rs
│   │   └── themes.rs
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── layout.rs
│   │   ├── renderer.rs
│   │   ├── search.rs
│   │   └── widgets.rs
│   ├── fuzzy.rs
│   ├── dispatch.rs
│   ├── plugin_command.rs
│   ├── tmux.rs
│   └── error.rs
├── tests/
│   ├── config.rs
│   ├── fuzzy.rs
│   ├── navigation.rs
│   ├── rendering.rs
│   └── dispatch.rs
└── examples/
    ├── commands.json
    ├── docker.json
    ├── github-prs.json
    └── npm-scripts.json
```

Do not introduce a Cargo workspace during alpha unless a genuine second crate appears. A single package keeps `cargo build` and crates.io publishing straightforward.

---

## 4. Dependency Selection

Recommended initial dependencies:

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
crossterm = "0.28"
ratatui = "0.29"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
unicode-width = "0.2"
dirs = "6"
```

Possible later dependencies:

```toml
nucleo-matcher = "0.3"
signal-hook = "0.3"
```

### Responsibilities

| Crate | Responsibility |
|---|---|
| `clap` | Commands, flags, validation and generated help |
| `ratatui` | Layout and screen rendering |
| `crossterm` | Raw terminal mode, keyboard, mouse and resize events |
| `serde` | Typed configuration structures |
| `serde_json` | Existing JSON configuration |
| `thiserror` | Structured application errors |
| `unicode-width` | Correct terminal width for Unicode text |
| `dirs` | Resolve the user configuration directory |
| `nucleo-matcher` | Optional improved fuzzy matching |
| `signal-hook` | Reliable cleanup on Unix signals |

For alpha, a custom fuzzy matcher may be ported directly to preserve existing behavior. Replacing it with another matcher too early could produce subtle ranking differences.

---

## 5. Command-Line Interface

Use `clap` derive structures rather than manually searching `process.argv`.

Proposed interface:

```text
tmux-palette commands
tmux-palette find-pane
tmux-palette move-pane
tmux-palette themes
tmux-palette palette <name>
```

Options:

```text
--category <CATEGORY>
--config-dir <PATH>
--measure
--client-width <COLUMNS>
--client-height <ROWS>
--no-mouse
--debug
```

Suggested command model:

```text
tmux-palette
├── commands
├── find-pane
├── move-pane
├── themes
├── palette <name>
└── measure <palette>
```

The default command should remain `commands` so existing invocations can stay concise:

```bash
tmux-palette
```

### Measurement compatibility

During alpha, retain a measurement subcommand because the current shell wrapper determines popup dimensions before opening it.

Example:

```bash
tmux-palette measure commands \
  --client-width 140 \
  --client-height 45
```

Output should remain shell-friendly:

```text
28	90	3	none	fg=#ffffff,bg=#111111	fg=#9580ff,bg=default
```

This preserves the current shell integration while replacing Bun.

A later version may replace tab-separated output with a structured internal tmux invocation, but that is not necessary for the first port.

---

## 6. Core Domain Model

### Item

```text
Item
├── title
├── description
├── category
├── icon
├── icon_color
├── shortcut
├── aliases
├── selectable
└── action
```

### Action

Represent action types as a Serde tagged or untagged enum compatible with existing JSON.

```text
Action
├── Tmux
├── Shell
├── Popup
├── Palette
└── None
```

Existing formats such as these should continue to work:

```json
{ "tmux": "split-window -h" }
```

```json
{ "shell": "open ." }
```

```json
{ "popup": "lazygit" }
```

```json
{ "palette": "tools" }
```

### PaletteDefinition

```text
PaletteDefinition
├── title
├── grouped
├── empty_text
├── initial_selected
├── theme
└── items
```

Runtime-only behavior such as callbacks should not be represented directly in JSON. Built-in palettes can implement behavior through Rust traits or enum-specific logic.

---

## 7. Application State

Use an explicit `App` state object.

```text
App
├── active_palette
├── palette_name
├── items
├── visible_items
├── selected_index
├── scroll_offset
├── filter
├── filter_cursor
├── selection_anchor
├── navigation_stack
├── theme
├── terminal_size
├── should_quit
└── pending_action
```

The current TypeScript implementation already maintains similar state for selection, scrolling, filtering, navigation history and live theme preview.

### Event loop

```text
initialize terminal
load palette
render frame

while not quitting:
    wait for event
    update application state
    render frame when state changed

restore terminal
dispatch selected action
exit
```

Supported events:

```text
Event
├── Key
├── Mouse
├── Resize
├── Tick
└── Signal
```

A tick is optional. Avoid periodic redraws unless they are needed.

---

## 8. Rendering Architecture

Use Ratatui for frame layout, but keep the visual model close to the existing palette.

Proposed vertical layout:

```text
┌──────────────────────────────┐
│ Header                   Esc │
│ Search input                 │
│                              │
│ Category                     │
│ Selected command             │
│ Command                      │
│ Command                      │
│                              │
│ Footer                       │
└──────────────────────────────┘
```

Rendering should be split into pure functions:

```text
render_header
render_search
render_item_list
render_item
render_category
render_footer
```

Pure rendering functions are easier to snapshot-test than a single stateful renderer.

### Important compatibility details

Preserve:

- grouped categories
- current fuzzy ranking
- highlighted selected row
- search cursor position
- keyboard navigation
- page navigation
- live theme preview
- narrow-terminal mode
- optional tmux border
- configurable horizontal padding
- Unicode icons

Do not depend on Ratatui’s alternate-screen mode when running inside `tmux display-popup` unless testing confirms that nested alternate-screen behavior is reliable. Raw mode plus direct popup rendering may be safer.

---

## 9. Input Handling

Use Crossterm events rather than manually parsing most ANSI sequences.

Default mappings:

```text
Up / Ctrl-P       previous item
Down / Ctrl-N     next item
PageUp            previous page
PageDown          next page
Enter             select
Esc               back or close
Backspace         delete previous character
Delete            delete next character
Left / Right      move search cursor
Ctrl-U            page up or clear, depending on compatibility decision
Ctrl-D            page down
```

Optional Vim mappings:

```text
Ctrl-K            previous item
Ctrl-J            next item
Ctrl-U            previous page
Ctrl-D            next page
```

Mouse support:

- click item to select
- click selected item to execute
- wheel to scroll
- click escape area to go back or close

Unknown terminal sequences should be ignored rather than crashing the application.

---

## 10. Configuration Compatibility

Continue using:

```text
~/.config/tmux-palette/
```

Supported files:

```text
commands.json
hidden.json
aliases.json
shortcuts.json
navigation.json
sizing.json
theme.json
palettes/*.json
themes/*.json
```

### Compatibility rule

The Rust fork should read existing configuration without requiring migration whenever possible.

For unsupported or malformed fields:

- ignore unknown fields by default
- report malformed required fields clearly
- include the file path in the error
- avoid terminating the whole application for one invalid optional file
- fall back to built-in defaults

Example warning:

```text
tmux-palette: unable to read ~/.config/tmux-palette/theme.json:
invalid value for "accent"; expected a hex color
```

Warnings should go to a debug log or tmux message rather than corrupting the popup display.

---

## 11. Built-In Palettes

### Commands

Responsible for pane, window, session and tmux operations.

Implementation:

```text
src/palette/commands.rs
```

The static built-in item list should be regular Rust data, not parsed from JSON during startup.

### Find Pane

Query tmux:

```bash
tmux list-panes -a
```

Parse pane identifiers, session names, window names, commands and current paths into items.

Selecting an item dispatches an appropriate tmux focus command.

### Move Pane

Load possible destination windows and panes.

Selecting a target dispatches the corresponding tmux join or move operation.

### Themes

Load bundled themes and user themes.

Selection behavior:

- moving selection previews a theme
- Enter persists the selected theme
- Escape restores the original theme

Theme persistence should use an atomic write:

```text
write temporary file
flush
rename over theme.json
```

---

## 12. Shell Plugin Sources

Custom palette commands are a core compatibility requirement.

Configuration example:

```json
{
  "title": "Git Branches",
  "command": "git branch --format='%(refname:short)'",
  "action": {
    "shell": "git switch {}"
  }
}
```

Execution model:

```text
spawn /bin/sh -c <command>
capture stdout
capture stderr
apply timeout
parse output
convert output to Item values
```

Supported output:

1. Plain text, one item per line.
2. Tab-separated icon, color and title.
3. JSON array of complete item objects.

Required safeguards:

- default timeout: 10 seconds
- maximum captured output size
- invalid UTF-8 handling
- non-zero exit status shown as an error item
- no shell interpolation performed by Rust beyond explicitly documented `{}` substitution
- preserve the user’s shell command semantics

The current implementation already supports command timeouts and both JSON and plain-text output.

---

## 13. Action Dispatch

Keep action execution outside the interactive terminal session.

Flow:

```text
user selects item
application exits raw mode
terminal state is restored
selected action is returned
shell wrapper or Rust dispatcher executes action
```

Possible implementations:

### Alpha approach

Continue using the command file currently supplied through:

```text
TMUX_PALETTE_CMD
```

Rust writes one of:

```text
tmux:<arguments>
shell:<command>
popup:<command>
palette:<name>
```

The shell wrapper reads and executes it.

This minimizes behavioral change during the port.

### Beta approach

Move dispatch into Rust:

- tmux actions invoke the located tmux binary directly
- shell actions invoke `/bin/sh -c`
- popup actions invoke `tmux display-popup`
- nested palettes stay in-process

Keep the command-file path as a compatibility fallback until the Rust dispatcher is proven stable.

---

## 14. tmux Integration

### Alpha wrapper

`bin/tmux-palette.sh` should:

1. locate the repository
2. locate `target/release/tmux-palette`
3. show an actionable message when the binary is missing
4. ask the binary for desired popup dimensions
5. open the tmux popup
6. execute the resulting action

Expected missing-build message:

```text
tmux-palette: binary not found.
Run: cd <plugin-directory> && cargo build --release
```

### Alpha TPM entry point

`tmux-palette.tmux` should:

- remove every Bun check
- never run package installation
- bind configured keys
- verify that Cargo-built binary exists
- display build instructions when missing

Example configuration remains:

```tmux
set -g @plugin '<fork-owner>/tmux-palette'
set -g @palette-key 'C-Space'
set -g @palette-find-pane-key 'M-f'
set -g @palette-move-pane-key 'M-m'
```

### Beta wrapper

Search in this order:

```text
TMUX_PALETTE_BIN
command -v tmux-palette
repository-local target/release/tmux-palette
```

This permits both crates.io installations and development builds.

Beta installation:

```bash
cargo install tmux-palette
```

TPM should not run `cargo install` automatically. Installing executables implicitly during tmux startup is surprising and can be slow or fail due to toolchain issues.

---

## 15. Error Handling

Define a central error type:

```text
PaletteError
├── Config
├── Json
├── Io
├── Terminal
├── Tmux
├── Command
├── Timeout
├── InvalidTheme
└── UnsupportedPlatform
```

Error presentation depends on context:

| Context | Presentation |
|---|---|
| Before popup opens | `tmux display-message` |
| During palette loading | visible non-selectable error item |
| Debug mode | stderr or log file |
| Terminal initialization | restore state, then print error |
| Action execution | `tmux display-message` |

Terminal cleanup must run for:

- normal exit
- handled error
- panic
- SIGINT
- SIGTERM
- broken pipe where practical

A panic hook should restore terminal state before printing diagnostics.

---

## 16. Testing Strategy

### Unit tests

Port existing conceptual tests:

- fuzzy filtering
- item ranking
- navigation steps
- selectable item detection
- search cursor movement
- category grouping
- theme parsing
- action substitution
- shell output parsing
- popup sizing
- command dispatch encoding

### Snapshot tests

Test rendered buffers without requiring a real terminal.

Cases:

- normal command palette
- filtered palette
- grouped items
- empty results
- narrow terminal
- border enabled
- long Unicode titles
- invalid command item
- theme preview

### Integration tests

Use a controlled fake tmux executable injected through `PATH`.

Verify:

- list-panes parsing
- pane switching command
- move-pane command
- popup construction
- command-file dispatch
- configuration precedence

### Manual tmux matrix

Test:

```text
macOS ARM64
macOS x86-64 where available
Linux x86-64
Linux ARM64
tmux 3.4
latest tmux
Kitty
Ghostty
iTerm2
WezTerm
Terminal.app
SSH session
nested tmux
narrow mobile terminal
```

---

## 17. Porting Sequence

### Phase 0: Fork preparation

- fork repository
- create `rust-port` branch
- preserve original TypeScript implementation temporarily
- capture screenshots and behavior notes
- record current default commands, themes, key mappings and config schemas
- add parity checklist

Deliverable:

```text
docs/compatibility.md
```

### Phase 1: Rust skeleton

- create Cargo package
- implement Clap command model
- implement config directory resolution
- implement terminal setup and cleanup
- render an empty Ratatui popup
- add `cargo build --release` instructions

Exit condition:

```bash
target/release/tmux-palette commands
```

opens and closes cleanly inside a tmux popup.

### Phase 2: Static command palette

- port item and action models
- port built-in commands
- add selection and scrolling
- add keyboard input
- add command dispatch
- add basic theme support

Exit condition:

The main command palette is usable without filtering or custom config.

### Phase 3: Search and rendering parity

- port fuzzy matcher
- add search editing
- add categories
- add footer and header
- add Unicode width handling
- add mouse support
- add narrow-terminal layout

Exit condition:

Main palette behavior visually and functionally matches the TypeScript version.

### Phase 4: Dynamic palettes

- port find-pane
- port move-pane
- port themes
- implement nested navigation stack
- implement live theme preview

Exit condition:

All built-in palettes work.

### Phase 5: User configuration

- load commands
- hidden items
- shortcuts
- aliases
- navigation settings
- sizing
- custom palettes
- custom themes

Exit condition:

Existing user configuration works without modification.

### Phase 6: Shell-generated palettes

- execute user commands
- parse plain-text and JSON output
- enforce timeout and output limit
- report failures inside palette

Exit condition:

Repository examples work against the Rust implementation.

### Phase 7: Remove Bun

- delete TypeScript source
- delete `package.json`
- delete `bun.lock`
- delete `tsconfig.json`
- remove Bun documentation
- simplify TPM installation
- update architecture documentation

Exit condition:

Repository contains no runtime or development dependency on Bun.

### Phase 8: Alpha release

Tag:

```text
v0.4.0-alpha.1
```

Installation:

```bash
git clone ...
cargo build --release
```

Alpha acceptance criteria:

- macOS and Linux build successfully
- no terminal corruption after exit
- main and built-in palettes work
- existing JSON configuration mostly works
- shell-generated items work
- documented known incompatibilities
- no Bun requirement

### Phase 9: crates.io preparation

Before publishing:

- choose an available crate name
- complete crate metadata
- include license
- include repository URL
- include README
- add keywords and categories
- ensure package excludes screenshots and unnecessary development assets
- run `cargo package`
- inspect package contents
- run `cargo publish --dry-run`

Recommended metadata:

```toml
[package]
name = "tmux-palette"
version = "0.2.0-beta.1"
edition = "2024"
license = "MIT"
repository = "https://github.com/<fork-owner>/tmux-palette"
description = "Native command palette for tmux"
keywords = ["tmux", "terminal", "tui", "productivity"]
categories = ["command-line-utilities"]
```

If `tmux-palette` is unavailable on crates.io, alternatives include:

```text
tmux-ratlette
tmux-command-palette
palette-tmux
```

The binary may still be named `tmux-palette`:

```toml
[[bin]]
name = "tmux-palette"
path = "src/main.rs"
```

### Phase 10: Beta release

Tag:

```text
v0.2.0-beta.1
```

Installation:

```bash
cargo install tmux-palette
```

Beta acceptance criteria:

- crates.io package installs on supported platforms
- TPM locates the Cargo-installed executable
- local build fallback remains supported
- upgrade instructions are documented
- configuration schema is stable enough to version
- CI runs formatting, linting, tests and packaging checks
- no required post-install scripts

---

## 18. CI Pipeline

Recommended GitHub Actions jobs:

### Check

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

### Build matrix

```text
macos-latest
ubuntu-latest
```

During alpha, native-host builds are sufficient.

### Package verification

```bash
cargo package --allow-dirty
cargo publish --dry-run
```

Run package verification on tags and pull requests that modify `Cargo.toml`, README or packaging files.

### Security and maintenance

Optionally add:

```bash
cargo audit
cargo deny check
```

Do not block the first alpha on an elaborate release pipeline.

---

## 19. Compatibility Policy

Use three levels:

### Guaranteed

- command names
- primary JSON action formats
- configuration directory
- principal key mappings
- built-in palette names

### Best effort

- identical fuzzy ranking
- exact colors across terminals
- mouse behavior across every terminal emulator
- unsupported fields from undocumented config formats

### Intentionally changed

- Bun installation
- `node_modules`
- direct execution of TypeScript source
- repository development tooling
- manual ANSI parsing where Crossterm provides a reliable event

Publish incompatibilities explicitly in:

```text
MIGRATION.md
```

---

## 20. Alpha Scope Boundaries

The alpha should not include:

- precompiled binaries
- automatic architecture detection
- automatic Rust installation
- Windows support
- plugin marketplace
- Lua configuration
- asynchronous command streaming
- background indexing
- daemon mode
- extensive animation
- a redesigned configuration format

The objective is a faithful runtime replacement, not a product redesign.

---

## 21. Beta Scope Boundaries

The beta adds:

- crates.io installation
- stable executable discovery
- improved error messages
- configuration schema documentation
- migration notes
- package verification
- stronger cross-platform testing

The beta still does not need:

- GitHub release binaries
- Homebrew formula
- Nix package
- Debian or RPM packages
- self-update functionality

Those belong after the crate interface and configuration format stabilize.

---

## 22. Principal Architectural Decisions

### Decision 1: Preserve the shell wrapper initially

Reason:

The shell wrapper already handles tmux popup creation and post-popup dispatch. Keeping it reduces the number of simultaneous changes.

Revisit after beta.

### Decision 2: Preserve JSON configuration

Reason:

Users should not need to rewrite working configuration merely because the implementation language changed.

### Decision 3: Keep built-ins compiled into the binary

Reason:

Faster startup, simpler packaging and stronger type checking.

### Decision 4: Keep dynamic shell sources

Reason:

They are central to the plugin’s extensibility and allow integration with `gh`, Docker, Git, npm and arbitrary scripts.

### Decision 5: No automatic Cargo installation from TPM

Reason:

TPM should configure the plugin, not silently compile third-party native software during tmux startup.

### Decision 6: Use one crate

Reason:

The project is small enough that separate core, UI and CLI crates would add ceremony without immediate benefit.

---

## 23. Main Risks

### Terminal behavior differences

Ratatui and Crossterm may emit different sequences from the hand-written renderer.

Mitigation:

- test inside actual tmux popups
- compare screenshots
- test resize and exit paths
- avoid assuming alternate-screen behavior

### Configuration drift

Serde structures may reject input that JavaScript previously tolerated.

Mitigation:

- use optional fields
- use defaults
- ignore unknown fields
- provide file-specific warnings

### Fuzzy matching differences

A different matcher may reorder results.

Mitigation:

- port the existing algorithm first
- consider `nucleo-matcher` only after parity

### Shell quoting

Moving dispatch between Bash and Rust can alter quoting behavior.

Mitigation:

- preserve the command-file protocol for alpha
- add tests for spaces, quotes, Unicode and shell metacharacters
- document where shell interpretation occurs

### Crate name availability

The desired crates.io name may already be taken.

Mitigation:

- verify before announcing the beta package name
- keep the installed binary named `tmux-palette`

### Build friction during alpha

Some users will not have Rust installed.

Mitigation:

- state clearly that alpha is contributor-oriented
- defer broad user adoption until crates.io or binary releases

---

## 24. Milestone Definition

### Alpha milestone

```text
Native Rust implementation
Local cargo build
TPM binding
Feature parity for normal usage
Existing JSON config
No Bun
```

### Beta milestone

```text
Published crate
cargo install workflow
Stable executable discovery
Documented schema
Reliable upgrade path
CI package verification
```

### Post-beta milestone

```text
Precompiled releases
Homebrew
Nix
Reduced shell wrapper
Possibly direct tmux popup management from Rust
```

---

## 25. Recommended First Pull Requests

### PR 1: Rust bootstrap

- add Cargo package
- add Clap commands
- add terminal setup
- show static palette
- retain TypeScript implementation

### PR 2: Main palette parity

- built-in command data
- navigation
- filtering
- rendering
- dispatch

### PR 3: Configuration

- Serde models
- commands, hidden, aliases, shortcuts
- sizing and themes

### PR 4: Dynamic palettes

- find-pane
- move-pane
- themes
- user palettes
- shell command sources

### PR 5: Rust becomes default

- update tmux wrapper
- update TPM entry point
- document `cargo build --release`
- leave Bun version behind an explicit legacy path temporarily

### PR 6: Remove TypeScript

- delete Bun and TypeScript files
- finalize migration documentation
- tag first alpha

### PR 7: crates.io beta

- package metadata
- installation discovery
- publishing CI
- tag first beta

---

## 26. Additional Design Considerations

💡 Keep the executable name independent from the crate name. This avoids compromising the command interface if `tmux-palette` is unavailable on crates.io.

💡 Add `TMUX_PALETTE_BIN` from the beginning. It makes testing, local development and alternative installation locations much easier.

💡 Port behavior before improving behavior. Rewriting and redesigning simultaneously would make regressions difficult to identify.

💡 Keep the measure protocol stable during alpha. It provides a clean seam between tmux-specific shell behavior and the Rust application.

💡 Store built-in palettes as Rust constants or constructors rather than embedded JSON. This gives compile-time validation without affecting external configuration.

💡 Add a hidden `--dump-config` or `--validate-config` command during beta. It would help diagnose malformed user files without opening a popup.

💡 Record startup time from the first alpha. A native rewrite should demonstrate the practical benefit rather than relying on assumptions.

💡 Avoid introducing async Rust initially. Input, rendering and short command execution can remain synchronous, reducing dependency and state complexity.

💡 Consider removing the shell wrapper only after crates.io beta. Direct popup control from Rust is attractive, but keeping Bash initially lowers migration risk.

💡 Keep the original repository’s MIT license and attribution unless the upstream license or fork requirements indicate otherwise.

---

## 27. Rust Port Progress

Last reviewed: 2026-08-06.

Check an item only after its automated tests pass. Check phase-level acceptance
items only after the relevant behavior has also been exercised in a real tmux
popup. This section records implementation status; the earlier sections remain
the source of truth for architecture and acceptance criteria.

### Foundation and static Commands palette

- [x] Create the single-package Rust bootstrap.
- [x] Implement CLI parsing and legacy-compatible measurement flags.
- [x] Resolve the compatible configuration directory.
- [x] Set up and restore the terminal on normal exit, setup failure, and panic.
- [x] Launch the Rust binary from the development wrapper and TPM entry point.
- [x] Preserve `TMUX_PALETTE_BIN` and `@palette-binary` overrides.
- [x] Add Rust `Action`, `Item`, and `Palette` domain models.
- [x] Port all 31 compiled-in Commands palette items.
- [x] Render grouped category and item rows with a highlighted selection.
- [x] Support Up/Down, Ctrl-P/Ctrl-N, PageUp/PageDown, Home, and End.
- [x] Keep selection visible while scrolling and resizing.
- [x] Dispatch tmux commands through the wrapper command-file protocol.
- [x] Handle empty palettes, tiny terminals, missing dispatch files, and
      unavailable actions without panicking or failing silently.
- [x] Size the static palette from its item/category count and respect
      `--category` during measurement and rendering.
- [x] Add a reusable theme model and apply the default bundled theme instead of
      fixed Ratatui styles.

Phase 2's functional exit condition is met: the main Commands palette is usable
without custom configuration. Default theme support is complete; user-selectable
theme parity remains in the later dynamic-theme phase.

### Next: search and rendering parity

- [x] Port the legacy fuzzy matcher with shared parity fixtures.
- [x] Add filter text, cursor position, and selection state to the Rust app.
- [x] Handle character insertion, Backspace, Delete, Left/Right, and word-wise
      cursor movement without corrupting UTF-8 input.
- [x] Re-rank visible items as the query changes and reset invalid selections.
- [x] Preserve initials/auto-alias and multi-word matching behavior.
- [x] Add the search row and place the real terminal cursor correctly.
- [x] Match header, footer, spacing, descriptions, shortcuts, and empty states.
- [x] Add terminal-cell-aware Unicode truncation and padding.
- [x] Add mouse click and wheel behavior, including off-screen rows.
- [x] Complete narrow-terminal and bordered-popup layout parity.
- [x] Compare pre-migration and Rust screenshots at representative popup sizes.

Search preserves the pre-migration query ordering captured by the parity fixtures,
editing remains correct for Unicode text and boundary keys, selection stays
visible after filtering/resizing, and the popup remains usable with zero results.

### Remaining port phases

- [x] Port `find-pane` and its initial current-pane selection.
- [x] Port `move-pane`.
- [x] Port bundled and user themes with live preview and atomic persistence.
- [x] Add the nested palette navigation stack and Escape/back behavior.
- [x] Load `commands.json`, `hidden.json`, `aliases.json`, and `shortcuts.json`.
- [x] Tolerate unknown optional fields and report malformed files with paths.
- [x] Load navigation, sizing, theme, and custom palette configuration.
- [x] Execute shell-generated palette sources with timeout and output limits.
- [x] Parse plain-text, tab-separated, and JSON command output.
- [x] Show plugin command failures inside the palette.
- [x] Port popup actions with global and per-item sizing overrides, border
      control, and relaunch of the originating palette.
- [x] Remove the TypeScript/Bun implementation and all active Bun documentation.
- [x] Port Shift-based search-field text selection.
- [x] Complete the alpha acceptance matrix and tag `v0.4.0-alpha.1`.
- [ ] Prepare and verify the crates.io package.
- [ ] Publish the beta and validate the `cargo install` workflow.

### Alpha acceptance matrix

- [x] `cargo fmt --check`.
- [x] `cargo clippy --all-targets -- -D warnings`.
- [x] All 132 Rust unit tests and 4 wrapper integration tests.
- [x] macOS ARM64 release build with the minimum Rust 1.85 toolchain.
- [x] Linux compatibility: the final tree passes `cargo check --all-targets
      --target x86_64-unknown-linux-gnu`, and the Ubuntu release-build workflow
      passes on the shared alpha code and dependency baseline.
- [x] Commands popup navigation and tmux dispatch smoke test on macOS with
      tmux 3.7b, including a real `Split Horizontal` dispatch.
- [x] Popup-action sizing, padding, close, and originating-palette relaunch test
      on macOS with tmux 3.7b.
- [x] Built-in palettes, compatible JSON configuration, generated sources,
      bounded subprocess failures, and removal of Bun covered by automated
      tests and release documentation.
- [x] Exact terminal-state restoration after `SIGHUP`, `SIGINT`, `SIGQUIT`, and
      `SIGTERM` in fresh tmux PTYs; forced subprocess failures are covered by
      the plugin-source tests.
- [x] Native warm measurement baseline on Apple Silicon: 100 release-binary
      `--measure` invocations in 0.20 seconds, about 2 ms per invocation.

The alpha gates above work on Unix hosts with Rust 1.85+ and tmux 3.4+ APIs.
`SIGKILL` cannot be intercepted for in-process cleanup. The shell wrapper still
requires a source checkout and a locally built binary during alpha.

### Expanded compatibility coverage (non-blocking after alpha)

- [ ] macOS test on the minimum supported tmux 3.4 release.
- [ ] Linux real-tmux smoke test.
- [ ] Kitty, Ghostty, iTerm2, WezTerm, and Terminal.app checks.
- [ ] SSH, nested tmux, and narrow mobile terminal checks.
