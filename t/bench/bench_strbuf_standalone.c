/*
 * Standalone C benchmark mirroring the Rust strbuf_bench.rs workloads.
 *
 * This copies git's strbuf.c logic inline so it compiles without git's full
 * build (no `die`, `BUG`, `xmalloc` dependencies). It lets us time the same
 * workloads as `crates/git-core/examples/strbuf_bench.rs` for comparison.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>

#define NSEC(ts) ((ts).tv_sec * 1000000000LL + (ts).tv_nsec)

/* Inline copy of git's ALLOC_GROW logic. */
#define ALLOC_GROW(ptr, nr, cap) \
    do { if ((nr) > (cap)) { (cap) = alloc_nr(cap); ptr = xrealloc(ptr, alloc_nr(cap)); } } while (0)
static inline size_t alloc_nr(size_t cap) { return (cap ? cap : 16) * 3 / 2; }
static inline void *xmalloc(size_t sz) { void *r = malloc(sz); if (!r) abort(); return r; }
static inline void *xrealloc(void *p, size_t sz) { void *r = realloc(p, sz); if (!r) abort(); return r; }

typedef struct strbuf {
    size_t alloc;
    size_t len;
    char *buf;
} strbuf;

#define STRBUF_INIT { .buf = NULL, .len = 0, .alloc = 0 }

static void strbuf_grow(struct strbuf *sb, size_t extra);
static void strbuf_init(struct strbuf *sb, size_t hint) {
    memset(sb, 0, sizeof(*sb));
    if (hint) strbuf_grow(sb, hint);
}

static void strbuf_grow(struct strbuf *sb, size_t extra) {
    size_t new_len = sb->len + extra + 1;
    if (new_len > sb->alloc) {
        char *new_buf = xrealloc(sb->buf, new_len);
        sb->buf = new_buf;
        sb->alloc = new_len;
    }
    if (!sb->buf) {
        sb->buf = xmalloc(1);
        sb->buf[0] = '\0';
        sb->alloc = 1;
    }
}

static void strbuf_release(struct strbuf *sb) {
    free(sb->buf);
    sb->buf = NULL;
    sb->len = 0;
    sb->alloc = 0;
}

static void strbuf_add(struct strbuf *sb, const void *data, size_t len) {
    strbuf_grow(sb, len);
    memcpy(sb->buf + sb->len, data, len);
    sb->len += len;
    sb->buf[sb->len] = '\0';
}

static void strbuf_addch(struct strbuf *sb, char c) {
    strbuf_grow(sb, 1);
    sb->buf[sb->len++] = c;
    sb->buf[sb->len] = '\0';
}

static void strbuf_addstr(struct strbuf *sb, const char *s) {
    strbuf_add(sb, s, strlen(s));
}

static const char *fmt_ns(long long ns) {
    static char buf[64];
    if (ns >= 1000000) snprintf(buf, sizeof(buf), "%.3f ms", ns / 1e6);
    else if (ns >= 1000) snprintf(buf, sizeof(buf), "%.3f us", ns / 1e3);
    else snprintf(buf, sizeof(buf), "%lld ns", ns);
    return buf;
}

#define BENCH(n, body) do { \
    struct timespec _a, _b; \
    long _i; \
    for (_i = 0; _i < (long)((n) < 16 ? (n) : 16); _i++) { body } \
    clock_gettime(CLOCK_MONOTONIC, &_a); \
    for (_i = 0; _i < (long)(n); _i++) { body } \
    clock_gettime(CLOCK_MONOTONIC, &_b); \
    _ns = (NSEC(_b) - NSEC(_a)) / (n); \
} while (0)

int main(void) {
    long long _ns;

    printf("\n=== strbuf_static_init (init + release) ===\n");
    BENCH(2000000, {
        struct strbuf b = STRBUF_INIT;
        strbuf_release(&b);
    });
    printf("%-34s %s\n", "STRBUF_INIT+release", fmt_ns(_ns));

    printf("\n=== strbuf_dynamic_init (init(1024) + release) ===\n");
    BENCH(1000000, {
        struct strbuf b;
        strbuf_init(&b, 1024);
        strbuf_release(&b);
    });
    printf("%-34s %s\n", "init(1024)+release", fmt_ns(_ns));

    printf("\n=== strbuf_add_single_char (addch + release) ===\n");
    BENCH(1000000, {
        struct strbuf b = STRBUF_INIT;
        strbuf_addch(&b, 'a');
        strbuf_release(&b);
    });
    printf("%-34s %s\n", "init; addch; release", fmt_ns(_ns));

    printf("\n=== strbuf_add_single_str (addstr + release) ===\n");
    BENCH(500000, {
        struct strbuf b = STRBUF_INIT;
        strbuf_addstr(&b, "hello there");
        strbuf_release(&b);
    });
    printf("%-34s %s\n", "init; addstr(11); release", fmt_ns(_ns));

    printf("\n=== strbuf_add_append_str (init + append + release) ===\n");
    BENCH(500000, {
        struct strbuf b = STRBUF_INIT;
        strbuf_addstr(&b, "initial value");
        strbuf_addstr(&b, "hello there");
        strbuf_release(&b);
    });
    printf("%-34s %s\n", "init; addstr+addstr; release", fmt_ns(_ns));

    printf("\n=== strbuf_many_small_appends (10000 x addch) ===\n");
    BENCH(1000, {
        struct strbuf b = STRBUF_INIT;
        long k;
        for (k = 0; k < 10000; k++) strbuf_addch(&b, 'x');
        strbuf_release(&b);
    });
    printf("%-34s %s (10000 addch each)\n", "buffers+release", fmt_ns(_ns));

    printf("\n=== strbuf_large_append (64MB addstr once) ===\n");
    {
        char *payload = xmalloc(64 * 1024 * 1024 + 1);
        memset(payload, 'y', 64 * 1024 * 1024);
        payload[64 * 1024 * 1024] = '\0';
        BENCH(3, {
            struct strbuf b = STRBUF_INIT;
            strbuf_addstr(&b, payload);
            strbuf_release(&b);
        });
        free(payload);
        printf("%-34s %s/iter (64 MB)\n", "addstr(64MB)+release", fmt_ns(_ns));
    }

    printf("\n=== Summary ===\n");
    printf("C strbuf benchmarks complete.\n");
    return 0;
}
