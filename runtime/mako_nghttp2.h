/* nghttp2 client integration — enabled when MAKO_HAS_NGHTTP2 + OpenSSL.
 * Live h2 GET/POST/mux over TLS using libnghttp2 session APIs. */
#ifndef MAKO_NGHTTP2_H
#define MAKO_NGHTTP2_H

#include "mako_tls.h"

#if defined(MAKO_HAS_NGHTTP2) && defined(MAKO_TLS_REAL)
#include <nghttp2/nghttp2.h>

#define MAKO_NG_MAX_STREAMS 2

typedef struct {
    int32_t stream_id;
    char body[4096];
    size_t body_len;
    int32_t status;
    int done;
} MakoNgStream;

typedef struct {
    SSL *ssl;
    int fd;
    int want_io;
    /* Request body for POST (read by data provider). */
    const uint8_t *req_data;
    size_t req_len;
    size_t req_off;
    int nstreams;
    int streams_done;
    MakoNgStream streams[MAKO_NG_MAX_STREAMS];
} MakoNgCtx;

static MakoNgStream *mako_ng_find_stream(MakoNgCtx *c, int32_t stream_id) {
    for (int i = 0; i < c->nstreams; i++) {
        if (c->streams[i].stream_id == stream_id) return &c->streams[i];
    }
    return NULL;
}

static ssize_t mako_ng_send_cb(
    nghttp2_session *session, const uint8_t *data, size_t length,
    int flags, void *user_data
) {
    (void)session;
    (void)flags;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    int n = SSL_write(c->ssl, data, (int)length);
    if (n <= 0) {
        int err = SSL_get_error(c->ssl, n);
        if (err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_READ) {
            c->want_io = 1;
            return NGHTTP2_ERR_WOULDBLOCK;
        }
        return NGHTTP2_ERR_CALLBACK_FAILURE;
    }
    return n;
}

static ssize_t mako_ng_recv_cb(
    nghttp2_session *session, uint8_t *buf, size_t length,
    int flags, void *user_data
) {
    (void)session;
    (void)flags;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    int n = SSL_read(c->ssl, buf, (int)length);
    if (n <= 0) {
        int err = SSL_get_error(c->ssl, n);
        if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
            c->want_io = 1;
            return NGHTTP2_ERR_WOULDBLOCK;
        }
        if (err == SSL_ERROR_ZERO_RETURN || n == 0)
            return NGHTTP2_ERR_EOF;
        return NGHTTP2_ERR_CALLBACK_FAILURE;
    }
    return n;
}

static int mako_ng_on_header(
    nghttp2_session *session, const nghttp2_frame *frame,
    const uint8_t *name, size_t namelen,
    const uint8_t *value, size_t valuelen,
    uint8_t flags, void *user_data
) {
    (void)session;
    (void)flags;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    if (frame->hd.type != NGHTTP2_HEADERS) return 0;
    MakoNgStream *s = mako_ng_find_stream(c, (int32_t)frame->hd.stream_id);
    if (!s) return 0;
    if (namelen == 7 && memcmp(name, ":status", 7) == 0) {
        int32_t st = 0;
        for (size_t i = 0; i < valuelen; i++) {
            if (value[i] < '0' || value[i] > '9') break;
            st = st * 10 + (value[i] - '0');
        }
        s->status = st;
    }
    return 0;
}

static int mako_ng_on_data(
    nghttp2_session *session, uint8_t flags, int32_t stream_id,
    const uint8_t *data, size_t len, void *user_data
) {
    (void)session;
    (void)flags;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    MakoNgStream *s = mako_ng_find_stream(c, stream_id);
    if (!s) return 0;
    size_t copy = len;
    if (s->body_len + copy > sizeof(s->body) - 1)
        copy = sizeof(s->body) - 1 - s->body_len;
    if (copy) {
        memcpy(s->body + s->body_len, data, copy);
        s->body_len += copy;
    }
    return 0;
}

static void mako_ng_mark_done(MakoNgCtx *c, MakoNgStream *s) {
    if (!s || s->done) return;
    s->done = 1;
    c->streams_done++;
}

