#!/usr/bin/env python3
"""Parse interleaved bench output from bench-vs-go-rust.sh into a ratio table."""
import sys

def parse(text: str):
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    results = {}
    i = 0
    lang = None
    while i < len(lines):
        if lines[i] == "lang" and i + 1 < len(lines):
            lang = lines[i + 1]
            results[lang] = {}
            i += 2
            continue
        if lang and lines[i] in ("fib30x5", "slice100k", "map50k") and i + 2 < len(lines):
            name = lines[i]
            # value then ns
            try:
                ns = int(lines[i + 2])
                results[lang][name] = ns
            except ValueError:
                pass
            i += 3
            continue
        i += 1
    return results

def main():
    text = sys.stdin.read()
    r = parse(text)
    if not r:
        print("no results parsed", file=sys.stderr)
        sys.exit(1)
    kernels = ["fib30x5", "slice100k", "map50k"]
    langs = [L for L in ("mako", "go", "rust") if L in r]
    print(f"{'kernel':12} " + " ".join(f"{L:>12}" for L in langs) + "   vs_go   vs_rust")
    for k in kernels:
        row = [f"{k:12}"]
        for L in langs:
            row.append(f"{r[L].get(k, 0):12d}")
        go = r.get("go", {}).get(k)
        rs = r.get("rust", {}).get(k)
        mk = r.get("mako", {}).get(k)
        vg = f"{(mk/go):.2f}x" if go and mk else "-"
        vr = f"{(mk/rs):.2f}x" if rs and mk else "-"
        print(" ".join(row) + f"   {vg:>6}  {vr:>7}")

if __name__ == "__main__":
    main()
