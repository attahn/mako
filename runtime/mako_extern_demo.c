/* Tiny C symbols for extern "C" demos — linked when present. */
#include <stdint.h>

int64_t mako_c_abs(int64_t n) {
    return n < 0 ? -n : n;
}

int64_t mako_c_add(int64_t a, int64_t b) {
    return a + b;
}
