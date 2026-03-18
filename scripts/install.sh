#!/usr/bin/env sh
set -eu

REPO_OWNER="JosunLP"
REPO_NAME="AiOfficeSwarm"
BINARY_NAME="swarm"
INSTALL_DIR="${SWARM_INSTALL_DIR:-${HOME}/.local/bin}"
REQUESTED_VERSION="${1:-latest}"

say() {
  printf '%s\n' "$*"
}

fail() {
  say "Error: $*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

normalize_arch() {
  case "$1" in
    x86_64|amd64) printf 'x86_64' ;;
    aarch64|arm64) printf 'aarch64' ;;
    *) fail "Unsupported architecture: $1" ;;
  esac
}

normalize_os() {
  case "$1" in
    Linux) printf 'unknown-linux-gnu' ;;
    Darwin) printf 'apple-darwin' ;;
    *) fail "Unsupported operating system: $1" ;;
  esac
}

validate_target() {
  case "$1" in
    x86_64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;;
    aarch64-unknown-linux-gnu) fail 'Linux ARM64 binaries are not published yet' ;;
    *) fail "Unsupported release target: $1" ;;
  esac
}

build_download_url() {
  asset_name="$1"
  if [ "$REQUESTED_VERSION" = "latest" ]; then
    printf 'https://github.com/%s/%s/releases/latest/download/%s' "$REPO_OWNER" "$REPO_NAME" "$asset_name"
  else
    case "$REQUESTED_VERSION" in
      v*) tag="$REQUESTED_VERSION" ;;
      *) tag="v$REQUESTED_VERSION" ;;
    esac
    printf 'https://github.com/%s/%s/releases/download/%s/%s' "$REPO_OWNER" "$REPO_NAME" "$tag" "$asset_name"
  fi
}

need_cmd uname
need_cmd mktemp
need_cmd tar
need_cmd install

if command -v curl >/dev/null 2>&1; then
  DOWNLOAD_TOOL='curl'
elif command -v wget >/dev/null 2>&1; then
  DOWNLOAD_TOOL='wget'
else
  fail 'curl or wget is required'
fi

ARCH="$(normalize_arch "$(uname -m)")"
PLATFORM="$(normalize_os "$(uname -s)")"
TARGET="${ARCH}-${PLATFORM}"
validate_target "$TARGET"
ASSET_NAME="${BINARY_NAME}-${TARGET}.tar.gz"
DOWNLOAD_URL="$(build_download_url "$ASSET_NAME")"
TMP_DIR="$(mktemp -d)"
ARCHIVE_PATH="$TMP_DIR/$ASSET_NAME"
BINARY_PATH="$INSTALL_DIR/$BINARY_NAME"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

mkdir -p "$INSTALL_DIR"

say "Downloading $ASSET_NAME ..."
case "$DOWNLOAD_TOOL" in
  curl)
    curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE_PATH"
    ;;
  wget)
    wget -q -O "$ARCHIVE_PATH" "$DOWNLOAD_URL"
    ;;
  *)
    fail "Unknown download tool"
    ;;
esac

say "Installing to $INSTALL_DIR ..."
tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"
install -m 755 "$TMP_DIR/$BINARY_NAME" "$BINARY_PATH"

say "Installed $("$BINARY_PATH" --version)"
say "If '$INSTALL_DIR' is not on your PATH yet, add it in your shell profile."
