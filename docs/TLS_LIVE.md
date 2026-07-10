# Live TLS / HTTP/2 / nghttp2 (OpenSSL)

Requires Homebrew OpenSSL (`MAKO_HAS_OPENSSL`). nghttp2 client requires
`libnghttp2` (`MAKO_HAS_NGHTTP2` — auto-detected via brew/pkg-config).

## Opt-in live tests

```bash
# Default suite: live tests early-return (still PASS)
cargo run --quiet -- test examples/testing

# OpenSSL hand-rolled probes
MAKO_LIVE_TLS=1 cargo run --quiet -- test examples/testing/tls_live_test.mko -v

# Real libnghttp2 client (also enabled when MAKO_LIVE_TLS=1 if lib linked)
MAKO_LIVE_NGHTTP2=1 cargo run --quiet -- test examples/testing/tls_live_test.mko -r Nghttp2 -v
```

| Builtin | Role |
|---------|------|
| `nghttp2_available()` | `1` if linked |
| `nghttp2_get(host, port, path, ca)` | session GET → `nghttp2:<status>;<body>` |
| `nghttp2_post(host, port, path, body, ca)` | session POST with body via data provider |
| `nghttp2_get_two(host, port, path1, path2, ca)` | two overlapping GETs on one session → `nghttp2:<st1>;<b1>\|<st2>;<b2>` |
| Prior `tls_*` / `tls_h2_*` / `tls_grpc_*` | OpenSSL hand probes |

Live nghttp2 tests: `TestNghttp2LiveGet`, `TestNghttp2LiveGetPath`, `TestNghttp2LivePost`, `TestNghttp2LiveMux`.

## Quiche / QUIC / HTTP/3 (opt-in)

```bash
MAKO_LIVE_QUIC=1 cargo run --quiet -- test examples/testing/quiche_link_test.mko -v
```

| Builtin | Role |
|---------|------|
| `quiche_available` / `quiche_version` | C ABI |
| `quiche_handshake(host, port, sni, verify)` | UDP → `quic:ok;h3` |
| `quiche_h3_get(host, port, path, sni, verify)` | HTTP/3 GET → `h3:<status>;<body>` |
| `quiche_h3_post(host, port, path, body, sni, verify)` | HTTP/3 POST → status/body (stock: `405`) |
| `quiche_h3_get_two(host, port, path1, path2, sni, verify)` | two H3 GETs on one conn |
| `quiche_start_server` / `quiche_stop_server` | stock `quiche-server` helper |

Needs `runtime/third_party/quiche/bin/quiche-server`. Soft-skips without `MAKO_LIVE_QUIC=1`.

## Third-party

See `runtime/third_party/README.md` — nghttp2 + quiche H3 GET/POST/mux. Further seeds ≫ packaging.
