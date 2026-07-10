#!/usr/bin/env bash
# Download, verify, and install a packaged Mako release.
# Usage:
#   curl -fsSL https://.../install-release.sh | bash
#   ./scripts/install-release.sh --version v0.1.0 --prefix "$HOME/.local"
#   MAKO_RELEASE_BASE_URL=file:///path/to/dist ./scripts/install-release.sh --artifact mako-aarch64-apple-darwin
set -euo pipefail

VERSION="${MAKO_VERSION:-latest}"
PREFIX="${PREFIX:-$HOME/.local}"
ARTIFACT="${MAKO_ARTIFACT:-}"
BASE_URL="${MAKO_RELEASE_BASE_URL:-}"
RUN_DOCTOR=1

usage() {
  cat <<'EOF'
Usage: install-release.sh [options]

Options:
  --version <tag|latest>  Release version to install (default: latest)
  --prefix <path>         Install prefix (default: $HOME/.local)
  --artifact <name>       Override detected artifact name
  --base-url <url>        Asset directory URL (supports https:// and file://)
  --skip-doctor           Do not run mako doctor after install
  -h, --help              Show this help

Environment:
  MAKO_VERSION, PREFIX, MAKO_ARTIFACT, MAKO_RELEASE_BASE_URL
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:?missing value for --version}"
      shift 2
      ;;
    --prefix)
      PREFIX="${2:?missing value for --prefix}"
      shift 2
      ;;
    --artifact)
      ARTIFACT="${2:?missing value for --artifact}"
      shift 2
      ;;
    --base-url)
      BASE_URL="${2:?missing value for --base-url}"
      shift 2
      ;;
    --skip-doctor)
      RUN_DOCTOR=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: missing required command: $1" >&2
    exit 1
  }
}

detect_artifact() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64) echo "mako-aarch64-apple-darwin" ;;
    Darwin:*) echo "mako-x86_64-apple-darwin" ;;
    Linux:aarch64|Linux:arm64) echo "mako-aarch64-unknown-linux-gnu" ;;
    Linux:*) echo "mako-x86_64-unknown-linux-gnu" ;;
    *)
      echo "error: unsupported host $os/$arch; pass --artifact explicitly" >&2
      exit 1
      ;;
  esac
}

download() {
  local url="$1" out="$2"
  case "$url" in
    file://*)
      cp "${url#file://}" "$out"
      ;;
    http://*|https://*)
      curl -fsSL "$url" -o "$out"
      ;;
    *)
      cp "$url" "$out"
      ;;
  esac
}

if [[ -z "$ARTIFACT" ]]; then
  ARTIFACT="$(detect_artifact)"
fi

if [[ -z "$BASE_URL" ]]; then
  if [[ "$VERSION" == "latest" ]]; then
    BASE_URL="https://github.com/loreste/mako/releases/latest/download"
  else
    BASE_URL="https://github.com/loreste/mako/releases/download/$VERSION"
  fi
fi
BASE_URL="${BASE_URL%/}"

need_cmd tar
need_cmd shasum
need_cmd awk
if [[ "$BASE_URL" == http://* || "$BASE_URL" == https://* ]]; then
  need_cmd curl
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/mako-install.XXXXXX")"
cleanup() {
  rm -rf "$WORK"
}
trap cleanup EXIT

ARCHIVE="$WORK/$ARTIFACT.tar.gz"
CHECKSUM_FILE="$WORK/$ARTIFACT.sha256"
ARCHIVE_URL="$BASE_URL/$ARTIFACT.tar.gz"
CHECKSUM_URL="$BASE_URL/$ARTIFACT.sha256"

echo "mako release install"
echo "  version:  $VERSION"
echo "  artifact: $ARTIFACT"
echo "  prefix:   $PREFIX"
echo "  source:   $BASE_URL"

download "$ARCHIVE_URL" "$ARCHIVE"
download "$CHECKSUM_URL" "$CHECKSUM_FILE"

EXPECTED="$(
  awk -v f="$ARTIFACT.tar.gz" '$2 == f { print $1; found=1 } END { if (!found) exit 1 }' "$CHECKSUM_FILE"
)" || {
  echo "error: checksum file does not contain $ARTIFACT.tar.gz" >&2
  exit 1
}
ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{ print $1 }')"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "error: checksum mismatch for $ARTIFACT.tar.gz" >&2
  echo "  expected: $EXPECTED" >&2
  echo "  actual:   $ACTUAL" >&2
  exit 1
fi
echo "checksum: ok"

tar -xzf "$ARCHIVE" -C "$WORK"
INSTALLER="$WORK/$ARTIFACT/scripts/install.sh"
if [[ ! -x "$INSTALLER" ]]; then
  echo "error: release archive missing executable scripts/install.sh" >&2
  exit 1
fi

PREFIX="$PREFIX" "$INSTALLER" --skip-build
if [[ "$RUN_DOCTOR" -eq 1 ]]; then
  (
    cd "$WORK"
    MAKO_RUNTIME="$PREFIX/share/mako/runtime" \
      MAKO_STD="$PREFIX/share/mako/std" \
      "$PREFIX/bin/mako" doctor
  )
fi
echo "mako installed at $PREFIX/bin/mako"
