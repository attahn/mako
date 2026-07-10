# Quiche FFI + HTTP/3 mux (2026-07-09)

## Verified

| Builtin | Result |
|---------|--------|
| `quiche_h3_get` | `h3:200;mako-quic-ok` |
| `quiche_h3_post` | `h3:405;` (stock GET-only app) |
| **`quiche_h3_get_two(host, port, path1, path2, sni, verify)`** | `h3:200;mako-quic-ok\|200;health` |

Mux submits two `quiche_h3_send_request` calls before pumping UDP; responses keyed by stream id.

```bash
MAKO_LIVE_QUIC=1 cargo run --quiet -- test examples/testing/quiche_link_test.mko -v
```

## Milestone note

Client-side TLS/h2/h3 **seeds are sufficient at ~99%**. Next highest leverage is packaging/release and product APIs, not more one-off FFI probes (push, echo-POST servers, etc.).
