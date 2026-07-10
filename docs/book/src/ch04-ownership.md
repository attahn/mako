# 4. Ownership: hold, share, arenas, NLL

Mako has **no mandatory GC**. Everyday values use lexical scope. When you need
stricter move semantics or shared reads, reach for `hold`, `share`, and `arena`.

## `let` and `mut`

```mko
let x = 1
let mut y = 2
y = y + 1
```

## Arenas

Request-scoped bump allocation: everything allocated inside the block is freed
when the arena exits. Ideal for per-request buffers.

```mko
arena a {
    let mut s = make([]int, 3, 8)  // backing store from arena
    s[0] = 10
}
```

Outside an arena, `make` uses the heap as usual.

## `hold` — move on rebind

`hold` bindings move when rebound, passed into calls, or fully read (non-Copy):

```mko
hold let x = 7
hold let y = x
print_int(y)
// print_int(x)  // error: use of moved value `x`
```

Copy types like `int` may be re-read. Field access moves only that path:

```mko
hold let p = Point { x: 1, y: 2 }
let x = p.x
print_int(p.y)   // y still usable
```

## CFG NLL (Done)

The checker joins moves across if/else and match arms, understands
`return`/`break`/`continue` as diverging, and re-checks loops only when a path
can re-enter. Const-bool edges (`if false`) are pruned. See GUIDE §7 and
`examples/hold_*` / `examples/bad/hold_*`.

## `share` — immutable shared borrow (RC seed)

```mko
share let a = share_int(7)
share let b = share_clone(a)
print_int(share_get(a))
share_drop(a)
print_int(share_get(b))
share_drop(b)
```

Rules of thumb:

- `share let` is immutable — no `share let mut`.
- Sharing a local blocks mutating that local while the share is live.
- Shares end at block end, mid-scope after last use (NLL), or explicit `share_drop`.
- Residual: not full RC object graphs — prefer arenas + hold for most servers.

## Idioms

| Situation | Prefer |
|-----------|--------|
| Per-request buffers | `arena` + `make` |
| Unique ownership / move | `hold` |
| Short shared read of an int | `share_int` |
| Long-lived graphs | redesign with actors/channels, or wait for deeper RC |

How-to: [howto/06-memory.md](../../howto/06-memory.md) · security contract:
[SECURITY.md](../../SECURITY.md).

Next: [Errors & Result](ch05-errors.md).
