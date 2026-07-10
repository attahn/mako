#!/usr/bin/env bash
# Verify mako → wasm32-wasip1 → wasmtime (skips cleanly when toolchain missing).
# Checks: wasi_hello · wasi_args_env · wasi_fs (--dir preopens).
# Usage: ./scripts/wasi-verify.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

find_wasi_sdk() {
  if [[ -n "${WASI_SDK_PATH:-}" && -x "${WASI_SDK_PATH}/bin/clang" ]]; then
    echo "$WASI_SDK_PATH"
    return 0
  fi
  for p in /opt/wasi-sdk /usr/local/wasi-sdk "$ROOT/.mako/toolchains/wasi-sdk"; do
    if [[ -x "$p/bin/clang" ]]; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

find_wasmtime() {
  if command -v wasmtime >/dev/null 2>&1; then
    command -v wasmtime
    return 0
  fi
  local cand
  for cand in "$ROOT/.mako/toolchains"/wasmtime-*/wasmtime; do
    if [[ -x "$cand" ]]; then
      echo "$cand"
      return 0
    fi
  done
  return 1
}

mako_bin() {
  if [[ -x "$ROOT/target/release/mako" ]]; then
    echo "$ROOT/target/release/mako"
    return 0
  fi
  local td
  td="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null || true)"
  if [[ -n "$td" && -x "$td/release/mako" ]]; then
    echo "$td/release/mako"
    return 0
  fi
  return 1
}

if ! SDK="$(find_wasi_sdk)"; then
  echo "skip: wasi-sdk not found (set WASI_SDK_PATH or install from https://github.com/WebAssembly/wasi-sdk/releases)"
  exit 0
fi
export WASI_SDK_PATH="$SDK"
echo "using wasi-sdk: $WASI_SDK_PATH"

if ! MAKO="$(mako_bin)"; then
  echo "building mako (release)…"
  cargo build --release
  MAKO="$(mako_bin)" || {
    echo "error: could not locate mako binary after cargo build"
    exit 1
  }
fi
echo "using mako: $MAKO"

mkdir -p "$ROOT/out"

build_one() {
  local src="$1" out="$2"
  "$MAKO" build "$src" --target wasm32-wasi -o "$out"
  test -s "$out"
  echo "built: $out ($(wc -c < "$out") bytes)"
}

build_one examples/wasi_hello.mko "$ROOT/out/wasi_hello.wasm"
build_one examples/wasi_args_env.mko "$ROOT/out/wasi_args_env.wasm"
build_one examples/wasi_fs.mko "$ROOT/out/wasi_fs.wasm"

if ! WT="$(find_wasmtime)"; then
  echo "skip run: wasmtime not found (install from https://github.com/bytecodealliance/wasmtime/releases)"
  echo "ok: wasm artifacts present"
  exit 0
fi

export WASMTIME_HOME="${WASMTIME_HOME:-$ROOT/.mako/wasmtime-cache}"
mkdir -p "$WASMTIME_HOME"

echo "running: $WT out/wasi_hello.wasm"
HELLO_TXT="$("$WT" "$ROOT/out/wasi_hello.wasm")"
echo "$HELLO_TXT"
echo "$HELLO_TXT" | grep -q "hello from mako wasi"
echo "$HELLO_TXT" | grep -q "55"

# wasmtime: --env KEY=VAL before the module; program args after the module path.
echo "running: $WT --env MAKO_WASI_GREET=hi out/wasi_args_env.wasm hello"
ARGS_TXT="$("$WT" --env MAKO_WASI_GREET=hi "$ROOT/out/wasi_args_env.wasm" hello)"
echo "$ARGS_TXT"
echo "$ARGS_TXT" | grep -q "hello"
echo "$ARGS_TXT" | grep -q "hi"
echo "$ARGS_TXT" | head -1 | grep -Eq '^[2-9]|[1-9][0-9]'

# FS preopens: map host sandbox to guest `.` so relative paths work.
FS_DIR="$ROOT/out/wasi_fs_sandbox"
rm -rf "$FS_DIR"
mkdir -p "$FS_DIR"
printf 'seed' > "$FS_DIR/in.txt"
echo "running: $WT --dir=$FS_DIR::. out/wasi_fs.wasm"
FS_TXT="$("$WT" --dir="$FS_DIR::." "$ROOT/out/wasi_fs.wasm")"
echo "$FS_TXT"
echo "$FS_TXT" | grep -q "seed"
echo "$FS_TXT" | grep -q "wrote"
echo "$FS_TXT" | grep -q "^0$"
test -f "$FS_DIR/out.txt"
grep -q "wrote" "$FS_DIR/out.txt"

echo "ok: wasi hello + args/env + fs preopens verified"
