#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-biglone/codex-history-viewer}"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="${BINARY_NAME:-codex-history-cli}"

usage() {
  cat <<'EOF'
Install codex-history-cli from GitHub Releases.

Usage:
  install-cli.sh [--version TAG|latest] [--install-dir DIR] [--repo OWNER/REPO]

Environment:
  VERSION       Release tag to install, defaults to latest
  INSTALL_DIR   Install directory, defaults to ~/.local/bin
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases

Examples:
  curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-cli.sh | bash
  VERSION=v1.3.7 bash scripts/install-cli.sh
  INSTALL_DIR=/usr/local/bin bash scripts/install-cli.sh
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      if [[ $# -lt 2 ]]; then
        echo "error: --version requires a value" >&2
        exit 2
      fi
      VERSION="${2:-}"
      shift 2
      ;;
    --install-dir)
      if [[ $# -lt 2 ]]; then
        echo "error: --install-dir requires a value" >&2
        exit 2
      fi
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --repo)
      if [[ $# -lt 2 ]]; then
        echo "error: --repo requires a value" >&2
        exit 2
      fi
      REPO="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$VERSION" || -z "$INSTALL_DIR" || -z "$REPO" ]]; then
  echo "error: VERSION, INSTALL_DIR, and REPO must be non-empty" >&2
  exit 2
fi

detect_asset() {
  local os arch platform
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) platform="Linux-x64" ;;
        aarch64|arm64) platform="Linux-arm64" ;;
        *) echo "error: unsupported Linux architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        arm64|aarch64) platform="macOS-arm64" ;;
        x86_64|amd64) platform="macOS-x64" ;;
        *) echo "error: unsupported macOS architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    *)
      echo "error: unsupported OS for this script: $os. Use scripts/install-cli.ps1 on Windows." >&2
      exit 1
      ;;
  esac

  printf 'codex-history-cli-%s' "$platform"
}

download_with_curl_or_wget() {
  local url="$1" dest="$2"
  if command -v curl >/dev/null 2>&1; then
    if [[ -n "${GH_TOKEN:-}" ]]; then
      curl -fL \
        -H "Authorization: Bearer ${GH_TOKEN}" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$url" -o "$dest"
    else
      curl -fL "$url" -o "$dest"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [[ -n "${GH_TOKEN:-}" ]]; then
      wget --header="Authorization: Bearer ${GH_TOKEN}" \
        --header="X-GitHub-Api-Version: 2022-11-28" \
        -O "$dest" "$url"
    else
      wget -O "$dest" "$url"
    fi
  else
    return 127
  fi
}

download_with_gh() {
  local asset="$1" dest="$2"
  command -v gh >/dev/null 2>&1 || return 127

  if [[ "$VERSION" == "latest" ]]; then
    gh release download --repo "$REPO" --pattern "$asset" --output "$dest" --clobber
  else
    gh release download "$VERSION" --repo "$REPO" --pattern "$asset" --output "$dest" --clobber
  fi
}

asset="$(detect_asset)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
tmp_bin="$tmp_dir/$asset"

if [[ "$VERSION" == "latest" ]]; then
  url="https://github.com/${REPO}/releases/latest/download/${asset}"
else
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
fi

echo "Installing $asset from ${REPO}@${VERSION}"
if ! download_with_curl_or_wget "$url" "$tmp_bin"; then
  echo "Direct download failed; trying GitHub CLI..." >&2
  if ! download_with_gh "$asset" "$tmp_bin"; then
    cat >&2 <<EOF
error: failed to download $asset

For draft or private releases, install GitHub CLI and authenticate first:
  gh auth login
  VERSION=$VERSION bash scripts/install-cli.sh
EOF
    exit 1
  fi
fi

chmod +x "$tmp_bin"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp_bin" "$INSTALL_DIR/$BINARY_NAME"

echo "Installed: $INSTALL_DIR/$BINARY_NAME"
if command -v "$BINARY_NAME" >/dev/null 2>&1; then
  "$BINARY_NAME" --help
else
  echo "Tip: add $INSTALL_DIR to PATH, or run:"
  echo "  $INSTALL_DIR/$BINARY_NAME --help"
fi
