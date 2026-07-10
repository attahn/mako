# 6. Concurrency: crew, channels, cancel, actors

Mako concurrency is **structured**. Jobs cannot outlive their `crew`. That is
the main footgun killer versus fire-and-forget threads.

## Crew, kick, join

```mko
fn producer(ch: chan[int], n: int) -> int {
    let mut i = 0
    while i < n {
        let _ = ch.send(i)
        i = i + 1
    }
    ch.close()
    0
}

fn consumer(ch: chan[int]) -> int {
    let mut sum = 0
    for v in range ch {
        sum = sum + v
    }
    sum
}

fn main() {
    let ch = chan_new(4)
    crew t {
        let p = t.kick(producer(ch, 5))
        let c = t.kick(consumer(ch))
        let _ = p.join()
        print_int(c.join())
    }
}
```

On crew exit, cancel **joins** kicked work — no orphan threads
([SECURITY.md](../../SECURITY.md)).

## Cancel

```mko
crew c {
    let a = c.kick(work(4))
    c.cancel()
    assert(c.cancelled())
    // further kicks observe cancel policy
}
```

## Fan — data parallel

```mko
fn main() {
    let xs = [1, 2, 3, 4]
    let ys = fan(xs, |x| x * x)
    for v in ys {
        print_int(v)
    }
}
```

## Channels and `select`

```mko
let ch = chan_new(4)
let _ = ch.send(1)
let v = ch.recv()
ch.close()
```

```mko
select timeout 30 {
    a => { print("got a") }
    b => { print("got b") }
    default => { print("default ok") }
}
```

Ready-arm value: `chan_select_value()`. Fairness is round-robin when many arms
are ready. Helpers: `chan_select2` / `3` / `4`.

## Actors

Actors desugar to a mailbox + crew loop — great for session state:

```mko
actor Session {
    receive Invite { print("invite") }
    receive Timer { print("tick") }
    receive Bye { print("bye") }
}

fn main() {
    let session = Session_spawn()
    crew t {
        let loopj = t.kick(Session_loop(session))
        let _ = Session_send(session, Session_Invite())
        let _ = Session_send(session, Session_Bye())
        print_int(loopj.join())
    }
}
```

`Bye` / `Stop` end the loop by convention. See `examples/actor.mko`.

## Async I/O note

Mako prefers **colorless** I/O with crews rather than colored `async`/`await`
everywhere. See [ASYNC.md](../../ASYNC.md) for the design stance.

How-to: [howto/05-concurrency.md](../../howto/05-concurrency.md).

Next: [Standard Library](ch07-stdlib.md).
