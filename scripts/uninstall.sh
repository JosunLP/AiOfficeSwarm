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

if [ -d "$INSTALL_DIR" ] && [ -z "$(ls -A "$INSTALL_DIR")" ]; then
  rmdir "$INSTALL_DIR"
  say "Removed empty directory $INSTALL_DIR"
fi

say "If '$INSTALL_DIR' is referenced in your PATH manually, you can remove that entry now."
