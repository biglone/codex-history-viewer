#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-biglone/codex-history-viewer}"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-}"
tmp_dir=""
mount_dir=""

cleanup() {
  if [[ -n "${mount_dir:-}" ]]; then
    hdiutil detach "$mount_dir" -quiet >/dev/null 2>&1 || true
    rm -rf "$mount_dir"
  fi
  if [[ -n "${tmp_dir:-}" ]]; then
    rm -rf "$tmp_dir"
  fi
}

trap cleanup EXIT

usage() {
  cat <<'EOF'
Install Codex History Viewer desktop app from GitHub Releases.

Usage:
  install-gui.sh [--version TAG|latest] [--install-dir DIR] [--repo OWNER/REPO]

Environment:
  VERSION       Release tag to install, defaults to latest
  INSTALL_DIR   Linux AppImage directory or macOS app directory
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases

Examples:
  curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.sh | bash
  curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.sh | VERSION=v1.3.8 bash
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      if [[ $# -lt 2 ]]; then echo "error: --version requires a value" >&2; exit 2; fi
      VERSION="${2:-}"
      shift 2
      ;;
    --install-dir)
      if [[ $# -lt 2 ]]; then echo "error: --install-dir requires a value" >&2; exit 2; fi
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --repo)
      if [[ $# -lt 2 ]]; then echo "error: --repo requires a value" >&2; exit 2; fi
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

download_direct() {
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

download_with_gh_pattern() {
  local pattern="$1" dest="$2" tmp_dir="$3"
  command -v gh >/dev/null 2>&1 || return 127

  if [[ "$VERSION" == "latest" ]]; then
    gh release download --repo "$REPO" --pattern "$pattern" --dir "$tmp_dir" --clobber
  else
    gh release download "$VERSION" --repo "$REPO" --pattern "$pattern" --dir "$tmp_dir" --clobber
  fi

  local downloaded
  downloaded="$(find "$tmp_dir" -maxdepth 1 -type f -name "$pattern" -print -quit)"
  [[ -n "$downloaded" ]] || return 1
  mv "$downloaded" "$dest"
}

download_asset() {
  local stable_asset="$1" fallback_pattern="$2" dest="$3" tmp_dir="$4" url
  if [[ "$VERSION" == "latest" ]]; then
    url="https://github.com/${REPO}/releases/latest/download/${stable_asset}"
  else
    url="https://github.com/${REPO}/releases/download/${VERSION}/${stable_asset}"
  fi

  if ! download_direct "$url" "$dest"; then
    echo "Direct download failed; trying GitHub CLI with pattern ${fallback_pattern}..." >&2
    if ! download_with_gh_pattern "$fallback_pattern" "$dest" "$tmp_dir"; then
      cat >&2 <<EOF
error: failed to download GUI asset

For draft or private releases, install GitHub CLI and authenticate first:
  gh auth login
  VERSION=$VERSION bash scripts/install-gui.sh
EOF
      exit 1
    fi
  fi
}

install_linux_appimage() {
  local arch="$1" tmp_dir="$2" appimage="$tmp_dir/codex-history-viewer.AppImage"
  if [[ "$arch" != "x86_64" && "$arch" != "amd64" ]]; then
    echo "error: Linux GUI package is currently published for x64 only. Use codex-history-cli on Linux ARM64." >&2
    exit 1
  fi

  local dir="${INSTALL_DIR:-$HOME/.local/bin}"
  download_asset \
    "codex-history-viewer-Linux-x64.AppImage" \
    "Codex.History.Viewer_*_amd64.AppImage" \
    "$appimage" \
    "$tmp_dir"

  mkdir -p "$dir"
  install -m 0755 "$appimage" "$dir/codex-history-viewer"

  local desktop_dir="$HOME/.local/share/applications"
  mkdir -p "$desktop_dir"
  cat > "$desktop_dir/codex-history-viewer.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Codex History Viewer
Exec=$dir/codex-history-viewer
Terminal=false
Categories=Development;Utility;
EOF

  echo "Installed: $dir/codex-history-viewer"
  echo "Desktop entry: $desktop_dir/codex-history-viewer.desktop"
}

install_macos_dmg() {
  local arch="$1" tmp_dir="$2" stable pattern dmg app target_dir
  case "$arch" in
    arm64|aarch64)
      stable="codex-history-viewer-macOS-arm64.dmg"
      pattern="Codex.History.Viewer_*_aarch64.dmg"
      ;;
    x86_64|amd64)
      stable="codex-history-viewer-macOS-x64.dmg"
      pattern="Codex.History.Viewer_*_x64.dmg"
      ;;
    *)
      echo "error: unsupported macOS architecture: $arch" >&2
      exit 1
      ;;
  esac

  dmg="$tmp_dir/codex-history-viewer.dmg"
  download_asset "$stable" "$pattern" "$dmg" "$tmp_dir"

  target_dir="${INSTALL_DIR:-/Applications}"
  if [[ ! -w "$target_dir" ]]; then
    target_dir="$HOME/Applications"
    mkdir -p "$target_dir"
  fi

  mount_dir="$(mktemp -d)"
  hdiutil attach "$dmg" -mountpoint "$mount_dir" -nobrowse -quiet

  app="$(find "$mount_dir" -maxdepth 1 -type d -name "*.app" -print -quit)"
  if [[ -z "$app" ]]; then
    echo "error: no .app found in DMG" >&2
    exit 1
  fi
  rm -rf "$target_dir/$(basename "$app")"
  cp -R "$app" "$target_dir/"

  echo "Installed: $target_dir/$(basename "$app")"
}

os="$(uname -s)"
arch="$(uname -m)"
tmp_dir="$(mktemp -d)"

case "$os" in
  Linux) install_linux_appimage "$arch" "$tmp_dir" ;;
  Darwin) install_macos_dmg "$arch" "$tmp_dir" ;;
  *)
    echo "error: unsupported OS for this script: $os. Use scripts/install-gui.ps1 on Windows." >&2
    exit 1
    ;;
esac
