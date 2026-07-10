# Concurrency

## Crew

Jobs cannot outlive their `crew` — all kicked work is joined on scope exit.

```mko
crew t {
    let j = t.kick(work())
    print_int(j.join())
}
```

## Channels and select

```mko
let ch = chan_new(4)
crew t {
    let _ = t.kick(producer(ch))
    select {
        ch => { let v = chan_recv(ch); print_int(v) }
        timeout 100 => { print("timeout") }
        default => { print("none") }
    }
}
```

## Fan

```mko
let ys = fan(xs, |x| x * x)
```

No mandatory GC — latency stays predictable. See [PERFORMANCE.md](../PERFORMANCE.md).
