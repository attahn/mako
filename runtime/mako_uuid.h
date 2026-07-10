/* Mako UUID — v4 generate + canonical parse/format (portable C) */
#ifndef MAKO_UUID_H
#define MAKO_UUID_H

#include "mako_rt.h"
#if defined(_WIN32) || defined(_WIN64)
#include <time.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint8_t b[16];
} MakoUuid;

static inline void mako_uuid_fill_random(uint8_t *out, size_t n) {
#if defined(_WIN32) || defined(_WIN64)
    static int seeded = 0;
    if (!seeded) {
        srand((unsigned)time(NULL) ^ (unsigned)(uintptr_t)&seeded);
        seeded = 1;
    }
    for (size_t i = 0; i < n; i++) out[i] = (uint8_t)(rand() & 0xff);
#elif defined(__APPLE__)
    /* arc4random_buf is CSPRNG on Apple platforms */
    arc4random_buf(out, n);
#else
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0) {
        mako_abort("uuid: cannot open /dev/urandom");
    }
    size_t got = 0;
    while (got < n) {
        ssize_t r = read(fd, out + got, n - got);
        if (r <= 0) {
            close(fd);
            mako_abort("uuid: /dev/urandom read failed");
        }
        got += (size_t)r;
    }
    close(fd);
#endif
}

static inline MakoUuid mako_uuid_v4(void) {
    MakoUuid u;
    mako_uuid_fill_random(u.b, 16);
    /* RFC 4122: version 4, variant 10xx */
    u.b[6] = (uint8_t)((u.b[6] & 0x0F) | 0x40);
    u.b[8] = (uint8_t)((u.b[8] & 0x3F) | 0x80);
    return u;
}

static inline MakoUuid mako_uuid_nil(void) {
    MakoUuid u;
    memset(u.b, 0, 16);
    return u;
}

static inline bool mako_uuid_is_nil(MakoUuid u) {
    for (int i = 0; i < 16; i++) {
        if (u.b[i] != 0) return false;
    }
    return true;
}

static inline bool mako_uuid_eq(MakoUuid a, MakoUuid b) {
    return memcmp(a.b, b.b, 16) == 0;
}

static inline char mako_uuid_hex_nibble(unsigned v) {
    return (char)(v < 10 ? ('0' + v) : ('a' + (v - 10)));
}

static inline MakoString mako_uuid_string(MakoUuid u) {
    char buf[37];
    static const int dash_at[] = {8, 13, 18, 23};
    int di = 0;
    int bi = 0;
    for (int i = 0; i < 36; i++) {
        if (di < 4 && i == dash_at[di]) {
            buf[i] = '-';
            di++;
        } else {
            uint8_t byte = u.b[bi / 2];
            unsigned nib = (bi % 2 == 0) ? (byte >> 4) : (byte & 0x0F);
            buf[i] = mako_uuid_hex_nibble(nib);
            bi++;
        }
    }
    buf[36] = 0;
    return mako_str_from_cstr(buf);
}

static inline int mako_uuid_hex_val(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/* Parse canonical 8-4-4-4-12 hex (case-insensitive). On failure returns nil + *ok=false. */
static inline MakoUuid mako_uuid_parse(MakoString s, bool *ok) {
    if (ok) *ok = false;
    if (s.len != 36) {
        return mako_uuid_nil();
    }
    static const int dash_pos[] = {8, 13, 18, 23};
    for (int i = 0; i < 4; i++) {
        if (s.data[dash_pos[i]] != '-') {
            return mako_uuid_nil();
        }
    }
    MakoUuid u;
    int bi = 0;
    for (size_t i = 0; i < 36; i++) {
        if (s.data[i] == '-') continue;
        int hi = mako_uuid_hex_val(s.data[i]);
        if (hi < 0 || i + 1 >= 36) return mako_uuid_nil();
        int lo = mako_uuid_hex_val(s.data[i + 1]);
        if (lo < 0) return mako_uuid_nil();
        u.b[bi++] = (uint8_t)((hi << 4) | lo);
        i++; /* consumed second nibble */
        if (bi > 16) return mako_uuid_nil();
    }
    if (bi != 16) return mako_uuid_nil();
    if (ok) *ok = true;
    return u;
}

static inline bool mako_uuid_parse_ok(MakoString s) {
    bool ok = false;
    (void)mako_uuid_parse(s, &ok);
    return ok;
}

/* Result[int,string]: Ok(1) if valid canonical UUID string, else Err(msg). */
static inline MakoResultInt mako_uuid_check(MakoString s) {
    if (mako_uuid_parse_ok(s)) {
        return mako_ok_int(1);
    }
    return mako_err_int(mako_str_from_cstr("invalid UUID string (want 8-4-4-4-12 hex)"));
}

#ifdef __cplusplus
}
#endif

#endif /* MAKO_UUID_H */
