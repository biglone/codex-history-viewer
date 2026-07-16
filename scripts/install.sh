#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-biglone/codex-history-viewer}"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-}"
TARGET="${TARGET:-}"
ADD_TO_PATH="${ADD_TO_PATH:-0}"
SILENT="${SILENT:-0}"

usage() {
  cat <<'EOF'
Install Codex History Viewer GUI or CLI from the command line.

Usage:
  install.sh --target cli|gui [--version TAG|latest] [--install-dir DIR] [--repo OWNER/REPO] [--add-to-path] [--silent]

Environment:
  TARGET        cli or gui
  VERSION       Release tag to install, defaults to latest
  INSTALL_DIR   Install directory passed to the underlying installer
  REPO          GitHub repository, defaults to biglone/codex-history-viewer
  GH_TOKEN      Optional token for private or draft releases
  ADD_TO_PATH   For Windows CLI installer compatibility
  SILENT        For Windows GUI installer compatibility

Examples:
  curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install.sh | bash -s -- --target cli
  curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install.sh | bash -s -- --target gui --version v1.3.8
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      if [[ $# -lt 2 ]]; then
        echo "error: --target requires a value" >&2
        exit 2
      fi
      TARGET="${2:-}"
      shift 2
      ;;
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
    --add-to-path)
      ADD_TO_PATH=1
      shift
      ;;
    --silent)
      SILENT=1
      shift
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

if [[ "$TARGET" != "cli" && "$TARGET" != "gui" ]]; then
  echo "error: --target must be cli or gui" >&2
  usage >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
helper_name="install-${TARGET}.sh"
helper_path="$script_dir/$helper_name"

run_helper() {
  local helper="$1"
  local args=(--version "$VERSION" --repo "$REPO")
  if [[ -n "$INSTALL_DIR" ]]; then
    args+=(--install-dir "$INSTALL_DIR")
  fi
  if [[ "$TARGET" == "gui" && "$SILENT" == "1" ]]; then
    args+=(--silent)
  fi
  if [[ "$TARGET" == "cli" && "$ADD_TO_PATH" == "1" ]]; then
    args+=(--add-to-path)
  fi
  bash "$helper" "${args[@]}"
}

if [[ -f "$helper_path" ]]; then
  run_helper "$helper_path"
  exit 0
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
tmp_helper="$tmp_dir/$helper_name"
raw_url="https://raw.githubusercontent.com/${REPO}/main/scripts/${helper_name}"

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$raw_url" -o "$tmp_helper"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$tmp_helper" "$raw_url"
else
  echo "error: curl or wget is required" >&2
  exit 1
fi

chmod +x "$tmp_helper"
run_helper "$tmp_helper"
