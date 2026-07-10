# WASI

```bash
mako build examples/wasi_hello.mko --target wasm32-wasip1 -o out/wasi_hello.wasm
wasmtime out/wasi_hello.wasm
```

Needs **wasi-sdk** (`WASI_SDK_PATH`). Preview1: hello, argv, FS preopens.
Sockets/TLS/DB stay native-only.

Verify: `./scripts/wasi-verify.sh` (skips cleanly if SDK missing).
Docs: [WASM.md](../WASM.md).
