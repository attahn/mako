# Third-party integration scaffold

## nghttp2 (linked)

GET + POST + 2-stream mux (`MAKO_LIVE_NGHTTP2=1`).

## quiche (handshake + HTTP/3 GET)

| Builtin | Role |
|---------|------|
| `quiche_available` / `quiche_version` | C ABI |
| `quiche_handshake(...)` | UDP → `quic:ok;h3` |
| `quiche_h3_get(host, port, path, sni, verify)` | H3 GET → `h3:<status>;<body>` |
| `quiche_h3_post(host, port, path, body, sni, verify)` | H3 POST via send_body (stock: `405`) |
| `quiche_h3_get_two(host, port, path1, path2, sni, verify)` | two overlapping GETs → `h3:…\|…` |
| `quiche_start_server` / `quiche_stop_server` | stock server helper |

```bash
MAKO_LIVE_QUIC=1 cargo run --quiet -- test examples/testing/quiche_link_test.mko -v
```

Verified: GET, POST (405), mux GET `/` + `/health`.

**Still missing:** H3 push; POST echo; packaging. Vision **~99%** — further seeds are diminishing returns.