static int mako_ng_on_stream_close(
    nghttp2_session *session, int32_t stream_id,
    uint32_t error_code, void *user_data
) {
    (void)session;
    (void)error_code;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    MakoNgStream *s = mako_ng_find_stream(c, stream_id);
    mako_ng_mark_done(c, s);
    return 0;
}

static int mako_ng_on_frame_recv(
    nghttp2_session *session, const nghttp2_frame *frame, void *user_data
) {
    (void)session;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    if (frame->hd.type == NGHTTP2_HEADERS
        && (frame->hd.flags & NGHTTP2_FLAG_END_STREAM)) {
        MakoNgStream *s = mako_ng_find_stream(c, (int32_t)frame->hd.stream_id);
        mako_ng_mark_done(c, s);
    }
    return 0;
}

static ssize_t mako_ng_data_source_read(
    nghttp2_session *session, int32_t stream_id,
    uint8_t *buf, size_t length, uint32_t *data_flags,
    nghttp2_data_source *source, void *user_data
) {
    (void)session;
    (void)stream_id;
    (void)source;
    MakoNgCtx *c = (MakoNgCtx *)user_data;
    size_t left = (c->req_len > c->req_off) ? (c->req_len - c->req_off) : 0;
    size_t n = left < length ? left : length;
    if (n && c->req_data) {
        memcpy(buf, c->req_data + c->req_off, n);
        c->req_off += n;
    }
    if (c->req_off >= c->req_len)
        *data_flags |= NGHTTP2_DATA_FLAG_EOF;
    return (ssize_t)n;
}

static int mako_ng_all_done(MakoNgCtx *c) {
    return c->nstreams > 0 && c->streams_done >= c->nstreams;
}

static int mako_ng_drive(nghttp2_session *session, MakoNgCtx *c) {
    for (int i = 0; i < 512 && !mako_ng_all_done(c); i++) {
        c->want_io = 0;
        int rv = nghttp2_session_send(session);
        if (rv != 0 && rv != NGHTTP2_ERR_WOULDBLOCK) return -1;
        rv = nghttp2_session_recv(session);
        if (rv != 0 && rv != NGHTTP2_ERR_WOULDBLOCK && rv != NGHTTP2_ERR_EOF)
            return -1;
        if (rv == NGHTTP2_ERR_EOF) break;
        if (!nghttp2_session_want_read(session)
            && !nghttp2_session_want_write(session)
            && !c->want_io)
            break;
    }
    return mako_ng_all_done(c) ? 0 : -1;
}

static void mako_ng_cleanup(
    nghttp2_session *session, SSL *ssl, int fd, SSL_CTX *tls_ctx
) {
    if (session) nghttp2_session_del(session);
    if (ssl) {
        SSL_shutdown(ssl);
        SSL_free(ssl);
    }
    if (fd >= 0) close(fd);
    if (tls_ctx) SSL_CTX_free(tls_ctx);
}

static nghttp2_session *mako_ng_session_new(MakoNgCtx *ctx) {
    nghttp2_session_callbacks *callbacks = NULL;
    if (nghttp2_session_callbacks_new(&callbacks) != 0) return NULL;
    nghttp2_session_callbacks_set_send_callback(callbacks, mako_ng_send_cb);
    nghttp2_session_callbacks_set_recv_callback(callbacks, mako_ng_recv_cb);
    nghttp2_session_callbacks_set_on_header_callback(callbacks, mako_ng_on_header);
    nghttp2_session_callbacks_set_on_data_chunk_recv_callback(
        callbacks, mako_ng_on_data
    );
    nghttp2_session_callbacks_set_on_stream_close_callback(
        callbacks, mako_ng_on_stream_close
    );
    nghttp2_session_callbacks_set_on_frame_recv_callback(
        callbacks, mako_ng_on_frame_recv
    );
    nghttp2_session *session = NULL;
    if (nghttp2_session_client_new(&session, callbacks, ctx) != 0) {
        nghttp2_session_callbacks_del(callbacks);
        return NULL;
    }
    nghttp2_session_callbacks_del(callbacks);
    return session;
}

static void mako_ng_trim_body(MakoNgStream *s) {
    while (s->body_len > 0
           && (s->body[s->body_len - 1] == '\n'
               || s->body[s->body_len - 1] == '\r'))
        s->body_len--;
}

