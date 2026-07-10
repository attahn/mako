#!/usr/bin/env bash
# HTTPS /health smoke via tls_serve_n + self-signed runtime/certs.
# Skips cleanly when OpenSSL was not linked into the binary.
# Usage: ./scripts/https-server-smoke.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PORT=18443
OUT="${TMPDIR:-/tmp}/mako_https_server_$$"
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

openssl_linked() {
  local bin="$1"
  if command -v otool >/dev/null 2>&1; then
    otool -L "$bin" 2>/dev/null | grep -q '[Ll]ibssl' && return 0
  fi
  if command -v ldd >/dev/null 2>&1; then
    ldd "$bin" 2>/dev/null | grep -q 'libssl' && return 0
  fi
  # Fallback: binary strings / nm
  if strings "$bin" 2>/dev/null | grep -q 'SSL_accept'; then
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

"$MAKO" build examples/https_server.mko -o "$BIN"

if ! openssl_linked "$BIN"; then
  echo "skip: OpenSSL not linked into $BIN (install openssl + rebuild mako)"
  echo "ok: https smoke skipped"
  exit 0
fi

if [[ ! -f runtime/certs/dev.crt || ! -f runtime/certs/dev.key ]]; then
  echo "skip: runtime/certs/dev.{crt,key} missing"
  exit 0
fi

"$BIN" >"$LOG" 2>&1 &
SPID=$!

for _ in $(seq 1 50); do
  if grep -q "tls_serve_n" "$LOG" 2>/dev/null; then
    break
  fi
  if ! kill -0 "$SPID" 2>/dev/null; then
    echo "error: https server exited early"
    cat "$LOG" || true
    exit 1
  fi
  sleep 0.05
done

BODY="$(curl -sk -m 3 "https://127.0.0.1:$PORT/")"
echo "GET / → $BODY"
echo "$BODY" | grep -q "hello from mako https"

HEALTH="$(curl -sk -m 3 -D - "https://127.0.0.1:$PORT/health")"
echo "GET /health →"
echo "$HEALTH"
echo "$HEALTH" | grep -q '"ok":true'
echo "$HEALTH" | tr -d '\r' | grep -qi '^Content-Type:.*application/json'

CODE="$(curl -sk -m 3 -o /tmp/mako_https_404_$$.txt -w "%{http_code}" "https://127.0.0.1:$PORT/nope")"
echo "GET /nope → HTTP $CODE"
test "$CODE" = "404"
grep -q "missing" /tmp/mako_https_404_$$.txt
rm -f /tmp/mako_https_404_$$.txt

for _ in $(seq 1 100); do
  if ! kill -0 "$SPID" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if kill -0 "$SPID" 2>/dev/null; then
  echo "error: https server still running"
  exit 1
fi
wait "$SPID" || true
SPID=
grep -q "https_server done" "$LOG"
echo "ok: https_server smoke"
