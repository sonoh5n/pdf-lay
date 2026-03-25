#!/bin/sh

set -eu

OWNER_REPO="sonoh5n/pdf-lay"
BIN_NAME="pdf-lay"

usage() {
  cat <<'EOF'
Install pdf-lay from GitHub Releases.

Usage:
  install.sh [--version <tag>] [--dir <path>]

Options:
  --version <tag>  Install a specific tag like v0.1.0-rc.1.
                   Defaults to the latest release.
  --dir <path>     Destination directory for the binary.
                   Defaults to $PDF_LAY_INSTALL_DIR or $HOME/.local/bin.
  -h, --help       Show this help.
EOF
}

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *)
      fail "unsupported operating system: $os"
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *)
      fail "unsupported architecture: $arch"
      ;;
  esac

  printf '%s-%s\n' "$arch_part" "$os_part"
}

VERSION=""
INSTALL_DIR="${PDF_LAY_INSTALL_DIR:-$HOME/.local/bin}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || fail "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --dir)
      [ "$#" -ge 2 ] || fail "--dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

need_cmd curl
need_cmd tar
need_cmd mktemp
need_cmd install

verify_checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$1" && sha256sum -c "$2")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$1" && shasum -a 256 -c "$2")
  else
    log "Warning: no sha256sum or shasum found, skipping checksum verification"
    return 0
  fi
}

TARGET=$(detect_target)
ARCHIVE="${BIN_NAME}-${TARGET}.tar.gz"

if [ -n "$VERSION" ]; then
  DOWNLOAD_URL="https://github.com/${OWNER_REPO}/releases/download/${VERSION}/${ARCHIVE}"
else
  DOWNLOAD_URL="https://github.com/${OWNER_REPO}/releases/latest/download/${ARCHIVE}"
fi

TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t pdf-lay)
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

mkdir -p "$INSTALL_DIR"

CHECKSUM_FILE="${ARCHIVE}.sha256"
if [ -n "$VERSION" ]; then
  CHECKSUM_URL="https://github.com/${OWNER_REPO}/releases/download/${VERSION}/${CHECKSUM_FILE}"
else
  CHECKSUM_URL="https://github.com/${OWNER_REPO}/releases/latest/download/${CHECKSUM_FILE}"
fi

log "Downloading ${DOWNLOAD_URL}"
curl -fL "$DOWNLOAD_URL" -o "$TMP_DIR/$ARCHIVE"

if curl -fsSL "$CHECKSUM_URL" -o "$TMP_DIR/$CHECKSUM_FILE" 2>/dev/null; then
  log "Verifying checksum"
  verify_checksum "$TMP_DIR" "$CHECKSUM_FILE" || fail "checksum verification failed"
else
  log "Warning: checksum file not found, skipping verification"
fi

log "Extracting ${ARCHIVE}"
tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR"

[ -f "$TMP_DIR/$BIN_NAME" ] || fail "archive did not contain ${BIN_NAME}"

log "Installing ${BIN_NAME} to ${INSTALL_DIR}"
install -m 0755 "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

log "Installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    log "Add ${INSTALL_DIR} to PATH if needed."
    ;;
esac