static MakoString mako_ng_fmt_resp_one(MakoNgStream *s) {
    if (s->status <= 0) return mako_str_from_cstr("");
    mako_ng_trim_body(s);
    char out[4200];
    int wn = snprintf(
        out, sizeof(out), "nghttp2:%d;%.*s",
        (int)s->status, (int)s->body_len, s->body
    );
    if (wn <= 0) return mako_str_from_cstr("");
    return mako_str_from_cstr(out);
}

static MakoString mako_ng_fmt_resp_two(MakoNgStream *a, MakoNgStream *b) {
    if (a->status <= 0 || b->status <= 0) return mako_str_from_cstr("");
    mako_ng_trim_body(a);
    mako_ng_trim_body(b);
    char out[8400];
    int wn = snprintf(
        out, sizeof(out), "nghttp2:%d;%.*s|%d;%.*s",
        (int)a->status, (int)a->body_len, a->body,
        (int)b->status, (int)b->body_len, b->body
    );
    if (wn <= 0) return mako_str_from_cstr("");
    return mako_str_from_cstr(out);
}

static void mako_ng_fill_get_hdrs(
    nghttp2_nv *hdrs, size_t *nh,
    const char *host, const char *path
) {
    size_t i = 0;
    hdrs[i].name = (uint8_t *)":method";
    hdrs[i].value = (uint8_t *)"GET";
    hdrs[i].namelen = 7;
    hdrs[i].valuelen = 3;
    hdrs[i].flags = NGHTTP2_NV_FLAG_NONE;
    i++;
    hdrs[i].name = (uint8_t *)":scheme";
    hdrs[i].value = (uint8_t *)"https";
    hdrs[i].namelen = 7;
    hdrs[i].valuelen = 5;
    hdrs[i].flags = NGHTTP2_NV_FLAG_NONE;
    i++;
    hdrs[i].name = (uint8_t *)":authority";
    hdrs[i].value = (uint8_t *)host;
    hdrs[i].namelen = 10;
    hdrs[i].valuelen = strlen(host);
    hdrs[i].flags = NGHTTP2_NV_FLAG_NONE;
    i++;
    hdrs[i].name = (uint8_t *)":path";
    hdrs[i].value = (uint8_t *)path;
    hdrs[i].namelen = 5;
    hdrs[i].valuelen = strlen(path);
    hdrs[i].flags = NGHTTP2_NV_FLAG_NONE;
    i++;
    *nh = i;
}

