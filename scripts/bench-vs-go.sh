#!/usr/bin/env bash
# Alias: prefer the three-way script.
exec "$(cd "$(dirname "$0")" && pwd)/bench-vs-go-rust.sh" "$@"
