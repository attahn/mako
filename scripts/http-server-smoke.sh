#!/usr/bin/env bash
# Build examples/http_server.mko (max-request demo), curl /, /health, 404;
# verify server exits after N requests. Exit 0 on success.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PORT=18100
# Exact request budget: / + /health + /nope + 40× /health = 43
MAX_REQ=43
OUT="${TMPDIR:-/tmp}/mako_http_server_$$"
BIN="$OUT.bin"
LOG="$OUT.log"

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

cleanup() {
  if [[ -n "${SPID:-}" ]]; then
    kill "$SPID" 2>/dev/null || true
    wait "$SPID" 2>/dev/null || true
  fi
  rm -f "$BIN" "$LOG"
}
trap cleanup EXIT

if ! MAKO="$(mako_bin)"; then
  cargo build --release
  MAKO="$(mako_bin)" || { echo "error: mako binary not found"; exit 1; }
fi

"$MAKO" build examples/http_server.mko -o "$BIN"
"$BIN" "$MAX_REQ" >"$LOG" 2>&1 &
SPID=$!

# Wait for bind
for _ in $(seq 1 50); do
  if grep -q "http_server on :$PORT" "$LOG" 2>/dev/null; then
    break
  fi
  if ! kill -0 "$SPID" 2>/dev/null; then
    echo "error: server exited early"
    cat "$LOG" || true
    exit 1
  fi
  sleep 0.05
done

BODY="$(curl -sS -m 2 "http://127.0.0.1:$PORT/")"
echo "GET / → $BODY"
echo "$BODY" | grep -q "hello from mako"

HEALTH_HDRS="$(curl -sS -m 2 -D - "http://127.0.0.1:$PORT/health")"
echo "GET /health →"
echo "$HEALTH_HDRS"
echo "$HEALTH_HDRS" | grep -q '"ok":true'
echo "$HEALTH_HDRS" | tr -d '\r' | grep -qi '^Content-Type:.*application/json'

CODE="$(curl -sS -m 2 -o /tmp/mako_http_404_$$.txt -w "%{http_code}" "http://127.0.0.1:$PORT/nope")"
echo "GET /nope → HTTP $CODE"
test "$CODE" = "404"
grep -q "missing" /tmp/mako_http_404_$$.txt
rm -f /tmp/mako_http_404_$$.txt

# Conn-table stress: many keep-alive curls must not exhaust slots
for i in $(seq 1 40); do
  curl -sS -m 2 "http://127.0.0.1:$PORT/health" | grep -q '"ok":true'
done
echo "40x /health ok"

# Server should exit after MAX_REQ (no kill needed)
for _ in $(seq 1 100); do
  if ! kill -0 "$SPID" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if kill -0 "$SPID" 2>/dev/null; then
  echo "error: server still running after $MAX_REQ requests"
  exit 1
fi
wait "$SPID" || true
SPID=
grep -q "http_server done" "$LOG"
echo "server exited cleanly"

echo "ok: http_server smoke"
