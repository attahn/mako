/* Mako cloud/distributed systems primitives.
 * Consistent hashing, rate limiting, circuit breaker, JWT, retry.
 * Designed for microservices, API gateways, distributed databases. */
#ifndef MAKO_CLOUD_H
#define MAKO_CLOUD_H

#include "mako_rt.h"
#include <stdatomic.h>
#include <string.h>
#include <stdlib.h>
#include <time.h>
#include <stdint.h>

/* ============================================================
 * Consistent Hash Ring — distribute keys across N nodes.
 * Uses virtual nodes (vnodes) for even distribution.
 * Thread-safe reads (sorted array, binary search).
 * ============================================================ */

#define CHASH_MAX_VNODES 4096

typedef struct {
    uint32_t hash;
    int node_id;
} CHashPoint;

typedef struct {
    CHashPoint points[CHASH_MAX_VNODES];
    int count;
    int node_count;
    int vnodes_per_node;
} MakoCHash;

static inline uint32_t chash_fnv1a(const char *data, size_t len) {
    uint32_t h = 2166136261u;
    for (size_t i = 0; i < len; i++) {
        h ^= (uint8_t)data[i];
        h *= 16777619u;
    }
    return h;
}

static int chash_cmp(const void *a, const void *b) {
    uint32_t ha = ((const CHashPoint *)a)->hash;
    uint32_t hb = ((const CHashPoint *)b)->hash;
    return (ha > hb) - (ha < hb);
}

/* Create a consistent hash ring with `nodes` nodes, `vnodes` virtual nodes each. */
static inline MakoCHash *mako_chash_new(int64_t nodes, int64_t vnodes) {
    MakoCHash *r = (MakoCHash *)calloc(1, sizeof(MakoCHash));
    if (!r) return NULL;
    r->node_count = (int)nodes;
    r->vnodes_per_node = (int)(vnodes > 0 ? vnodes : 150);
    r->count = 0;

    for (int n = 0; n < (int)nodes && r->count < CHASH_MAX_VNODES; n++) {
        for (int v = 0; v < r->vnodes_per_node && r->count < CHASH_MAX_VNODES; v++) {
            char buf[64];
            int len = snprintf(buf, sizeof(buf), "node%d#%d", n, v);
            r->points[r->count].hash = chash_fnv1a(buf, (size_t)len);
            r->points[r->count].node_id = n;
            r->count++;
        }
    }
    qsort(r->points, (size_t)r->count, sizeof(CHashPoint), chash_cmp);
    return r;
}

/* Look up which node owns a key. Returns node_id (0-based). */
static inline int64_t mako_chash_get(MakoCHash *r, MakoString key) {
    if (!r || r->count == 0 || !key.data) return 0;
    uint32_t h = chash_fnv1a(key.data, key.len);

    /* Binary search for first point >= h */
    int lo = 0, hi = r->count;
    while (lo < hi) {
        int mid = (lo + hi) / 2;
        if (r->points[mid].hash < h) lo = mid + 1;
        else hi = mid;
    }
    if (lo >= r->count) lo = 0; /* wrap around */
    return (int64_t)r->points[lo].node_id;
}

/* Add a node to the ring. Returns new node_id. */
static inline int64_t mako_chash_add_node(MakoCHash *r) {
    if (!r) return -1;
    int n = r->node_count;
    r->node_count++;
    for (int v = 0; v < r->vnodes_per_node && r->count < CHASH_MAX_VNODES; v++) {
        char buf[64];
        int len = snprintf(buf, sizeof(buf), "node%d#%d", n, v);
        r->points[r->count].hash = chash_fnv1a(buf, (size_t)len);
        r->points[r->count].node_id = n;
        r->count++;
    }
    qsort(r->points, (size_t)r->count, sizeof(CHashPoint), chash_cmp);
    return (int64_t)n;
}

/* Remove a node from the ring. */
static inline void mako_chash_remove_node(MakoCHash *r, int64_t node_id) {
    if (!r) return;
    int w = 0;
    for (int i = 0; i < r->count; i++) {
        if (r->points[i].node_id != (int)node_id) {
            r->points[w++] = r->points[i];
        }
    }
    r->count = w;
}

