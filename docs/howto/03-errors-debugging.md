# Errors and debugging

## Result and `?`

```mko
fn parse_positive(n: int) -> Result[int, string] {
    if n < 0 { return error("negative") }
    return Ok(n)
}

fn main() {
    let x = parse_positive(3)?
    print_int(x)
}
```

Unused `Result` is a **compile error**. Prefer `?`, `match`, or `let _ = …`.

Helpers: `error` / `errorf` / `wrap_err` / `error_is` / `error_string`.

## dbg

```mko
let x = dbg(3)           // stderr: [dbg] file:line: …
let s = dbg_str("hi")
```

## Native debug

Default builds: clang `-O0 -g`.

```bash
mako build main.mko -o app
lldb ./app
mako build --sanitize=address main.mko -o app_asan
```

Details: [DEBUG.md](../DEBUG.md).
