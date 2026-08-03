**Do not split it into two independent product repositories yet.** Use:

1. **One main repository** containing the Rust application and the TPM adapter.
2. **One optional Homebrew tap repository** containing only the formula.
3. Publish the executable from the main repository to crates.io and GitHub Releases.

The important distinction is:

- `tmux-ratlette` is a **binary application**, not primarily a Rust library.
- The TPM plugin is merely an **integration wrapper** that creates tmux bindings and launches that executable.
- Homebrew and `cargo install` are alternative distribution channels for the same executable.

## Recommended structure

```text
tmux-ratlette/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── app.rs
│   ├── config.rs
│   ├── palette.rs
│   ├── theme.rs
│   └── tmux.rs
├── tmux-ratlette.tmux
├── scripts/
│   ├── install-binary.sh
│   └── detect-platform.sh
├── assets/
│   ├── themes/
│   └── examples/
└── .github/workflows/release.yml
```

A Cargo package may contain both a library crate and a binary crate, with the binary depending on the library. You therefore do not need separate repositories—or even a workspace—just to separate application logic from the executable entry point.

Conceptually:

```text
src/lib.rs
    reusable application logic
        ↑
src/main.rs
    clap + startup + error reporting
```

The published package could still be:

```bash
cargo install tmux-ratlette
```

`cargo install` is specifically intended to install binary crates locally.

## Installation channels

### Cargo

```bash
cargo install tmux-ratlette
```

This compiles and installs the `tmux-ratlette` executable.

Crates.io should publish the application package named `tmux-ratlette`; it does **not** need to be presented as a reusable library.

### Homebrew

```bash
brew install num13ru/tap/tmux-ratlette
```

Initially, the usual arrangement would be a separate tap:

```text
num13ru/homebrew-tap
└── Formula/
    └── tmux-ratlette.rb
```

The formula could:

- build the Rust project from a release source archive; or
- install architecture-specific GitHub Release binaries.

Homebrew formulae describe how software is installed, while bottles are Homebrew’s prebuilt package format.

The tap repository is worth separating because it is Homebrew packaging metadata, not application source.

### GitHub Releases

Release these assets:

```text
tmux-ratlette-aarch64-apple-darwin.tar.gz
tmux-ratlette-x86_64-apple-darwin.tar.gz
tmux-ratlette-aarch64-unknown-linux-gnu.tar.gz
tmux-ratlette-x86_64-unknown-linux-gnu.tar.gz
```

This corresponds to the four platform combinations already proposed in issue #11.

### TPM

Users would configure:

```tmux
set -g @plugin 'num13ru/tmux-ratlette'
```

TPM clones plugin repositories and sources them from its plugin directory.

The repository’s `tmux-ratlette.tmux` should be a very thin adapter:

```bash
#!/usr/bin/env bash

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${TMUX_PALETTE_BIN:-}"

if [ -z "$BINARY" ]; then
    BINARY="$(command -v tmux-ratlette 2>/dev/null || true)"
fi

if [ -z "$BINARY" ] && [ -x "$CURRENT_DIR/bin/tmux-ratlette" ]; then
    BINARY="$CURRENT_DIR/bin/tmux-ratlette"
fi

if [ -z "$BINARY" ]; then
    tmux display-message \
        "tmux-ratlette binary not found; install with brew, cargo, or the release binary"
    exit 0
fi

tmux bind-key -n C-Space run-shell \
    "$CURRENT_DIR/scripts/open-popup.sh '$BINARY'"
```

The actual wrapper would need careful shell quoting and platform handling, but the boundary should be exactly this thin.

## Should TPM automatically download the binary?

There are two reasonable policies.

### Policy A: TPM integration only

TPM installs the tmux wrapper, while the user separately installs the executable:

```bash
brew install num13ru/tap/tmux-ratlette
```

or:

```bash
cargo install tmux-ratlette
```

Advantages:

- no hidden network request during tmux startup
- one clear executable in `PATH`
- Homebrew or Cargo owns upgrades
- no duplicate installation
- TPM code remains trivial

Disadvantage:

- installation has two steps

This is architecturally cleaner.

### Policy B: TPM includes a binary bootstrapper