static inline int64_t mako_chash_node_count(MakoCHash *r) {
    return r ? (int64_t)r->node_count : 0;
}

static inline void mako_chash_free(MakoCHash *r) { free(r); }

/* ============================================================
 * Token Bucket Rate Limiter — thread-safe, microsecond precision.
 * ============================================================ */

typedef struct {
    atomic_int_fast64_t tokens;     /* current tokens (scaled by 1000 for precision) */
    atomic_int_fast64_t last_us;    /* last refill timestamp (microseconds) */
    int64_t max_tokens;             /* capacity (scaled by 1000) */
    int64_t refill_rate;            /* tokens per second (scaled by 1000) */
} MakoRateLimiter;

static inline int64_t mako_rl_now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000LL + (int64_t)ts.tv_nsec / 1000LL;
}

/* Create rate limiter: `rate` requests/second, `burst` max burst size. */
static inline MakoRateLimiter *mako_ratelimit_new(int64_t rate, int64_t burst) {
    MakoRateLimiter *rl = (MakoRateLimiter *)calloc(1, sizeof(MakoRateLimiter));
    if (!rl) return NULL;
    rl->max_tokens = burst * 1000;
    rl->refill_rate = rate * 1000;
    atomic_store(&rl->tokens, rl->max_tokens);
    atomic_store(&rl->last_us, mako_rl_now_us());
    return rl;
}

/* Try to consume 1 token. Returns 1 if allowed, 0 if rate-limited. */
static inline int64_t mako_ratelimit_allow(MakoRateLimiter *rl) {
    if (!rl) return 0;
    int64_t now = mako_rl_now_us();
    int64_t last = atomic_load(&rl->last_us);
    int64_t elapsed_us = now - last;

    if (elapsed_us > 0) {
        /* Refill tokens */
        int64_t add = (elapsed_us * rl->refill_rate) / 1000000;
        if (add > 0) {
            atomic_store(&rl->last_us, now);
            int64_t cur = atomic_load(&rl->tokens);
            int64_t new_val = cur + add;
            if (new_val > rl->max_tokens) new_val = rl->max_tokens;
            atomic_store(&rl->tokens, new_val);
        }
    }

    /* Try to consume */
    int64_t cur = atomic_load(&rl->tokens);
    if (cur >= 1000) {
        atomic_fetch_sub(&rl->tokens, 1000);
        return 1;
    }
    return 0;
}

/* Check tokens remaining without consuming. */
static inline int64_t mako_ratelimit_remaining(MakoRateLimiter *rl) {
    if (!rl) return 0;
    return atomic_load(&rl->tokens) / 1000;
}

static inline void mako_ratelimit_free(MakoRateLimiter *rl) { free(rl); }

/* ============================================================
 * Circuit Breaker — resilient service calls.
 * States: 0=CLOSED (normal), 1=OPEN (failing), 2=HALF_OPEN (testing)
 * ============================================================ */

#define CB_CLOSED    0
#define CB_OPEN      1
#define CB_HALF_OPEN 2

typedef struct {
    atomic_int state;
    atomic_int_fast64_t failures;
    atomic_int_fast64_t successes;
    atomic_int_fast64_t last_failure_us;
    int64_t threshold;      /* failures before opening */
    int64_t timeout_us;     /* how long to stay open before half-open */
    int64_t half_open_max;  /* successes needed to close from half-open */
} MakoCircuitBreaker;

static inline MakoCircuitBreaker *mako_breaker_new(int64_t threshold, int64_t timeout_ms, int64_t half_open_max) {
    MakoCircuitBreaker *cb = (MakoCircuitBreaker *)calloc(1, sizeof(MakoCircuitBreaker));
    if (!cb) return NULL;
    cb->threshold = threshold > 0 ? threshold : 5;
    cb->timeout_us = timeout_ms * 1000;
    cb->half_open_max = half_open_max > 0 ? half_open_max : 3;
    atomic_store(&cb->state, CB_CLOSED);
    atomic_store(&cb->failures, 0);
    atomic_store(&cb->successes, 0);
    atomic_store(&cb->last_failure_us, 0);
    return cb;
}

