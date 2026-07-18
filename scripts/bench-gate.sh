#!/usr/bin/env bash
# Gate: Mako release microbench must stay within a factor of Rust.
# Speed bar: as close to Rust as possible — fail CI if we regress badly.
#
# Usage:
#   ./scripts/bench-gate.sh           # default max 2.0×
#   ./scripts/bench-gate.sh 1.5       # strict (CI stretch goal)
#   MAKO_BENCH_STRICT=1 ./scripts/bench-gate.sh  # same as 1.5
#   MAKO_BENCH_RUNS=5 ./scripts/bench-gate.sh    # increase samples
# Checks fib30x5, slice100k, map50k.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [[ -n "${1:-}" ]]; then
  MAX_RATIO="$1"
elif [[ "${MAKO_BENCH_STRICT:-}" == "1" ]]; then
  MAX_RATIO="1.5"
else
  MAX_RATIO="2.0"
fi
RUNS="${MAKO_BENCH_RUNS:-3}"
if [[ ! "$RUNS" =~ ^[1-9][0-9]*$ ]]; then
  echo "bench-gate: MAKO_BENCH_RUNS must be a positive integer" >&2
  exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "bench-gate: rustc not found — skip (install rustc to enforce gate)"
  exit 0
fi

BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"
if [[ ! -x "$BIN" ]]; then
  cargo build --release
  BIN="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/release/mako"
fi

mkdir -p out
"$BIN" build --release --no-incremental examples/bench/micro.mko -o out/bench_micro_gate
rustc -C opt-level=3 -C lto -C codegen-units=1 \
  examples/bench/micro_rs.rs -o out/bench_micro_rs_gate 2>/dev/null

run_bench() {
  local bin="$1"
  local run
  for ((run = 0; run < RUNS; run++)); do
    "$bin"
  done
}

mako_out="$(run_bench ./out/bench_micro_gate)"
rust_out="$(run_bench ./out/bench_micro_rs_gate)"

extract_ns() {
  local out="$1" key="$2"
  echo "$out" | awk -v k="$key" '
    $0==k { getline; getline; print }
  '
}

fail=0
for key in fib30x5 slice100k map50k; do
  m="$(extract_ns "$mako_out" "$key")"
  r="$(extract_ns "$rust_out" "$key")"
  if [[ -z "$m" || -z "$r" ]]; then
    echo "bench-gate: could not parse $key"
    fail=1
    continue
  fi
  python3 - "$key" "$MAX_RATIO" "$m" "$r" <<'PY' || fail=1
import statistics
import sys

key, max_r = sys.argv[1], float(sys.argv[2])
mako_samples = [float(v) for v in sys.argv[3].split()]
rust_samples = [float(v) for v in sys.argv[4].split()]
if not mako_samples or not rust_samples or len(mako_samples) != len(rust_samples):
    print(f"bench-gate {key}: invalid sample count")
    sys.exit(1)
mako = statistics.median(mako_samples)
rust = statistics.median(rust_samples)
if rust <= 0:
    print(f"bench-gate {key}: invalid rust median ns {rust}")
    sys.exit(1)
ratio = mako / rust
print(f"bench-gate {key}: mako={mako:.0f}ns rust={rust:.0f}ns ratio={ratio:.2f}x (median of {len(mako_samples)} runs; max {max_r}x)")
if ratio > max_r:
    print(f"FAIL: {key} — Mako is {ratio:.2f}x slower than Rust (limit {max_r}x)")
    sys.exit(1)
PY
done

if [[ "$fail" -ne 0 ]]; then
  echo "bench-gate: FAILED"
  exit 1
fi
echo "PASS: all kernels within speed gate (≤${MAX_RATIO}× Rust)"
