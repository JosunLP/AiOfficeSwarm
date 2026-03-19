#!/usr/bin/env sh
set -eu

BINARY_NAME="swarm"

say() {
  printf '%s\n' "$*"
}

if [ -n "${SWARM_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$SWARM_INSTALL_DIR"
elif [ -n "${HOME:-}" ]; then
  INSTALL_DIR="${HOME}/.local/bin"
else
  say 'Error: HOME is not set; set SWARM_INSTALL_DIR to choose an install directory' >&2
  exit 1
fi

BINARY_PATH="$INSTALL_DIR/$BINARY_NAME"

if [ -f "$BINARY_PATH" ]; then
  rm -f "$BINARY_PATH"
  say "Removed $BINARY_PATH"
else
  say "Nothing to remove at $BINARY_PATH"
fi

if [ -d "$INSTALL_DIR" ]; then
  if rmdir "$INSTALL_DIR" 2>/dev/null; then
    say "Removed empty directory $INSTALL_DIR"
  fi
fi

say "If '$INSTALL_DIR' is referenced in your PATH manually, you can remove that entry now."