When TPM clones the repository, the plugin downloads the matching GitHub Release binary into:

```text
~/.tmux/plugins/tmux-ratlette/bin/tmux-palette
```

Advantages:

- familiar one-step TPM installation
- no Rust or Homebrew requirement
- closer to normal tmux-plugin expectations

Disadvantages:

- you must implement platform detection
- download checksums should be verified
- TPM updates and binary versions can become mismatched
- users may unknowingly have two copies installed
- downloading during every tmux startup would be unacceptable

This should happen only through an explicit TPM install/update hook, or only once when the binary is absent—not on every tmux configuration reload.

## My preferred model

Support both, with this resolution order:

```text
1. @tmux_palette_binary option
2. TMUX_PALETTE_BIN environment variable
3. tmux-ratlette found in PATH
4. TPM-local bin/tmux-ratlette
5. clear installation error
```

For example:

```tmux
set -g @tmux_palette_binary '/opt/homebrew/bin/tmux-ratlette'
set -g @plugin 'num13ru/tmux-ratlette'
```

Most users would not need the explicit setting because `command -v tmux-ratlette` would find the Homebrew or Cargo installation.

## Do you need a Cargo workspace?

Initially, no.

Use one package:

```toml
[package]
name = "tmux-ratlette"
version = "0.1.0"

[lib]
name = "tmux_palette"
path = "src/lib.rs"

[[bin]]
name = "tmux-ratlette"
path = "src/main.rs"
```

That gives you an internal library and an executable without introducing multiple packages.

Move to a workspace only when there is a real independent boundary:

```text
Cargo.toml
crates/
├── tmux-ratlette-core/
├── tmux-ratlette-cli/
└── tmux-ratlette-config/
```

Cargo workspaces are intended for related packages developed together, sharing dependency resolution and build output.

A workspace becomes justified when:

- another executable consumes the core
- the core gets a stable public API
- configuration parsing is independently reusable
- you need feature isolation
- different packages need separate release versions

Otherwise it introduces manifests and dependency boundaries without solving a current problem.

## What I would not do

I would not create:

```text
num13ru/tmux-ratlette-plugin
num13ru/tmux-ratlette-rs
```

That creates unnecessary coordination:

- matching tags across repositories
- duplicated README and issue tracking
- uncertainty about which repository is authoritative
- TPM wrapper releases becoming disconnected from binary releases
- two contribution paths for one small application

The TPM adapter belongs next to the executable because changes to CLI flags, popup measurement output, environment variables, configuration paths, and binary naming may require simultaneous wrapper changes.

## Suggested repository boundaries

```text
num13ru/tmux-ratlette
    Rust source
    TPM entrypoint
    install scripts
    GitHub releases
    crates.io publication
    documentation

num13ru/homebrew-tap
    Formula/tmux-ratlette.rb
```

Later, only if the TPM adapter becomes an independently useful generic plugin:

```text
num13ru/tmux-ratlette-tpm
```

But that is unlikely. It would probably remain fewer than a few hundred lines of shell and tmux configuration.

💡 **Call it a binary crate, not a library to download.** `tmux-ratlette-core` may be a library internally, but the user installs the `tmux-palette` executable.

💡 **Keep the executable responsible for palette behavior.** The shell wrapper should not duplicate sizing, configuration parsing, themes, or platform behavior.

💡 **Do not make Homebrew mandatory.** Linux users should be able to use GitHub Releases or `cargo install`.

💡 **Do not make Cargo mandatory.** Most tmux users should download a precompiled binary rather than compile the entire Rust dependency graph.

💡 **Use one version number across the executable and TPM wrapper.** A single repository and tag naturally preserve compatibility.

💡 **Add `tmux-ratlette doctor`.** It could report the executable version, tmux version, configuration directory, terminal capabilities, resolved theme, and plugin path.

💡 **Let the executable print its tmux bindings.** Something like `tmux-ratlette init tmux` could eventually generate integration commands, further reducing shell logic.

My recommendation is therefore:

> **One main repository, one Rust package containing a library target and binary target, one thin TPM adapter in the same repository, GitHub Release binaries, crates.io distribution, and a separate Homebrew tap only for formula metadata.**
