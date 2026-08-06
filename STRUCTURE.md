# Repository Structure

`tmux-ratlette` is one Rust package with a library target, a binary target, and
a thin tmux integration layer. The repository does not need a Cargo workspace:
the application and adapter are released together and share one version.

## Current layout

```text
tmux-ratlette/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs              executable entry point
│   ├── lib.rs               library module boundary
│   ├── cli.rs               arguments and measurement mode
│   ├── app.rs               input, selection, navigation, and rendering
│   ├── config.rs            config paths and popup sizing
│   ├── dispatch.rs          command-file protocol
│   ├── fuzzy.rs             search ranking
│   ├── plugin_output.rs     generated-item parsing
│   ├── plugin_source.rs     bounded shell command execution
│   ├── terminal.rs          terminal setup and restoration
│   ├── themes.rs            bundled and user themes
│   ├── tmux.rs              tmux query helpers
│   ├── user_config.rs       JSON configuration loading
│   ├── model/               actions, items, palettes, and themes
│   └── palettes/            built-in palette constructors
├── tests/
│   ├── wrapper.rs           shell-wrapper integration tests
│   └── fixtures/            shared behavior fixtures
├── examples/                drop-in custom palette configurations
├── bin/tmux-palette.sh      popup and deferred-dispatch wrapper
├── tmux-palette.tmux        TPM entry point and key bindings
└── .github/workflows/rust.yml
```

## Runtime flow

1. tmux invokes `bin/tmux-palette.sh` through a key binding.
2. The wrapper resolves the native executable.
3. It asks the executable for popup dimensions using `--measure`.
4. It opens `tmux display-popup` with the chosen palette and dimensions.
5. The executable owns configuration, search, rendering, navigation, and item
   selection.
6. For tmux and shell actions, the executable writes an encoded action to a
   temporary command file and exits.
7. The wrapper dispatches that action after the popup has closed, allowing
   interactive tmux prompts to receive input.

The wrapper resolves the executable in this order:

1. `TMUX_PALETTE_BIN`
2. tmux option `@palette-binary`
3. checkout-local `target/debug/tmux-ratlette`
4. checkout-local `target/release/tmux-ratlette`
5. checkout-local `bin/tmux-ratlette`
6. `tmux-ratlette` from `PATH`

An explicit override must point to an executable. If no candidate resolves, the
wrapper prints an actionable build error and does not create the popup.

## Responsibility boundaries

The Rust executable is responsible for all palette behavior: configuration,
sizing, search, themes, rendering, generated sources, and action selection. The
shell wrapper is limited to tmux popup integration and deferred dispatch. The
TPM entry point only validates binary availability and creates configured key
bindings.

User customization stays outside the checkout under
`~/.config/tmux-palette/`. JSON actions intentionally expose only tmux, shell,
popup, and nested-palette operations; internal Rust actions such as live theme
application are not part of the user schema.

Generated palette commands run through `/bin/sh -c`. Their execution time and
combined output are bounded, descendant processes are cleaned up, malformed
output becomes a disabled error row, and static items remain available.

## Build and verification

The minimum supported compiler is Rust 1.85. A local verification run is:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

CI runs the same checks on Linux. Real tmux popup checks remain necessary for
terminal behavior that unit and wrapper tests cannot prove.

## Distribution boundary

The repository currently supports checkout builds and TPM integration. Future
crates.io, GitHub Release, and Homebrew channels should distribute the same
`tmux-ratlette` executable; they should not fork application behavior into a
second package or repository. A separate Homebrew tap may contain formula
metadata only.
