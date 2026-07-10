#!/usr/bin/env bash
# HTTP/2 TLS server smoke via tls_serve_h2_routes + curl --http2.
# Skips cleanly when OpenSSL not linked or curl lacks HTTP/2.
# Usage: ./scripts/h2-server-smoke.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PORT=18446
OUT="${TMPDIR:-/tmp}/mako_h2_server_$$"
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
  if strings "$bin" 2>/dev/null | grep -q 'SSL_accept'; then
    return 0
  fi
  return 1
}

curl_http2_ok() {
  command -v curl >/dev/null 2>&1 || return 1
  curl --version 2>&1 | grep -qi 'HTTP2\|nghttp2' || return 1
  return 0
}

cleanup() {
  if [[ -n "${SPID:-}" ]]; then
    kill "$SPID" 2>/dev/null || true
    wait "$SPID" 2>/dev/null || true
  fi
  rm -f "$BIN" "$LOG"
}
trap cleanup EXIT

if ! curl_http2_ok; then
  echo "skip: curl with HTTP/2 support not found"
  echo "ok: h2 smoke skipped"
  exit 0
fi

if ! MAKO="$(mako_bin)"; then
  cargo build --release
  MAKO="$(mako_bin)" || { echo "error: mako binary not found"; exit 1; }
fi

"$MAKO" build examples/h2_server.mko -o "$BIN"

if ! openssl_linked "$BIN"; then
  echo "skip: OpenSSL not linked into $BIN (install openssl + rebuild mako)"
  echo "ok: h2 smoke skipped"
  exit 0
fi

if [[ ! -f runtime/certs/dev.crt || ! -f runtime/certs/dev.key ]]; then
  echo "skip: runtime/certs/dev.{crt,key} missing"
  exit 0
fi

"$BIN" >"$LOG" 2>&1 &
SPID=$!

for _ in $(seq 1 50); do
  if grep -q "tls_h2_routes\|tls_serve_h2" "$LOG" 2>/dev/null; then
    break
  fi
  if ! kill -0 "$SPID" 2>/dev/null; then
    echo "error: h2 server exited early"
    cat "$LOG" || true
    exit 1
  fi
  sleep 0.05
done

# One TLS connection, two multiplexed GETs (server max_reqs=2).
# Capture version from first URL via -w on a write-out after both bodies.
OUT_TXT="$(curl -sk --http2 -m 5 -w "\n__http_version=%{http_version}\n" \
  "https://127.0.0.1:$PORT/health" \
  "https://127.0.0.1:$PORT/")"
echo "$OUT_TXT"
echo "$OUT_TXT" | grep -q '"ok":true'
echo "$OUT_TXT" | grep -q "hello from mako h2"
# Last transfer's version should be HTTP/2.
echo "$OUT_TXT" | grep -q '__http_version=2'

for _ in $(seq 1 100); do
  if ! kill -0 "$SPID" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if kill -0 "$SPID" 2>/dev/null; then
  echo "error: h2 server still running"
  exit 1
fi
wait "$SPID" || true
SPID=
grep -q "h2_server done" "$LOG"
echo "ok: h2_server smoke"
