#!/usr/bin/env bash
# TPM entry point for tmux-palette.
# Sourced by tmux when the plugin is installed via tmux-plugins/tpm.

set -eu

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Use the wrapper's resolver so TPM and direct development bindings agree
# on @palette-binary, TMUX_PALETTE_BIN, checkout builds, and PATH installs.
if ! "$CURRENT_DIR/bin/tmux-palette.sh" --check >/dev/null; then
  exit 0
fi

get_opt() {
  local val
  val="$(tmux show-option -gqv "$1" 2>/dev/null || true)"
  echo "${val:-$2}"
}

PALETTE_KEY="$(get_opt @palette-key 'C-Space')"
FIND_PANE_KEY="$(get_opt @palette-find-pane-key '')"
MOVE_PANE_KEY="$(get_opt @palette-move-pane-key '')"

if [ "$PALETTE_KEY" != "off" ] && [ -n "$PALETTE_KEY" ]; then
  tmux bind-key -n "$PALETTE_KEY" run-shell "$CURRENT_DIR/bin/tmux-palette.sh"
fi

if [ -n "$FIND_PANE_KEY" ]; then
  tmux bind-key -n "$FIND_PANE_KEY" run-shell "$CURRENT_DIR/bin/tmux-palette.sh find-pane"
fi

if [ -n "$MOVE_PANE_KEY" ]; then
  tmux bind-key -n "$MOVE_PANE_KEY" run-shell "$CURRENT_DIR/bin/tmux-palette.sh move-pane"
fi
