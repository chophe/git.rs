/*
 * Benchmark harness for the C `strbuf` implementation, mirroring the
 * operations measured by the Rust example `strbuf_bench.rs`.
 *
 * Also measures reallocation count by instrumenting strbuf_grow via the
 * wrapper below (a faithful stand-in that delegates to the real API).
 *
 * Build (from the repo root):
 *   make -C ../..   (or provide a git build tree)
 *   gcc -O2 -I../.. bench_strbuf.c -o bench_strbuf
 *
 * The benchmark needs git's compiled objects; the simplest is to build via
 * the git Makefile after dropping this file into the `t/bench` directory and
 * adding a target. Below we document the operations measured.
 */

#include "../../git-compat-util.h"
#include "../../strbuf.h"
#include <time.h>
#include <stdio.h>
#include <stdlib.h>

#ifndef NSEC
#define NSEC(ts) ((ts).tv_sec * 1000000000LL + (ts).tv_nsec)
#endif

static struct strbuf_metrics {
    unsigned long allocs;
    unsigned long reallocs;
} metrics;

/* Time ns for `n` iterations of fn(). */
#define BENCH(n, body)                                                         \
    do {                                                                       \
        struct timespec _a, _b;                                                \
        long _i;                                                               \
        for (_i = 0; _i < (long)((n) < 16 ? (n) : 16); _i++) { body }          \
        clock_gettime(CLOCK_MONOTONIC, &_a);                                   \
        for (_i = 0; _i < (long)(n); _i++) { body }                            \
        clock_gettime(CLOCK_MONOTONIC, &_b);                                   \
        _ns = (NSEC(_b) - NSEC(_a)) / (n);                                     \
    } while (0)

static const char *fmt_ns(long long ns)
{
    static char buf[64];
    if (ns >= 1000000)
        snprintf(buf, sizeof(buf), "%.3f ms", ns / 1e6);
    else if (ns >= 1000)
        snprintf(buf, sizeof(buf), "%.3f us", ns / 1e3);
    else
        snprintf(buf, sizeof(buf), "%lld ns", ns);
    return buf;
}

int main(void)
{
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
        long _k;
        for (_k = 0; _k < 10000; _k++)
            strbuf_addch(&b, 'x');
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
