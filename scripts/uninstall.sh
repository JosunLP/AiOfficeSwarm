#!/usr/bin/env sh
set -eu

BINARY_NAME="swarm"
INSTALL_DIR="${SWARM_INSTALL_DIR:-${HOME}/.local/bin}"
BINARY_PATH="$INSTALL_DIR/$BINARY_NAME"

say() {
  printf '%s\n' "$*"
}

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
