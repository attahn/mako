#!/usr/bin/env bash
# Smoke: multi-route JSON API (skips cleanly if curl missing).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
command -v curl >/dev/null || { echo "skip: no curl"; exit 0; }
BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"
[[ -x "$BIN" ]] || { cargo build --release; BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"; }
mkdir -p out
"$BIN" build examples/api_backend/main.mko -o out/api_backend
./out/api_backend 10 >/tmp/mako_api_srv.log 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
sleep 0.3
curl -sf http://127.0.0.1:18200/health | grep -q ok
curl -sf -X POST -H 'Content-Type: application/json' -d '{"title":"t1","body":"b1"}' http://127.0.0.1:18200/v1/notes | grep -q created
curl -sf http://127.0.0.1:18200/v1/notes | grep -q t1
echo "api_backend smoke ok"
