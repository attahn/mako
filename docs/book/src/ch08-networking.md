# 8. Networking & HTTP

Beachhead: **HTTP/1.1**, HTTPS (OpenSSL when linked), HTTP/2 TLS server,
gRPC/H3-client pieces, and a small HTTP library with a typed `Request`. This is
a systems beachhead — not a full web framework.

## Minimal HTTP/1.1 server

```mko
fn main() {
    let fd = http_bind(18100)
    if fd < 0 {
        print("bind failed")
        return
    }
    let c = http_accept(fd)
    if c >= 0 {
        let p = http_path(c)
        if str_eq(p, "/health") {
            let _ = http_respond_ct(c, 200, "application/json", "{\"ok\":true}\n")
        } else {
            let _ = http_respond(c, 200, "hello from mako\n")
        }
        let _ = http_close(c)
    }
    let _ = http_close_listener(fd)
}
```

Full loop: `examples/http_server.mko`. Smoke: `./scripts/http-server-smoke.sh`.

## TCP

```mko
let fd = tcp_listen(18082)
let c = tcp_accept(fd)
let _ = tcp_write(c, "hi\n")
let _ = tcp_close(c)
let peer = tcp_connect("127.0.0.1", 18082)
```

## HTTPS / HTTP/2

- HTTPS listener wrap when OpenSSL is linked — `examples/https_server.mko`
- HTTP/2 TLS + ALPN `h2` — `examples/h2_server.mko`
- Live optional tests: `MAKO_LIVE_TLS=1`, `MAKO_LIVE_NGHTTP2=1`, `MAKO_LIVE_QUIC=1`

## HTTP library & `Request`

Higher-level helpers and a typed request surface live under the HTTP library
(see `examples/http_lib/` and [howto/02-http-apis.md](../../howto/02-http-apis.md)).
Prefer validating headers with `http_header_ok` to reject CR/LF injection.

## Client notes

Client GET/POST helpers and httptest utilities ship in std (`testing/httptest`,
net/http builtins). Prefer parameterized URLs and explicit timeouts via
`context` helpers where available.

## Security checklist

| Concern | Practice |
|---------|----------|
| Header injection | `http_header_ok` |
| Secrets | `secret_from_str` / `secret_drop` |
| TLS | Use linked OpenSSL paths; don’t ship with verify disabled in prod |
| Crews | Serve under a crew so cancel joins workers |

Details: [STDLIB.md](../../STDLIB.md) · [GUIDE.md](../../GUIDE.md) §11 ·
[TLS_LIVE.md](../../TLS_LIVE.md).

Next: [Data](ch09-data.md).
