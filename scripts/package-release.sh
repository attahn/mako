#!/usr/bin/env bash
# Package a self-contained Mako release artifact.
# Usage: ./scripts/package-release.sh [artifact-name]
# Example: ./scripts/package-release.sh mako-aarch64-apple-darwin
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NAME="${1:-${ARTIFACT_NAME:-}}"
if [[ -z "$NAME" ]]; then
  ARCH="$(uname -m)"
  OS="$(uname -s)"
  case "$OS" in
    Darwin)
      case "$ARCH" in
        arm64|aarch64) NAME="mako-aarch64-apple-darwin" ;;
        *) NAME="mako-x86_64-apple-darwin" ;;
      esac
      ;;
    Linux)
      case "$ARCH" in
        aarch64|arm64) NAME="mako-aarch64-unknown-linux-gnu" ;;
        *) NAME="mako-x86_64-unknown-linux-gnu" ;;
      esac
      ;;
    *)
      echo "error: set ARTIFACT_NAME or pass name as arg" >&2
      exit 1
      ;;
  esac
fi

BIN="$ROOT/target/release/mako"
if [[ ! -x "$BIN" ]]; then
  echo "Building release…"
  cargo build --release --quiet
fi
if [[ ! -x "$BIN" ]]; then
  echo "error: missing $BIN" >&2
  exit 1
fi

DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/mako/runtime" "$STAGE/share/mako/std" "$STAGE/share/mako/docs" "$STAGE/scripts"

cp "$BIN" "$STAGE/bin/mako"
chmod +x "$STAGE/bin/mako"
for h in "$ROOT"/runtime/*.h; do
  cp "$h" "$STAGE/share/mako/runtime/"
done
if [[ -d "$ROOT/runtime/certs" ]]; then
  mkdir -p "$STAGE/share/mako/runtime/certs"
  cp -R "$ROOT/runtime/certs/." "$STAGE/share/mako/runtime/certs/"
fi
if [[ -d "$ROOT/std" ]]; then
  cp -R "$ROOT/std/." "$STAGE/share/mako/std/"
fi
if [[ -d "$ROOT/editors/vscode" ]]; then
  mkdir -p "$STAGE/share/mako/editors"
  cp -R "$ROOT/editors/vscode" "$STAGE/share/mako/editors/vscode"
fi
for doc in README.md CHANGELOG.md; do
  if [[ -f "$ROOT/$doc" ]]; then
    cp "$ROOT/$doc" "$STAGE/share/mako/docs/$doc"
  fi
done
if [[ -d "$ROOT/docs" ]]; then
  mkdir -p "$STAGE/share/mako/docs/docs"
  cp -R "$ROOT/docs/." "$STAGE/share/mako/docs/docs/"
fi
cp "$ROOT/scripts/install.sh" "$STAGE/scripts/install.sh"
cp "$ROOT/scripts/uninstall.sh" "$STAGE/scripts/uninstall.sh"
cp "$ROOT/scripts/install.ps1" "$STAGE/scripts/install.ps1"
cp "$ROOT/scripts/uninstall.ps1" "$STAGE/scripts/uninstall.ps1"
cp "$ROOT/scripts/install-release.sh" "$STAGE/scripts/install-release.sh"
chmod +x "$STAGE/scripts/install.sh" "$STAGE/scripts/uninstall.sh" "$STAGE/scripts/install-release.sh"
cat > "$STAGE/README.txt" << EOF
Mako release layout ($NAME)

  bin/mako                 — compiler CLI
  share/mako/runtime/      — C runtime headers (required to compile .mko)
  share/mako/std/          — standard library sources
  share/mako/editors/      — editor integration scaffolds
  share/mako/docs/         — release docs snapshot
  scripts/install.sh       — install this artifact into PREFIX
  scripts/install-release.sh — download, verify, and install release artifacts
  scripts/uninstall.sh     — remove files installed under PREFIX

Install:
  # Unix
  PREFIX=\$HOME/.local ./scripts/install.sh --skip-build
  # or copy bin/mako onto PATH and:
  export MAKO_RUNTIME=\$(pwd)/share/mako/runtime

Docs: docs/RELEASE.md
EOF

tar -C "$DIST" -czf "$DIST/$NAME.tar.gz" "$NAME"
(
  cd "$DIST"
  shasum -a 256 "$NAME.tar.gz" "$NAME/bin/mako" > "$NAME.sha256"
)
# Also leave a bare binary copy for GitHub Releases convenience
cp "$STAGE/bin/mako" "$DIST/$NAME"
cp "$ROOT/scripts/install-release.sh" "$DIST/install-release.sh"
echo "Packed $DIST/$NAME.tar.gz, $DIST/$NAME, and $DIST/$NAME.sha256"
