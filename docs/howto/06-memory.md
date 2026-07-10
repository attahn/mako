# Memory: arena, hold, share

## Arenas

Request-scoped bump allocation — free once at end of scope:

```mko
arena a {
    let s = arena_text(a, "body")
    // …
} // region freed
```

## hold (move)

```mko
hold let s = "hi"
let t = s          // moves
// print(s)        // error: use of moved value
```

CFG NLL + labeled `break`/`continue` (`label: while` / `break label`).

## share

Reference-counted / shared binding seed (`share_int` RC). Prefer `hold` when unique ownership works.

Safety: [SECURITY.md](../SECURITY.md) · NLL examples under `examples/hold_*.mko`.
