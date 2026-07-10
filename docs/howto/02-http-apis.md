# HTTP APIs (library)

Mako’s HTTP surface is a coherent **library of builtins** (implemented in
`runtime/mako_http.h`), not a separate crate. TLS/H2 are extensions (`tls_*`,
`nghttp2_*`) — see GUIDE § Networking.

## Server

```mko
let fd = http_bind(8080)           // listen; <0 on failure
let c = http_accept(fd)            // connection id
let method = http_method(c)
let path = http_path(c)
let body = http_body(c)
let ua = http_header(c, "User-Agent")
let _ = http_respond_json(c, 200, "{\"ok\":true}\n")
// or: http_respond(c, 200, "text")
// or: http_respond_ct(c, 200, "text/html", html)
let _ = http_close(c)
let _ = http_close_listener(fd)
```

Keep-alive: `http_next(c)` / `http_keepalive(c)` for multi-request connections.

### Typed `HttpRequest`

```mko
let req = http_request_parse("GET /x HTTP/1.1\r\n\r\n")
print(http_request_method(req))
print(http_request_path(req))
// From an accepted connection:
// let req = http_request_from_conn(c)
// After TLS decrypt, parse the plaintext the same way.
```

Examples: `examples/http_server.mko`, `examples/api_backend/`, `examples/http_lib/`
(including `request_type.mko`), `mako init --backend`.

## Client (plain HTTP)

```mko
let body = http_get("http://127.0.0.1:8080/health")
let st = http_last_status()          // e.g. 200
let ct = http_last_header("Content-Type")

let echo = http_post("http://127.0.0.1:8080/echo", "hi")
let raw = http_request("PUT", "http://host/path", body, 5000)  // timeout ms
let g = http_get_timeout(url, 3000)
let p = http_post_timeout(url, body, 3000)
```

- Scheme must be `http://` (use `tls_get` / `nghttp2_get` for HTTPS/H2).
- `http_last_status` / `http_last_header` reflect the **most recent** client call.
- Timeout ≤ 0 means no socket timeout.

Smoke: `./scripts/http-lib-smoke.sh` · tests: `examples/testing/http_lib_test.mko`.

## HTTPS / H2

| Need | API |
|------|-----|
| HTTPS server | `tls_serve_n` — `examples/https_server.mko` |
| H2 server | `tls_serve_h2_routes` — `examples/h2_server.mko` |
| HTTPS client | `tls_get` / `tls_post` |
| H2 client | `nghttp2_get` / `tls_h2_get` |

Full tables: [STDLIB.md](../STDLIB.md) · [GUIDE.md](../GUIDE.md) § Networking.
