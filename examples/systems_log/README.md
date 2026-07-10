# Systems append log

Small durable event log: `append_file` + arena-scoped read + `hold` for paths.

```bash
mako run examples/systems_log/main.mko   # prints 3
```