/* Check if a request is allowed. Returns 1=allow, 0=blocked (circuit open). */
static inline int64_t mako_breaker_allow(MakoCircuitBreaker *cb) {
    if (!cb) return 1;
    int state = atomic_load(&cb->state);

    if (state == CB_CLOSED) return 1;

    if (state == CB_OPEN) {
        /* Check if timeout elapsed → transition to half-open */
        int64_t now = mako_rl_now_us();
        int64_t last = atomic_load(&cb->last_failure_us);
        if (now - last > cb->timeout_us) {
            atomic_store(&cb->state, CB_HALF_OPEN);
            atomic_store(&cb->successes, 0);
            return 1;
        }
        return 0; /* still open */
    }

    /* HALF_OPEN: allow limited requests */
    return 1;
}

/* Report success. */
static inline void mako_breaker_success(MakoCircuitBreaker *cb) {
    if (!cb) return;
    int state = atomic_load(&cb->state);
    if (state == CB_HALF_OPEN) {
        int64_t s = atomic_fetch_add(&cb->successes, 1) + 1;
        if (s >= cb->half_open_max) {
            atomic_store(&cb->state, CB_CLOSED);
            atomic_store(&cb->failures, 0);
        }
    } else if (state == CB_CLOSED) {
        atomic_store(&cb->failures, 0);
    }
}

/* Report failure. */
static inline void mako_breaker_failure(MakoCircuitBreaker *cb) {
    if (!cb) return;
    int state = atomic_load(&cb->state);
    atomic_store(&cb->last_failure_us, mako_rl_now_us());

    if (state == CB_HALF_OPEN) {
        /* Any failure in half-open → reopen */
        atomic_store(&cb->state, CB_OPEN);
    } else if (state == CB_CLOSED) {
        int64_t f = atomic_fetch_add(&cb->failures, 1) + 1;
        if (f >= cb->threshold) {
            atomic_store(&cb->state, CB_OPEN);
        }
    }
}

/* Get current state: 0=closed, 1=open, 2=half-open */
static inline int64_t mako_breaker_state(MakoCircuitBreaker *cb) {
    return cb ? (int64_t)atomic_load(&cb->state) : 0;
}

static inline void mako_breaker_reset(MakoCircuitBreaker *cb) {
    if (!cb) return;
    atomic_store(&cb->state, CB_CLOSED);
    atomic_store(&cb->failures, 0);
    atomic_store(&cb->successes, 0);
}

static inline void mako_breaker_free(MakoCircuitBreaker *cb) { free(cb); }

/* ============================================================
 * JWT (HMAC-SHA256) — sign and verify tokens.
 * Minimal implementation: header.payload.signature
 * ============================================================ */