/* Shared request: method GET or POST; path customized; optional body for POST. */
static inline MakoString mako_nghttp2_request(
    MakoString host, int64_t port, MakoString path,
    MakoString ca_pem_path, int is_post, MakoString req_body
) {
    char hbuf[256], pbuf[512], cabuf[512];
    if (host.len >= sizeof(hbuf) || path.len >= sizeof(pbuf)
        || ca_pem_path.len >= sizeof(cabuf))
        return mako_str_from_cstr("");
    if (path.len < 1 || path.data[0] != '/')
        return mako_str_from_cstr("");
    memcpy(hbuf, host.data, host.len);
    hbuf[host.len] = 0;
    memcpy(pbuf, path.data, path.len);
    pbuf[path.len] = 0;
    memcpy(cabuf, ca_pem_path.data, ca_pem_path.len);
    cabuf[ca_pem_path.len] = 0;

    SSL_CTX *tls_ctx = NULL;
    int fd = -1;
    SSL *ssl = mako_tls_h2_connect(hbuf, port, cabuf, &tls_ctx, &fd);
    if (!ssl) return mako_str_from_cstr("");

    MakoNgCtx ctx;
    memset(&ctx, 0, sizeof(ctx));
    ctx.ssl = ssl;
    ctx.fd = fd;
    ctx.nstreams = 1;
    ctx.streams[0].status = -1;
    if (is_post && req_body.data && req_body.len) {
        ctx.req_data = (const uint8_t *)req_body.data;
        ctx.req_len = req_body.len;
        ctx.req_off = 0;
    }

    nghttp2_session *session = mako_ng_session_new(&ctx);
    if (!session) {
        mako_ng_cleanup(NULL, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }

    const char *method = is_post ? "POST" : "GET";
    size_t method_len = is_post ? 4 : 3;

    nghttp2_nv hdrs[6];
    size_t nh = 0;
    hdrs[nh].name = (uint8_t *)":method";
    hdrs[nh].value = (uint8_t *)method;
    hdrs[nh].namelen = 7;
    hdrs[nh].valuelen = method_len;
    hdrs[nh].flags = NGHTTP2_NV_FLAG_NONE;
    nh++;
    hdrs[nh].name = (uint8_t *)":scheme";
    hdrs[nh].value = (uint8_t *)"https";
    hdrs[nh].namelen = 7;
    hdrs[nh].valuelen = 5;
    hdrs[nh].flags = NGHTTP2_NV_FLAG_NONE;
    nh++;
    hdrs[nh].name = (uint8_t *)":authority";
    hdrs[nh].value = (uint8_t *)hbuf;
    hdrs[nh].namelen = 10;
    hdrs[nh].valuelen = strlen(hbuf);
    hdrs[nh].flags = NGHTTP2_NV_FLAG_NONE;
    nh++;
    hdrs[nh].name = (uint8_t *)":path";
    hdrs[nh].value = (uint8_t *)pbuf;
    hdrs[nh].namelen = 5;
    hdrs[nh].valuelen = strlen(pbuf);
    hdrs[nh].flags = NGHTTP2_NV_FLAG_NONE;
    nh++;
    if (is_post) {
        hdrs[nh].name = (uint8_t *)"content-type";
        hdrs[nh].value = (uint8_t *)"text/plain";
        hdrs[nh].namelen = 12;
        hdrs[nh].valuelen = 10;
        hdrs[nh].flags = NGHTTP2_NV_FLAG_NONE;
        nh++;
    }

    nghttp2_settings_entry iv = {NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS, 100};
    if (nghttp2_submit_settings(session, NGHTTP2_FLAG_NONE, &iv, 1) != 0) {
        mako_ng_cleanup(session, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }

    int32_t stream_id;
    if (is_post) {
        nghttp2_data_provider data_prd;
        data_prd.source.ptr = NULL;
        data_prd.read_callback = mako_ng_data_source_read;
        stream_id = nghttp2_submit_request(
            session, NULL, hdrs, nh, &data_prd, NULL
        );
    } else {
        stream_id = nghttp2_submit_request(session, NULL, hdrs, nh, NULL, NULL);
    }
    if (stream_id < 0) {
        mako_ng_cleanup(session, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }
    ctx.streams[0].stream_id = stream_id;

    int ok = mako_ng_drive(session, &ctx) == 0 && ctx.streams[0].status > 0;
    MakoString out = ok ? mako_ng_fmt_resp_one(&ctx.streams[0])
                        : mako_str_from_cstr("");
    mako_ng_cleanup(session, ssl, fd, tls_ctx);
    return out;
}

/* Live nghttp2 GET — path is fully customizable (must start with '/'). */
static inline MakoString mako_nghttp2_get(
    MakoString host, int64_t port, MakoString path, MakoString ca_pem_path
) {
    MakoString empty = {NULL, 0};
    return mako_nghttp2_request(host, port, path, ca_pem_path, 0, empty);
}

/* Live nghttp2 POST with body via data provider. */
static inline MakoString mako_nghttp2_post(
    MakoString host, int64_t port, MakoString path,
    MakoString body, MakoString ca_pem_path
) {
    return mako_nghttp2_request(host, port, path, ca_pem_path, 1, body);
}

/* Two overlapping GETs on one session (submit both before driving I/O).
 * Returns "nghttp2:<st1>;<body1>|<st2>;<body2>" on success. */
static inline MakoString mako_nghttp2_get_two(
    MakoString host, int64_t port,
    MakoString path1, MakoString path2, MakoString ca_pem_path
) {
    char hbuf[256], p1buf[512], p2buf[512], cabuf[512];
    if (host.len >= sizeof(hbuf) || path1.len >= sizeof(p1buf)
        || path2.len >= sizeof(p2buf) || ca_pem_path.len >= sizeof(cabuf))
        return mako_str_from_cstr("");
    if (path1.len < 1 || path1.data[0] != '/'
        || path2.len < 1 || path2.data[0] != '/')
        return mako_str_from_cstr("");
    memcpy(hbuf, host.data, host.len);
    hbuf[host.len] = 0;
    memcpy(p1buf, path1.data, path1.len);
    p1buf[path1.len] = 0;
    memcpy(p2buf, path2.data, path2.len);
    p2buf[path2.len] = 0;
    memcpy(cabuf, ca_pem_path.data, ca_pem_path.len);
    cabuf[ca_pem_path.len] = 0;

    SSL_CTX *tls_ctx = NULL;
    int fd = -1;
    SSL *ssl = mako_tls_h2_connect(hbuf, port, cabuf, &tls_ctx, &fd);
    if (!ssl) return mako_str_from_cstr("");

    MakoNgCtx ctx;
    memset(&ctx, 0, sizeof(ctx));
    ctx.ssl = ssl;
    ctx.fd = fd;
    ctx.nstreams = 2;
    ctx.streams[0].status = -1;
    ctx.streams[1].status = -1;

    nghttp2_session *session = mako_ng_session_new(&ctx);
    if (!session) {
        mako_ng_cleanup(NULL, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }

    nghttp2_settings_entry iv = {NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS, 100};
    if (nghttp2_submit_settings(session, NGHTTP2_FLAG_NONE, &iv, 1) != 0) {
        mako_ng_cleanup(session, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }

    nghttp2_nv hdrs1[4], hdrs2[4];
    size_t nh1 = 0, nh2 = 0;
    mako_ng_fill_get_hdrs(hdrs1, &nh1, hbuf, p1buf);
    mako_ng_fill_get_hdrs(hdrs2, &nh2, hbuf, p2buf);

    /* Overlap: submit both requests before driving send/recv. */
    int32_t sid1 = nghttp2_submit_request(session, NULL, hdrs1, nh1, NULL, NULL);
    int32_t sid2 = nghttp2_submit_request(session, NULL, hdrs2, nh2, NULL, NULL);
    if (sid1 < 0 || sid2 < 0) {
        mako_ng_cleanup(session, ssl, fd, tls_ctx);
        return mako_str_from_cstr("");
    }
    ctx.streams[0].stream_id = sid1;
    ctx.streams[1].stream_id = sid2;

    int ok = mako_ng_drive(session, &ctx) == 0
        && ctx.streams[0].status > 0
        && ctx.streams[1].status > 0;
    MakoString out = ok
        ? mako_ng_fmt_resp_two(&ctx.streams[0], &ctx.streams[1])
        : mako_str_from_cstr("");
    mako_ng_cleanup(session, ssl, fd, tls_ctx);
    return out;
}

static inline int64_t mako_nghttp2_available(void) {
    return 1;
}

#else /* !MAKO_HAS_NGHTTP2 || !MAKO_TLS_REAL */

static inline MakoString mako_nghttp2_get(
    MakoString host, int64_t port, MakoString path, MakoString ca_pem_path
) {
    (void)host;
    (void)port;
    (void)path;
    (void)ca_pem_path;
    fprintf(stderr, "mako nghttp2_get: libnghttp2 not linked (need MAKO_HAS_NGHTTP2)\n");
    return mako_str_from_cstr("");
}

static inline MakoString mako_nghttp2_post(
    MakoString host, int64_t port, MakoString path,
    MakoString body, MakoString ca_pem_path
) {
    (void)host;
    (void)port;
    (void)path;
    (void)body;
    (void)ca_pem_path;
    fprintf(stderr, "mako nghttp2_post: libnghttp2 not linked (need MAKO_HAS_NGHTTP2)\n");
    return mako_str_from_cstr("");
}

static inline MakoString mako_nghttp2_get_two(
    MakoString host, int64_t port,
    MakoString path1, MakoString path2, MakoString ca_pem_path
) {
    (void)host;
    (void)port;
    (void)path1;
    (void)path2;
    (void)ca_pem_path;
    fprintf(stderr, "mako nghttp2_get_two: libnghttp2 not linked (need MAKO_HAS_NGHTTP2)\n");
    return mako_str_from_cstr("");
}

static inline int64_t mako_nghttp2_available(void) {
    return 0;
}

#endif /* MAKO_HAS_NGHTTP2 && MAKO_TLS_REAL */

#endif /* MAKO_NGHTTP2_H */
