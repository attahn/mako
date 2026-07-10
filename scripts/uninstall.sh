#!/usr/bin/env bash
# Remove files installed by scripts/install.sh.
# Usage:
#   ./scripts/uninstall.sh
#   PREFIX=/usr/local ./scripts/uninstall.sh
#   ./scripts/uninstall.sh --dry-run
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
SHARE_DIR="${SHARE_DIR:-$PREFIX/share/mako}"
DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then DRY_RUN=1; fi

remove_path() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    echo "skip missing $path"
    return
  fi
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "would remove $path"
  else
    rm -rf "$path"
    echo "removed $path"
  fi
}

echo "mako uninstall"
echo "  prefix: $PREFIX"
echo "  bin:    $BIN_DIR/mako"
echo "  share:  $SHARE_DIR"

remove_path "$BIN_DIR/mako"
remove_path "$SHARE_DIR"

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "Dry run only. Re-run without --dry-run to remove files."
else
  echo "Done. Remove $BIN_DIR from PATH if it was added only for Mako."
fi
