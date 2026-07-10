/* WebSocket — RFC6455 upgrade, server/client frames, ping/pong, close. */
#ifndef MAKO_WS_H
#define MAKO_WS_H

#include "mako_http.h"
#include <stdint.h>

#if defined(__APPLE__)
#include <CommonCrypto/CommonDigest.h>
#define MAKO_WS_SHA1_CC 1
#elif defined(MAKO_HAS_OPENSSL) || defined(MAKO_USE_OPENSSL)
#include <openssl/sha.h>
#define MAKO_WS_SHA1_SSL 1
#endif

#ifdef __cplusplus
extern "C" {
#endif

static inline void mako_ws_sha1(const char *data, size_t n, unsigned char out[20]) {
#if defined(MAKO_WS_SHA1_CC)
    CC_SHA1(data, (CC_LONG)n, out);
#elif defined(MAKO_WS_SHA1_SSL)
    SHA1((const unsigned char *)data, n, out);
#else
    uint32_t h0 = 0x67452301u, h1 = 0xEFCDAB89u, h2 = 0x98BADCFEu;
    uint32_t h3 = 0x10325476u, h4 = 0xC3D2E1F0u;
    uint64_t bit_len = (uint64_t)n * 8u;
    size_t total = n + 1 + 8;
    size_t padded = ((total + 63) / 64) * 64;
    unsigned char *msg = (unsigned char *)calloc(1, padded);
    if (!msg) mako_abort("ws sha1 OOM");
    if (n) memcpy(msg, data, n);
    msg[n] = 0x80;
    for (int i = 0; i < 8; i++) msg[padded - 1 - i] = (unsigned char)(bit_len >> (8 * i));
    for (size_t off = 0; off < padded; off += 64) {
        uint32_t w[80];
        for (int i = 0; i < 16; i++) {
            size_t j = off + (size_t)i * 4;
            w[i] = ((uint32_t)msg[j] << 24) | ((uint32_t)msg[j + 1] << 16)
                | ((uint32_t)msg[j + 2] << 8) | (uint32_t)msg[j + 3];
        }
        for (int i = 16; i < 80; i++) {
            uint32_t v = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
            w[i] = (v << 1) | (v >> 31);
        }
        uint32_t a = h0, b = h1, c = h2, d = h3, e = h4;
        for (int i = 0; i < 80; i++) {
            uint32_t f, k;
            if (i < 20) {
                f = (b & c) | ((~b) & d);
                k = 0x5A827999u;
            } else if (i < 40) {
                f = b ^ c ^ d;
                k = 0x6ED9EBA1u;
            } else if (i < 60) {
                f = (b & c) | (b & d) | (c & d);
                k = 0x8F1BBCDCu;
            } else {
                f = b ^ c ^ d;
                k = 0xCA62C1D6u;
            }
            uint32_t temp = ((a << 5) | (a >> 27)) + f + e + k + w[i];
            e = d;
            d = c;
            c = (b << 30) | (b >> 2);
            b = a;
            a = temp;
        }
        h0 += a; h1 += b; h2 += c; h3 += d; h4 += e;
    }
    free(msg);
    uint32_t hs[5] = {h0, h1, h2, h3, h4};
    for (int i = 0; i < 5; i++) {
        out[i * 4 + 0] = (unsigned char)(hs[i] >> 24);
        out[i * 4 + 1] = (unsigned char)(hs[i] >> 16);
        out[i * 4 + 2] = (unsigned char)(hs[i] >> 8);
        out[i * 4 + 3] = (unsigned char)hs[i];
    }
#endif
}

static inline void mako_ws_b64(const unsigned char *in, size_t n, char *out, size_t outcap) {
    static const char *T =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t j = 0;
    for (size_t i = 0; i < n && j + 4 < outcap; i += 3) {
        unsigned v = (unsigned)in[i] << 16;
        if (i + 1 < n) v |= (unsigned)in[i + 1] << 8;
        if (i + 2 < n) v |= (unsigned)in[i + 2];
        out[j++] = T[(v >> 18) & 63];
        out[j++] = T[(v >> 12) & 63];
        out[j++] = (i + 1 < n) ? T[(v >> 6) & 63] : '=';
        out[j++] = (i + 2 < n) ? T[v & 63] : '=';
    }
    if (j < outcap) out[j] = 0;
}

static inline int mako_ws_header_value(const char *req, const char *name, char *out, size_t outcap) {
    size_t nlen = strlen(name);
    const char *p = req;
    while (p && *p) {
        const char *line = p;
        const char *nl = strstr(p, "\r\n");
        size_t llen = nl ? (size_t)(nl - p) : strlen(p);
        if (llen >= nlen + 1) {
            int match = 1;
            for (size_t i = 0; i < nlen; i++) {
                char a = line[i], b = name[i];
                if (a >= 'A' && a <= 'Z') a = (char)(a - 'A' + 'a');
                if (b >= 'A' && b <= 'Z') b = (char)(b - 'A' + 'a');
                if (a != b) {
                    match = 0;
                    break;
                }
            }
            if (match && line[nlen] == ':') {
                const char *v = line + nlen + 1;
                while (*v == ' ') v++;
                size_t vlen = (size_t)((line + llen) - v);
                if (vlen >= outcap) vlen = outcap - 1;
                memcpy(out, v, vlen);
                out[vlen] = 0;
                return 1;
            }
        }
        if (!nl) break;
        p = nl + 2;
        if (p[0] == '\r' && p[1] == '\n') break;
    }
    return 0;
}

static inline int mako_ws_send_text(int fd, const char *data, size_t len) {
    unsigned char hdr[10];
    size_t hlen = 0;
    hdr[0] = 0x81; /* FIN + text */
    if (len < 126) {
        hdr[1] = (unsigned char)len;
        hlen = 2;
    } else if (len < 65536) {
        hdr[1] = 126;
        hdr[2] = (unsigned char)((len >> 8) & 0xff);
        hdr[3] = (unsigned char)(len & 0xff);
        hlen = 4;
    } else {
        return -1; /* oversized for this Partial */
    }
    if (send(fd, hdr, hlen, 0) < 0) return -1;
    if (len && send(fd, data, len, 0) < 0) return -1;
    return 0;
}

static inline int mako_ws_send_binary(int fd, const char *data, size_t len) {
    unsigned char hdr[10];
    size_t hlen = 0;
    hdr[0] = 0x82; /* FIN + binary */
    if (len < 126) {
        hdr[1] = (unsigned char)len;
        hlen = 2;
    } else if (len < 65536) {
        hdr[1] = 126;
        hdr[2] = (unsigned char)((len >> 8) & 0xff);
        hdr[3] = (unsigned char)(len & 0xff);
        hlen = 4;
    } else {
        return -1;
    }
    if (send(fd, hdr, hlen, 0) < 0) return -1;
    if (len && send(fd, data, len, 0) < 0) return -1;
    return 0;
}

static inline int mako_ws_send_ping(int fd, const char *data, size_t len) {
    if (len > 125) return -1;
    unsigned char hdr[2];
    hdr[0] = 0x89;
    hdr[1] = (unsigned char)len;
    if (send(fd, hdr, 2, 0) < 0) return -1;
    if (len && send(fd, data, len, 0) < 0) return -1;
    return 0;
}

/* Read one client frame (masked). Returns payload length, or:
 * -2 close, -3 ping handled (pong sent), -4 pong ignored, -1 error.
 * Sets mako_ws_last_opcode to 1 (text) or 2 (binary) on data frames. */
static int mako_ws_last_opcode = 0;

static inline int64_t mako_ws_recv_text(int fd, char *buf, size_t cap) {
    unsigned char h[2];
    if (recv(fd, h, 2, MSG_WAITALL) != 2) return -1;
    int opcode = h[0] & 0x0f;
    int masked = (h[1] & 0x80) != 0;
    uint64_t plen = h[1] & 0x7f;
    if (plen == 126) {
        unsigned char e[2];
        if (recv(fd, e, 2, MSG_WAITALL) != 2) return -1;
        plen = ((uint64_t)e[0] << 8) | e[1];
    } else if (plen == 127) {
        return -1;
    }
    unsigned char mask[4] = {0, 0, 0, 0};
    if (masked) {
        if (recv(fd, mask, 4, MSG_WAITALL) != 4) return -1;
    }
    if (opcode == 8) return -2;
    if (opcode == 9) {
        if (plen >= cap) return -1;
        size_t got = 0;
        while (got < plen) {
            ssize_t n = recv(fd, buf + got, (size_t)(plen - got), 0);
            if (n <= 0) return -1;
            got += (size_t)n;
        }
        if (masked) {
            for (size_t i = 0; i < plen; i++) buf[i] = (char)(buf[i] ^ mask[i % 4]);
        }
        unsigned char ph[2];
        ph[0] = 0x8A;
        ph[1] = (unsigned char)plen;
        if (send(fd, ph, 2, 0) < 0) return -1;
        if (plen && send(fd, buf, (size_t)plen, 0) < 0) return -1;
        return -3;
    }
    if (opcode == 10) {
        uint64_t left = plen;
        char dump[256];
        while (left) {
            size_t chunk = left > sizeof(dump) ? sizeof(dump) : (size_t)left;
            ssize_t n = recv(fd, dump, chunk, 0);
            if (n <= 0) break;
            left -= (uint64_t)n;
        }
        return -4;
    }
    if (opcode != 1 && opcode != 2) {
        uint64_t left = plen;
        char dump[256];
        while (left) {
            size_t chunk = left > sizeof(dump) ? sizeof(dump) : (size_t)left;
            ssize_t n = recv(fd, dump, chunk, 0);
            if (n <= 0) break;
            left -= (uint64_t)n;
        }
        return -1;
    }
    mako_ws_last_opcode = opcode;
    if (plen >= cap) return -1;
    size_t got = 0;
    while (got < plen) {
        ssize_t n = recv(fd, buf + got, (size_t)(plen - got), 0);
        if (n <= 0) return -1;
        got += (size_t)n;
    }
    if (masked) {
        for (size_t i = 0; i < plen; i++) buf[i] = (char)(buf[i] ^ mask[i % 4]);
    }
    buf[plen] = 0;
    return (int64_t)plen;
}

static inline int mako_ws_upgrade(int fd, const char *req) {
    char key[128];
    if (!mako_ws_header_value(req, "Sec-WebSocket-Key", key, sizeof(key))) {
        fprintf(stderr, "ws: missing Sec-WebSocket-Key\n");
        return -1;
    }
    char concat[256];
    snprintf(
        concat,
        sizeof(concat),
        "%s258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
        key
    );
    unsigned char dig[20];
    mako_ws_sha1(concat, strlen(concat), dig);
    char accept[64];
    mako_ws_b64(dig, 20, accept, sizeof(accept));
    char resp[256];
    int n = snprintf(
        resp,
        sizeof(resp),
        "HTTP/1.1 101 Switching Protocols\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        "Sec-WebSocket-Accept: %s\r\n"
        "\r\n",
        accept
    );
    if (n <= 0 || send(fd, resp, (size_t)n, 0) < 0) return -1;
    return 0;
}

static inline MakoString mako_ws_accept_key(MakoString key) {
    char k[160];
    size_t n = key.len < sizeof(k) - 1 ? key.len : sizeof(k) - 1;
    if (n) memcpy(k, key.data ? key.data : "", n);
    k[n] = 0;
    char concat[256];
    snprintf(concat, sizeof(concat), "%s258EAFA5-E914-47DA-95CA-C5AB0DC85B11", k);
    unsigned char dig[20];
    mako_ws_sha1(concat, strlen(concat), dig);
    char accept[64];
    mako_ws_b64(dig, 20, accept, sizeof(accept));
    return mako_str_from_cstr(accept);
}

static inline int64_t mako_ws_upgrade_request_ok(MakoString req) {
    const char *r = req.data ? req.data : "";
    char upgrade[64], conn[128], key[128], version[32];
    if (!mako_ws_header_value(r, "Upgrade", upgrade, sizeof(upgrade))) return 0;
    if (!mako_ws_header_value(r, "Connection", conn, sizeof(conn))) return 0;
    if (!mako_ws_header_value(r, "Sec-WebSocket-Key", key, sizeof(key))) return 0;
    if (!mako_ws_header_value(r, "Sec-WebSocket-Version", version, sizeof(version))) return 0;
    for (char *p = upgrade; *p; p++) if (*p >= 'A' && *p <= 'Z') *p = (char)(*p + 32);
    for (char *p = conn; *p; p++) if (*p >= 'A' && *p <= 'Z') *p = (char)(*p + 32);
    return strstr(upgrade, "websocket") && strstr(conn, "upgrade") && strcmp(version, "13") == 0 ? 1 : 0;
}

static inline MakoString mako_ws_client_request(MakoString host, MakoString path, MakoString key) {
    size_t cap = host.len + path.len + key.len + 256;
    char *d = (char *)malloc(cap);
    if (!d) mako_abort("ws client request OOM");
    int n = snprintf(
        d, cap,
        "GET %.*s HTTP/1.1\r\n"
        "Host: %.*s\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        "Sec-WebSocket-Key: %.*s\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n",
        (int)path.len, path.data ? path.data : "/",
        (int)host.len, host.data ? host.data : "",
        (int)key.len, key.data ? key.data : ""
    );
    if (n < 0) n = 0;
    return (MakoString){d, (size_t)n};
}

static inline int64_t mako_ws_client_accept_ok(MakoString key, MakoString response) {
    char accept[128];
    if (!mako_ws_header_value(response.data ? response.data : "", "Sec-WebSocket-Accept", accept, sizeof(accept))) return 0;
    MakoString expected = mako_ws_accept_key(key);
    int ok = expected.len == strlen(accept) && memcmp(expected.data, accept, expected.len) == 0;
    return ok ? 1 : 0;
}

static inline int64_t mako_ws_accept(int64_t listen_fd) {
    if (listen_fd < 0) return -1;
    int cfd = accept((int)listen_fd, NULL, NULL);
    if (cfd < 0) return -1;
    char req[8192];
    ssize_t n = recv(cfd, req, sizeof(req) - 1, 0);
    if (n < 0) n = 0;
    req[n] = 0;
    if (mako_ws_upgrade(cfd, req) != 0) {
        mako_sock_close((mako_sock_t)cfd);
        return -1;
    }
    return cfd;
}

static inline MakoString mako_ws_recv(int64_t fd, int64_t max_bytes) {
    if (fd < 0) return mako_str_from_cstr("");
    if (max_bytes <= 0) max_bytes = 4096;
    if (max_bytes > 65535) max_bytes = 65535;
    char *buf = (char *)malloc((size_t)max_bytes + 1);
    if (!buf) mako_abort("ws recv OOM");
    int64_t n;
    do {
        n = mako_ws_recv_text((int)fd, buf, (size_t)max_bytes + 1);
    } while (n == -3 || n == -4);
    if (n < 0) {
        free(buf);
        return mako_str_from_cstr("");
    }
    buf[n] = 0;
    return (MakoString){buf, (size_t)n};
}

static inline int64_t mako_ws_last_frame_opcode(void) {
    return (int64_t)mako_ws_last_opcode;
}

static inline int64_t mako_ws_send_text_msg(int64_t fd, MakoString msg) {
    if (fd < 0) return -1;
    return mako_ws_send_text((int)fd, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_send_binary_msg(int64_t fd, MakoString msg) {
    if (fd < 0) return -1;
    return mako_ws_send_binary((int)fd, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_send_ping_msg(int64_t fd, MakoString msg) {
    if (fd < 0) return -1;
    return mako_ws_send_ping((int)fd, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_send_close(int64_t fd, int64_t code, MakoString reason) {
    if (fd < 0 || reason.len > 123) return -1;
    unsigned char hdr[2];
    unsigned char payload[125];
    size_t plen = 0;
    if (code > 0) {
        payload[plen++] = (unsigned char)((code >> 8) & 0xff);
        payload[plen++] = (unsigned char)(code & 0xff);
    }
    if (reason.len) {
        memcpy(payload + plen, reason.data ? reason.data : "", reason.len);
        plen += reason.len;
    }
    hdr[0] = 0x88;
    hdr[1] = (unsigned char)plen;
    if (send((int)fd, hdr, 2, 0) < 0) return -1;
    if (plen && send((int)fd, (const char *)payload, plen, 0) < 0) return -1;
    return 0;
}

static inline int64_t mako_ws_close(int64_t fd) {
    if (fd < 0) return 0;
    return mako_sock_close((mako_sock_t)fd) == 0 ? 1 : 0;
}

static inline int mako_ws_send_client_frame(int fd, int opcode, const char *data, size_t len) {
    if (len >= 65536) return -1;
    unsigned char hdr[14];
    size_t hlen = 0;
    hdr[0] = (unsigned char)(0x80 | (opcode & 0x0f));
    if (len < 126) {
        hdr[1] = (unsigned char)(0x80 | len);
        hlen = 2;
    } else {
        hdr[1] = 0x80 | 126;
        hdr[2] = (unsigned char)((len >> 8) & 0xff);
        hdr[3] = (unsigned char)(len & 0xff);
        hlen = 4;
    }
    unsigned char mask[4];
    uint32_t seed = (uint32_t)mako_now_ms() ^ (uint32_t)(uintptr_t)data ^ (uint32_t)len;
    for (int i = 0; i < 4; i++) {
        seed = seed * 1664525u + 1013904223u;
        mask[i] = (unsigned char)(seed >> 24);
        hdr[hlen++] = mask[i];
    }
    if (send(fd, hdr, hlen, 0) < 0) return -1;
    if (len) {
        char *m = (char *)malloc(len);
        if (!m) mako_abort("ws client frame OOM");
        for (size_t i = 0; i < len; i++) m[i] = (char)((data ? data[i] : 0) ^ mask[i % 4]);
        int rc = send(fd, m, len, 0) < 0 ? -1 : 0;
        free(m);
        return rc;
    }
    return 0;
}

static inline int64_t mako_ws_client_send_text(int64_t fd, MakoString msg) {
    if (fd < 0) return -1;
    return mako_ws_send_client_frame((int)fd, 1, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_client_send_binary(int64_t fd, MakoString msg) {
    if (fd < 0) return -1;
    return mako_ws_send_client_frame((int)fd, 2, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_client_send_ping(int64_t fd, MakoString msg) {
    if (fd < 0 || msg.len > 125) return -1;
    return mako_ws_send_client_frame((int)fd, 9, msg.data ? msg.data : "", msg.len);
}

static inline int64_t mako_ws_client_connect(MakoString host, int64_t port, MakoString path, MakoString key) {
    mako_net_init();
    char hbuf[256], pbuf[32];
    size_t hlen = host.len < sizeof(hbuf) - 1 ? host.len : sizeof(hbuf) - 1;
    if (hlen) memcpy(hbuf, host.data ? host.data : "", hlen);
    hbuf[hlen] = 0;
    snprintf(pbuf, sizeof(pbuf), "%lld", (long long)port);
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(hbuf, pbuf, &hints, &res) != 0 || !res) return -1;
    int fd = -1;
    for (struct addrinfo *it = res; it; it = it->ai_next) {
        fd = (int)socket(it->ai_family, it->ai_socktype, it->ai_protocol);
        if (fd < 0) continue;
        if (connect(fd, it->ai_addr, it->ai_addrlen) == 0) break;
        mako_sock_close((mako_sock_t)fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0) return -1;
    MakoString req = mako_ws_client_request(host, path, key);
    if (send(fd, req.data, req.len, 0) < 0) {
        mako_sock_close((mako_sock_t)fd);
        return -1;
    }
    char resp[2048];
    ssize_t n = recv(fd, resp, sizeof(resp) - 1, 0);
    if (n <= 0) {
        mako_sock_close((mako_sock_t)fd);
        return -1;
    }
    resp[n] = 0;
    MakoString rs = {resp, (size_t)n};
    if (!mako_ws_client_accept_ok(key, rs)) {
        mako_sock_close((mako_sock_t)fd);
        return -1;
    }
    return fd;
}

/* Accept one WS client, echo one text frame, close. Returns 0 on success. */
static inline int64_t mako_ws_echo_once(int64_t port) {
    int fd = mako_http_listen_fd(port);
    if (fd < 0) {
        fprintf(stderr, "error: ws_echo_once bind(:%lld) failed\n", (long long)port);
        return 1;
    }
    fprintf(stderr, "mako ws_echo_once on :%lld\n", (long long)port);
    int cfd = accept(fd, NULL, NULL);
    if (cfd < 0) {
        close(fd);
        return 1;
    }
    char req[8192];
    ssize_t n = recv(cfd, req, sizeof(req) - 1, 0);
    if (n < 0) n = 0;
    req[n] = 0;
    if (mako_ws_upgrade(cfd, req) != 0) {
        close(cfd);
        close(fd);
        return 1;
    }
    char payload[4096];
    int64_t plen;
    do {
        plen = mako_ws_recv_text(cfd, payload, sizeof(payload));
    } while (plen == -3 || plen == -4);
    if (plen >= 0) {
        if (mako_ws_last_opcode == 2)
            (void)mako_ws_send_binary(cfd, payload, (size_t)plen);
        else
            (void)mako_ws_send_text(cfd, payload, (size_t)plen);
    }
    /* close frame */
    unsigned char close_fr[2] = {0x88, 0x00};
    (void)send(cfd, close_fr, 2, 0);
    close(cfd);
    close(fd);
    return plen >= 0 ? 0 : 1;
}

/* Forever echo loop (blocks). */
static inline int64_t mako_ws_echo(int64_t port) {
    int fd = mako_http_listen_fd(port);
    if (fd < 0) return 1;
    fprintf(stderr, "mako ws_echo on :%lld\n", (long long)port);
    for (;;) {
        int cfd = accept(fd, NULL, NULL);
        if (cfd < 0) continue;
        char req[8192];
        ssize_t n = recv(cfd, req, sizeof(req) - 1, 0);
        if (n < 0) n = 0;
        req[n] = 0;
        if (mako_ws_upgrade(cfd, req) != 0) {
            close(cfd);
            continue;
        }
        for (;;) {
            char payload[4096];
            int64_t plen = mako_ws_recv_text(cfd, payload, sizeof(payload));
            if (plen == -2 || plen < 0) break;
            if (mako_ws_send_text(cfd, payload, (size_t)plen) < 0) break;
        }
        close(cfd);
    }
}

/* Legacy stub name → real once. */
static inline int64_t mako_ws_echo_stub(int64_t port) {
    return mako_ws_echo_once(port);
}

#ifdef __cplusplus
}
#endif

#endif /* MAKO_WS_H */
