#!/usr/bin/env bash
# Sketch: build Cloudflare quiche as a C-linkable library for Mako.
# Does NOT wire find_quiche() — documents the FFI path and records real errors.
#
# Prerequisites: rustup + cargo, git, cmake (BoringSSL), pkg-config
# Usage:
#   ./runtime/third_party/build_quiche_ffi.sh          # print steps only
#   ./runtime/third_party/build_quiche_ffi.sh --try    # clone + cargo build (may fail)
#
# Expected outcome when fully wired:
#   - libquiche.{a,dylib} under runtime/third_party/quiche/target/release/
#   - header: runtime/third_party/quiche/src/quiche/include/quiche.h
#   - find_quiche() in src/main.rs + -DMAKO_HAS_QUICHE
#   - mako_quiche.h builtins
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/runtime/third_party/quiche"
SRC="$OUT/src"
TARGET_DIR="$OUT/target"
LOG="$OUT/build_attempt.log"
TRY=0
if [[ "${1:-}" == "--try" ]]; then TRY=1; fi

echo "mako quiche FFI sketch"
echo "  repo root: $ROOT"
echo "  clone:     $SRC"
echo "  target:    $TARGET_DIR"
echo ""
echo "Quiche is a Rust workspace. Suggested steps:"
echo "  1. git clone --recursive https://github.com/cloudflare/quiche.git $SRC"
echo "  2. CARGO_TARGET_DIR=$TARGET_DIR cargo build -p quiche --release --features ffi,pkg-config-meta"
echo "  3. Point find_quiche() at $TARGET_DIR/release + $SRC/quiche/include/"
echo "  4. Add -DMAKO_HAS_QUICHE -lquiche (and BoringSSL deps as needed)"
echo ""

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found — sketch only; skipping clone/build."
  exit 0
fi
echo "cargo found: $(cargo --version)"
echo "rustc found: $(rustc --version 2>/dev/null || echo missing)"

if [[ "$TRY" -ne 1 ]]; then
  echo "Not cloning/building (pass --try to attempt)."
  exit 0
fi

mkdir -p "$OUT"
: > "$LOG"
{
  echo "=== quiche FFI build attempt $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
  echo "cargo: $(cargo --version)"
  echo "rustc: $(rustc --version 2>/dev/null || true)"
  echo "cmake: $(cmake --version 2>/dev/null | head -1 || echo missing)"
  echo "CARGO_TARGET_DIR (env before unset): ${CARGO_TARGET_DIR:-<unset>}"
} | tee -a "$LOG"

if [[ ! -d "$SRC/.git" ]]; then
  echo "Cloning quiche (recursive)…" | tee -a "$LOG"
  if ! git clone --depth 1 --recursive https://github.com/cloudflare/quiche.git "$SRC" >>"$LOG" 2>&1; then
    echo "FAIL: git clone failed — see $LOG"
    tail -n 40 "$LOG" || true
    exit 1
  fi
else
  echo "Using existing clone at $SRC" | tee -a "$LOG"
fi

# Must build from quiche tree; force local target dir (Cursor may set CARGO_TARGET_DIR).
echo "Building -p quiche --release --features ffi,pkg-config-meta…" | tee -a "$LOG"
set +e
(
  cd "$SRC" || exit 99
  export CARGO_TARGET_DIR="$TARGET_DIR"
  cargo build -p quiche --release --features ffi,pkg-config-meta
) >>"$LOG" 2>&1
RC=$?
set -e

if [[ $RC -ne 0 ]]; then
  echo "FAIL: cargo build exited $RC — exact error tail:"
  echo "-----"
  tail -n 60 "$LOG" || true
  echo "-----"
  echo "Full log: $LOG"
  echo "Quiche remains unlinked in Mako (honest: build did not succeed)."
  exit $RC
fi

HDR="$SRC/quiche/include/quiche.h"
echo "OK: cargo -p quiche finished."
echo "Header expected: $HDR"
if [[ -f "$HDR" ]]; then
  echo "  found: $HDR"
else
  echo "  MISSING header at expected path"
fi

mkdir -p "$TARGET_DIR/release"
shopt -s nullglob
LIBS=("$TARGET_DIR/release"/libquiche.*)
if [[ ${#LIBS[@]} -eq 0 ]]; then
  # Cursor/sandbox may override CARGO_TARGET_DIR; copy from cargo metadata dir if set.
  FALLBACK="${CARGO_TARGET_DIR_SAVED:-}"
  if [[ -z "$FALLBACK" && -n "${ORIG_CARGO_TARGET_DIR:-}" ]]; then
    FALLBACK="$ORIG_CARGO_TARGET_DIR"
  fi
  echo "WARN: no libquiche.* under $TARGET_DIR/release — checking fallbacks" | tee -a "$LOG"
fi

# Ensure install_name is @rpath so Mako's -Wl,-rpath finds the dylib.
DYLIB="$TARGET_DIR/release/libquiche.dylib"
if [[ -f "$DYLIB" ]]; then
  if command -v install_name_tool >/dev/null 2>&1; then
    install_name_tool -id "@rpath/libquiche.dylib" "$DYLIB" 2>>"$LOG" || true
  fi
fi

LIBS=("$TARGET_DIR/release"/libquiche.*)
if [[ ${#LIBS[@]} -eq 0 ]]; then
  echo "WARN: no libquiche.* under $TARGET_DIR/release"
  echo "      Copy libs then: install_name_tool -id @rpath/libquiche.dylib …"
  echo "      Mako find_quiche() also searches CARGO_TARGET_DIR + MAKO_QUICHE_ROOT."
  ls -la "$TARGET_DIR/release" 2>&1 | head -20 || true
  exit 2
fi
echo "Libs:"
ls -la "${LIBS[@]}"
echo "Mako: find_quiche() → -DMAKO_HAS_QUICHE; builtins quiche_available / quiche_version."
exit 0
