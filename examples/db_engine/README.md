# Mini embedded KV engine

Append-only log with `PUT` / `DEL` records, replayed into a map.

```bash
mako run examples/db_engine/main.mko   # prints 3 then empty
mako test examples/testing/db_engine_test.mko
```

Not a production DB — a systems/storage sketch showing Mako can build engines
(file I/O, parsers, maps) without a GC.
