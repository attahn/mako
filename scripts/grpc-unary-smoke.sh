#!/usr/bin/env bash
# gRPC unary TLS smoke: tls_serve_grpc_once + tls_grpc_unary (protobuf-framed seed).
# Skips cleanly without OpenSSL. Not grpcurl / full gRPC stack.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

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

if ! MAKO="$(mako_bin)"; then
  cargo build --release
  MAKO="$(mako_bin)" || { echo "error: mako binary not found"; exit 1; }
fi

if [[ ! -f runtime/certs/dev.crt ]]; then
  echo "skip: runtime/certs/dev.crt missing"
  echo "ok: grpc smoke skipped"
  exit 0
fi

# Soft-skip when OpenSSL not linked: live test early-returns without MAKO_LIVE_TLS,
# but we force live and accept soft-fail from missing SSL in binary.
OUT="$("$MAKO" test examples/testing/tls_live_test.mko -r GrpcUnary 2>&1)" || true
if echo "$OUT" | grep -qi 'OpenSSL not linked\|tls_serve_grpc_once: OpenSSL'; then
  echo "skip: OpenSSL not linked"
  echo "ok: grpc smoke skipped"
  exit 0
fi

export MAKO_LIVE_TLS=1
"$MAKO" test examples/testing/tls_live_test.mko -r Grpc
echo "ok: grpc unary+stream smoke"