/* Base64url encode (no padding) */
static inline size_t mako_b64url_encode(const uint8_t *src, size_t slen, char *dst, size_t dlen) {
    static const char t[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    size_t o = 0;
    for (size_t i = 0; i < slen && o + 4 < dlen; i += 3) {
        uint32_t v = (uint32_t)src[i] << 16;
        if (i + 1 < slen) v |= (uint32_t)src[i+1] << 8;
        if (i + 2 < slen) v |= (uint32_t)src[i+2];
        dst[o++] = t[(v >> 18) & 63];
        dst[o++] = t[(v >> 12) & 63];
        if (i + 1 < slen) dst[o++] = t[(v >> 6) & 63];
        if (i + 2 < slen) dst[o++] = t[v & 63];
    }
    dst[o] = 0;
    return o;
}

static inline size_t mako_b64url_decode(const char *src, size_t slen, uint8_t *dst, size_t dlen) {
    static const int8_t t[256] = {
        [0 ... 255] = -1,
        ['A'] = 0, ['B'] = 1, ['C'] = 2, ['D'] = 3, ['E'] = 4, ['F'] = 5,
        ['G'] = 6, ['H'] = 7, ['I'] = 8, ['J'] = 9, ['K'] = 10, ['L'] = 11,
        ['M'] = 12, ['N'] = 13, ['O'] = 14, ['P'] = 15, ['Q'] = 16, ['R'] = 17,
        ['S'] = 18, ['T'] = 19, ['U'] = 20, ['V'] = 21, ['W'] = 22, ['X'] = 23,
        ['Y'] = 24, ['Z'] = 25,
        ['a'] = 26, ['b'] = 27, ['c'] = 28, ['d'] = 29, ['e'] = 30, ['f'] = 31,
        ['g'] = 32, ['h'] = 33, ['i'] = 34, ['j'] = 35, ['k'] = 36, ['l'] = 37,
        ['m'] = 38, ['n'] = 39, ['o'] = 40, ['p'] = 41, ['q'] = 42, ['r'] = 43,
        ['s'] = 44, ['t'] = 45, ['u'] = 46, ['v'] = 47, ['w'] = 48, ['x'] = 49,
        ['y'] = 50, ['z'] = 51,
        ['0'] = 52, ['1'] = 53, ['2'] = 54, ['3'] = 55, ['4'] = 56,
        ['5'] = 57, ['6'] = 58, ['7'] = 59, ['8'] = 60, ['9'] = 61,
        ['-'] = 62, ['_'] = 63,
    };
    size_t o = 0;
    for (size_t i = 0; i < slen && o < dlen;) {
        uint32_t v = 0;
        int bits = 0;
        for (int j = 0; j < 4 && i < slen; j++, i++) {
            int8_t d = t[(uint8_t)src[i]];
            if (d < 0) break;
            v = (v << 6) | (uint32_t)d;
            bits += 6;
        }
        if (bits >= 8) { dst[o++] = (uint8_t)(v >> (bits - 8)); bits -= 8; }
        if (bits >= 8 && o < dlen) { dst[o++] = (uint8_t)(v >> (bits - 8)); bits -= 8; }
        if (bits >= 8 && o < dlen) { dst[o++] = (uint8_t)(v >> (bits - 8)); }
    }
    return o;
}

/* Uses mako_hmac_sha256_raw from mako_std.h (included before this file) */

/* Sign a JWT payload with HMAC-SHA256. Returns "header.payload.signature". */
static inline MakoString mako_jwt_sign(MakoString payload, MakoString secret) {
    /* Header is always {"alg":"HS256","typ":"JWT"} */
    const char *hdr_json = "{\"alg\":\"HS256\",\"typ\":\"JWT\"}";
    size_t hdr_len = strlen(hdr_json);

    /* Base64url encode header and payload */
    char hdr_b64[128], pay_b64[8192];
    size_t hdr_b64_len = mako_b64url_encode((const uint8_t *)hdr_json, hdr_len, hdr_b64, sizeof(hdr_b64));
    size_t pay_b64_len = mako_b64url_encode((const uint8_t *)payload.data, payload.len, pay_b64, sizeof(pay_b64));

    /* Build signing input: header.payload */
    size_t input_len = hdr_b64_len + 1 + pay_b64_len;
    char *input = (char *)malloc(input_len + 1);
    if (!input) return mako_str_from_cstr("");
    memcpy(input, hdr_b64, hdr_b64_len);
    input[hdr_b64_len] = '.';
    memcpy(input + hdr_b64_len + 1, pay_b64, pay_b64_len);
    input[input_len] = 0;

    /* HMAC-SHA256 */
    MakoString input_str = {input, input_len};
    MakoString sig_raw = mako_hmac_sha256_raw(secret, input_str);

    /* Base64url encode signature */
    char sig_b64[128];
    size_t sig_b64_len = mako_b64url_encode((const uint8_t *)sig_raw.data, sig_raw.len, sig_b64, sizeof(sig_b64));

    /* Build final token: header.payload.signature */
    size_t total = input_len + 1 + sig_b64_len;
    char *token = (char *)malloc(total + 1);
    if (!token) { free(input); return mako_str_from_cstr(""); }
    memcpy(token, input, input_len);
    token[input_len] = '.';
    memcpy(token + input_len + 1, sig_b64, sig_b64_len);
    token[total] = 0;

    free(input);
    return (MakoString){token, total};
}

/* Verify a JWT token. Returns 1 if valid, 0 if invalid. */
static inline int64_t mako_jwt_verify(MakoString token, MakoString secret) {
    if (!token.data || token.len < 5) return 0;

    /* Find last dot (separates signature) */
    int last_dot = -1;
    for (int i = (int)token.len - 1; i >= 0; i--) {
        if (token.data[i] == '.') { last_dot = i; break; }
    }
    if (last_dot < 0) return 0;

    /* Signing input = everything before last dot */
    MakoString input = {token.data, (size_t)last_dot};

    /* Recompute HMAC */
    MakoString sig_raw = mako_hmac_sha256_raw(secret, input);
    char sig_b64[128];
    size_t sig_b64_len = mako_b64url_encode((const uint8_t *)sig_raw.data, sig_raw.len, sig_b64, sizeof(sig_b64));

    /* Compare with provided signature */
    const char *provided = token.data + last_dot + 1;
    size_t provided_len = token.len - (size_t)last_dot - 1;

    if (provided_len != sig_b64_len) return 0;

    /* Constant-time compare to prevent timing attacks */
    uint8_t diff = 0;
    for (size_t i = 0; i < sig_b64_len; i++) {
        diff |= (uint8_t)(sig_b64[i] ^ provided[i]);
    }
    return diff == 0 ? 1 : 0;
}

/* Extract payload from JWT (base64url-decoded). Does NOT verify! */
static inline MakoString mako_jwt_payload(MakoString token) {
    if (!token.data) return mako_str_from_cstr("");
    /* Find first and second dots */
    int first_dot = -1, second_dot = -1;
    for (size_t i = 0; i < token.len; i++) {
        if (token.data[i] == '.') {
            if (first_dot < 0) first_dot = (int)i;
            else { second_dot = (int)i; break; }
        }
    }
    if (first_dot < 0 || second_dot < 0) return mako_str_from_cstr("");

    const char *pay_b64 = token.data + first_dot + 1;
    size_t pay_b64_len = (size_t)(second_dot - first_dot - 1);

    uint8_t decoded[8192];
    size_t dlen = mako_b64url_decode(pay_b64, pay_b64_len, decoded, sizeof(decoded));
    if (dlen == 0) return mako_str_from_cstr("");

    char *out = (char *)malloc(dlen + 1);
    if (!out) return mako_str_from_cstr("");
    memcpy(out, decoded, dlen);
    out[dlen] = 0;
    return (MakoString){out, dlen};
}

/* ============================================================
 * Retry with exponential backoff.
 * Helper: compute sleep duration for attempt N.
 * ============================================================ */

/* Returns milliseconds to sleep before retry N (0-based).
 * Formula: min(base_ms * 2^attempt, max_ms) + jitter */
static inline int64_t mako_backoff_ms(int64_t attempt, int64_t base_ms, int64_t max_ms) {
    int64_t delay = base_ms;
    for (int64_t i = 0; i < attempt && delay < max_ms; i++) {
        delay *= 2;
    }
    if (delay > max_ms) delay = max_ms;
    /* Add ~10% jitter */
    delay += (delay / 10) * ((int64_t)rand() % 3);
    return delay;
}

/* ============================================================
 * Secure environment / secrets helpers.
 * ============================================================ */

/* Get env var or return default. Never returns NULL. */
static inline MakoString mako_env_get_or(MakoString name, MakoString def) {
    char nbuf[512];
    if (!name.data || name.len >= sizeof(nbuf)) return def;
    memcpy(nbuf, name.data, name.len);
    nbuf[name.len] = 0;
    const char *val = getenv(nbuf);
    if (!val || val[0] == 0) return def;
    return mako_str_from_cstr(val);
}

/* Check if env var is set (non-empty). */
static inline int64_t mako_env_has(MakoString name) {
    char nbuf[512];
    if (!name.data || name.len >= sizeof(nbuf)) return 0;
    memcpy(nbuf, name.data, name.len);
    nbuf[name.len] = 0;
    const char *val = getenv(nbuf);
    return (val && val[0] != 0) ? 1 : 0;
}

#endif /* MAKO_CLOUD_H */
