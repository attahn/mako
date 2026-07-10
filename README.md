# Mako

Mako is a programming language for people who build backend software and want
something between Go and Rust. You get Go's simplicity and fast deploys with
stronger memory safety and more predictable performance — without Rust's steep
learning curve for everyday services.

Sources use **`.mko`**. The compiler emits C, then hands off to clang or zig for
native binaries. No mandatory garbage collector. One binary out the door.

**Status:** 0.1.0 MVP complete. 130 tests passing. Standard library covers ~98%
of major Go stdlib areas. See [STATUS.md](docs/STATUS.md) for the honest matrix.

## Install

**macOS / Linux**

```bash
curl -fsSL https://github.com/loreste/mako/releases/latest/download/install-release.sh | bash
```

Or build from source:

```bash
make install                    # installs to ~/.local/bin/mako
mako version                    # mako version mako0.1.0 darwin/arm64
```

**Windows (PowerShell)**

```powershell
cargo build --release
.\scripts\install.ps1
mako version
```

You'll need **Rust** and **clang** (Xcode on macOS, `apt install clang` on Linux,
or LLVM on Windows). Optional deps for specific features: OpenSSL, libnghttp2,
SQLite, libpq, quiche.

Cross-compile with `mako build --target <triple>` — uses zig cc when available.
Full details: [RELEASE.md](docs/RELEASE.md).

## Hello, Mako

```bash
mako init hello && cd hello
mako run main.mko
```

```mko
fn main() {
    print("hello from mako")
    print_int(fib(10))
}

fn fib(n: int) -> int {
    if n <= 1 { return n }
    return fib(n - 1) + fib(n - 2)
}
```

## Why Mako

**Memory safety without a GC.** Mako uses ownership (`hold`/`share`) and a
move checker to prevent use-after-free and leaks at compile time. Arena
allocators let you scope memory to a request — allocate many, free once:

```mko
arena a {
    let msg = arena_text(a, "hello arena")
    let xs = arena_ints(a, 1000)
    // use msg, xs freely...
}
// everything in `a` is freed here — one call, no GC pause
```

**Concurrency that won't bite you.** `crew` blocks manage concurrent work and
guarantee cleanup. No orphaned threads, no forgotten joins:

```mko
fn main() {
    let ch = chan_new(4)
    crew t {
        let p = t.kick(producer(ch, 5))
        let c = t.kick(consumer(ch))
        let _ = p.join()
        print_int(c.join())
    }
    // all tasks joined and cleaned up here, always
}
```

**Errors are values, not surprises.** `Result` types are enforced — the compiler
rejects code that ignores a `Result`. Wrapping and propagation feel natural:

```mko
fn load_config(path: string) -> Result[int, string] {
    let fd = open_cfg(path)?          // propagate errors with ?
    Ok(fd)
}
```

**Batteries included.** The standard library covers HTTP servers, TLS, WebSocket,
JSON, database drivers (SQLite, Postgres), crypto, compression, regular
expressions, and more. Build a JSON API in one file:

```mko
fn main() {
    let fd = http_bind(8080)
    while true {
        let c = http_accept(fd)
        let path = http_path(c)
        if str_eq(path, "/health") {
            let _ = http_respond_json(c, 200, "{\"ok\":true}")
        }
        let _ = http_close(c)
    }
}
```

**Fast builds, small binaries.** Incremental compilation is on by default.
Release builds use `-O3 -flto`. Benchmarks show performance faster than Go and
competitive with Rust on common workloads.
See [PERFORMANCE.md](docs/PERFORMANCE.md).

## Common Commands

```bash
mako init myapp                  # scaffold a new project
mako run main.mko                # compile and run
mako build main.mko              # compile to native binary
mako build --release main.mko    # optimized release build (-O3 -flto)
mako test examples/testing       # run test suite
mako test -r TestAdd -v          # run specific tests, verbose
mako fmt -w                      # format source files in place
mako lint                        # static analysis
mako check main.mko              # type-check without compiling
mako build --target wasm32-wasip1 main.mko  # compile to WebAssembly
```

## Packages

Projects use `mako.toml` for dependencies and metadata:

```bash
mako init mylib                  # create a new package
mako pkg add helper ../helper    # add a local dependency
mako pkg fetch                   # fetch git dependencies
mako pkg lock                    # pin versions in mako.lock
mako pkg audit                   # check advisories and licenses
```

## Documentation

| | |
|---|---|
| **[The Mako Book](docs/book/)** | Guided tour — install through concurrency, HTTP, WASI |
| [How-to Guides](docs/howto/README.md) | Getting started, HTTP APIs, errors, packages, concurrency |
| [Language Guide](docs/GUIDE.md) | Complete syntax reference with examples |
| [Standard Library](docs/STDLIB.md) | Library catalog and API surface |
| [Security](docs/SECURITY.md) | Memory safety model, ownership, secure defaults |
| [Performance](docs/PERFORMANCE.md) | Benchmarks vs Go and Rust |
| [Status](docs/STATUS.md) | What works, what's left, verified test matrix |
| [Vision](docs/VISION.md) | Where Mako is headed |
| [Roadmap](docs/ROADMAP.md) | Engineering queue and priorities |
| [Release](docs/RELEASE.md) | Packaging, cross-compilation, install targets |
| [Debug](docs/DEBUG.md) | lldb/gdb, `dbg()`, sanitizers |
| [Changelog](CHANGELOG.md) | Release notes |

## Editor Support

**VS Code** — full extension with syntax highlighting, LSP (completions, hover,
go-to-definition, rename), debugging via CodeLLDB, and built-in commands for
build/run/test/format. See [editors/vscode/](editors/vscode/).

The language server runs via `mako lsp` and works with any editor that speaks LSP.

## Testing

```bash
mako test examples/testing              # full suite (130 tests)
mako test examples/testing -r TestAdd -v  # filter by name
mako test --coverage                    # coverage report
```

Test categories: unit, property, fuzz, snapshot, fixture, mock.

Some tests need live services. Set `MAKO_LIVE_TLS=1`, `MAKO_LIVE_NGHTTP2=1`, or
`MAKO_LIVE_QUIC=1` to enable them. The default suite runs clean without these.

## Known Limitations

This is version 0.1.0. A few things are still in progress:

- Unicode property escapes work for common scripts but the full PCRE database isn't there yet
- JPEG encoding uses JFIF headers with a custom payload — not yet readable by all viewers
- Struct field reflection has a schema registry but field values are still string-typed at runtime
- SMTP AUTH works over plaintext; full AUTH over TLS is partial
- Generics syntax (`List<T>`, `Map<K,V>`) is working but may see refinements

The full list lives in [STATUS.md](docs/STATUS.md) under "True hard residuals."

## License

MIT — see [LICENSE](LICENSE).
