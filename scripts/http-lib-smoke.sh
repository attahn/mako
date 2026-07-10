#!/usr/bin/env bash
# Smoke: HTTP library server + client (GET/POST, last_status).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
command -v curl >/dev/null || { echo "skip: no curl"; exit 0; }
BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"
[[ -x "$BIN" ]] || { cargo build --release; BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"; }
mkdir -p out
"$BIN" build examples/http_lib/main.mko -o out/http_lib
./out/http_lib 6 >/tmp/mako_http_lib_srv.log 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
sleep 0.25
curl -sf http://127.0.0.1:18250/health | grep -q ok
curl -sf -X POST -d 'hello' http://127.0.0.1:18250/echo | grep -q hello
# Client builtins against the live server (separate process)
cat > /tmp/mako_http_client_check.mko <<'EOF'
fn main() {
    let b = http_get("http://127.0.0.1:18250/health")
    print(b)
    print_int(http_last_status())
    let p = http_post("http://127.0.0.1:18250/echo", "from-client")
    print(p)
    print_int(http_last_status())
}
EOF
"$BIN" run /tmp/mako_http_client_check.mko | tee /tmp/mako_http_client_out.txt
grep -q ok /tmp/mako_http_client_out.txt
grep -q from-client /tmp/mako_http_client_out.txt
grep -q 200 /tmp/mako_http_client_out.txt
echo "http_lib smoke ok"
