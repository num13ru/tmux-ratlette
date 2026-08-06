#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
WRAPPER="$DIR/bin/tmux-palette.sh"
TMUX_BIN="${TMUX_BIN:-$(command -v tmux 2>/dev/null || true)}"

tmux_message() {
  local message="$1"
  if [ -n "$TMUX_BIN" ]; then
    "$TMUX_BIN" display-message "$message" 2>/dev/null || printf '%s\n' "$message" >&2
  else
    printf '%s\n' "$message" >&2
  fi
}

resolve_candidate() {
  local candidate="$1"
  case "$candidate" in
    */*) [ -x "$candidate" ] && printf '%s\n' "$candidate" ;;
    *) command -v "$candidate" 2>/dev/null || true ;;
  esac
}

resolve_binary() {
  local configured="${TMUX_PALETTE_BIN:-}"
  local source="TMUX_PALETTE_BIN"

  if [ -z "$configured" ] && [ -n "$TMUX_BIN" ]; then
    configured="$("$TMUX_BIN" show-option -gqv @palette-binary 2>/dev/null || true)"
    source="@palette-binary"
  fi

  if [ -n "$configured" ]; then
    local resolved
    resolved="$(resolve_candidate "$configured" || true)"
    if [ -z "$resolved" ]; then
      tmux_message "tmux-ratlette: $source does not point to an executable: $configured"
      return 1
    fi
    printf '%s\n' "$resolved"
    return 0
  fi

  local candidate
  for candidate in \
    "$DIR/target/debug/tmux-ratlette" \
    "$DIR/target/release/tmux-ratlette" \
    "$DIR/bin/tmux-ratlette"; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  command -v tmux-ratlette 2>/dev/null || {
    tmux_message "tmux-ratlette: Rust binary not found. Run: cd $DIR && cargo build"
    return 1
  }
}

BINARY="$(resolve_binary)" || exit 1

if [ "${1:-}" = "--check" ]; then
  printf '%s\n' "$BINARY"
  exit 0
fi

if [ -z "$TMUX_BIN" ]; then
  printf '%s\n' "tmux-ratlette: tmux not found in PATH" >&2
  exit 1
fi

CMD_FILE="$(mktemp)"
trap 'rm -f "$CMD_FILE"' EXIT

PALETTE="${1:-commands}"
shift || true
# Remaining args (e.g. --category=Tools) get forwarded to both the
# measure pass and the popup invocation so filters affect sizing too.

CH="$("$TMUX_BIN" display-message -p '#{client_height}' 2>/dev/null || echo 24)"
CW="$("$TMUX_BIN" display-message -p '#{client_width}' 2>/dev/null || echo 80)"
[[ "$CH" =~ ^[1-9][0-9]*$ ]] || CH=24
[[ "$CW" =~ ^[1-9][0-9]*$ ]] || CW=80

case "$PALETTE" in
  commands | find-pane | move-pane | themes) PALETTE_ARGS=("$PALETTE") ;;
  *) PALETTE_ARGS=(palette "$PALETTE") ;;
esac
RUST_ARGS=("${PALETTE_ARGS[@]}")
if [ "$#" -gt 0 ]; then
  RUST_ARGS+=("$@")
fi

# Ask the palette how big it wants to be. The Rust executable emits tab-separated
# rows<TAB>width<TAB>padX<TAB>border<TAB>bodyStyle<TAB>borderStyle,
# with bootstrap defaults applied. Passing client dimensions enables its
# narrow-terminal mode. Values from sizing.json are included in this response.
MEASURE="$("$BINARY" "${RUST_ARGS[@]}" --measure "--cw=$CW" "--ch=$CH" 2>/dev/null || echo "20	90	3	none	default	default")"
IFS=$'\t' read -r WANT_H WANT_W WANT_PADX WANT_BORDER WANT_BODY_STYLE WANT_BORDER_STYLE <<< "$MEASURE"
[[ "$WANT_H" =~ ^[1-9][0-9]*$ ]] || WANT_H=20
[[ "$WANT_W" =~ ^[1-9][0-9]*$ ]] || WANT_W=90
[[ "$WANT_PADX" =~ ^[0-9]+$ ]] || WANT_PADX=3
WANT_BORDER="${WANT_BORDER:-none}"
WANT_BODY_STYLE="${WANT_BODY_STYLE:-default}"
WANT_BORDER_STYLE="${WANT_BORDER_STYLE:-default}"

# Cap by client size, leaving breathing room (mobile mode already
# uses full dims, so the cap is a no-op there).
MAX_H=$(( CH - 2 ))
(( MAX_H > 0 )) || MAX_H=1
MAX_W=$(( CW - 4 ))
(( MAX_W > 0 )) || MAX_W=1
H=$(( WANT_H > MAX_H ? MAX_H : WANT_H ))
W=$(( WANT_W > MAX_W ? MAX_W : WANT_W ))

# Mobile mode wants edge-to-edge: undo the breathing cap.
if [ "$WANT_W" -ge "$CW" ]; then H="$CH"; W="$CW"; fi

# Allow env override.
H="${TMUX_PALETTE_HEIGHT:-$H}"
W="${TMUX_PALETTE_WIDTH:-$W}"

# Border: "none" maps to tmux's -B (no border). Anything else is passed
# through tmux's -b <type>: single, double, heavy, rounded, padded, simple.
# -s sets the popup body style, -S the border style — both match the
# palette theme by default so the chrome doesn't read as stock tmux.
BORDER_ARGS=(-B -s "$WANT_BODY_STYLE")
if [ "$WANT_BORDER" != "none" ]; then
  BORDER_ARGS=(-b "$WANT_BORDER" -s "$WANT_BODY_STYLE" -S "$WANT_BORDER_STYLE")
fi

# Build the final argv with shell-safe quoting for tmux's popup shell.
ARG_STR=""
for a in "${RUST_ARGS[@]}"; do
  ARG_STR+=" $(printf %q "$a")"
done

# BORDERED=1 tells the palette to skip its own top/bottom pad rows
# (the tmux border replaces them visually, otherwise it looks double-padded).
BORDERED=0
[ "$WANT_BORDER" != "none" ] && BORDERED=1

printf -v CMD_FILE_Q '%q' "$CMD_FILE"
printf -v BINARY_Q '%q' "$BINARY"
printf -v WRAPPER_Q '%q' "$WRAPPER"
printf -v TMUX_BIN_Q '%q' "$TMUX_BIN"
printf -v PAD_X_Q '%q' "$WANT_PADX"
printf -v BORDERED_Q '%q' "$BORDERED"

"$TMUX_BIN" display-popup "${BORDER_ARGS[@]}" -w "$W" -h "$H" -E \
  "TMUX_PALETTE_CMD=$CMD_FILE_Q TMUX_PALETTE_BIN=$BINARY_Q TMUX_PALETTE_WRAPPER=$WRAPPER_Q TMUX_PALETTE_TMUX_BIN=$TMUX_BIN_Q TMUX_PALETTE_PADX=$PAD_X_Q TMUX_PALETTE_BORDERED=$BORDERED_Q exec $BINARY_Q$ARG_STR"

if [ -s "$CMD_FILE" ]; then
  CMD="$(cat "$CMD_FILE")"
  case "$CMD" in
    tmux:*)
      # Don't propagate the dispatched command's exit status. tmux's `run-shell`
      # surfaces "script returned N" on non-zero exit, and some commands return
      # non-zero even when the prompt itself worked.
      eval "\"$TMUX_BIN\" ${CMD#tmux:}" || true
      ;;
    shell:*)
      eval "${CMD#shell:}" || true
      ;;
  esac
fi
exit 0
