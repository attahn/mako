#!/usr/bin/env bash
# CI helper: build examples/hello.mko to wasm32-wasi via docker/wasi-build.Dockerfile.
# Requires Docker. Exit 0 on success.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/out}"
mkdir -p "$OUT"
IMAGE="mako-wasi-ci:local"
docker build -f "$ROOT/docker/wasi-build.Dockerfile" -t "$IMAGE" "$ROOT"
docker run --rm -v "$OUT:/hostout" --entrypoint sh "$IMAGE" -c \
  'cp /out/hello.wasm /hostout/hello.wasm && ls -la /hostout/hello.wasm'
test -s "$OUT/hello.wasm"
echo "ok: $OUT/hello.wasm ($(wc -c < "$OUT/hello.wasm") bytes)"
