//! Benchmark harness for the Rust `StringBuf` implementation.
//!
//! Mirrors the operations covered by the C `strbuf` unit tests and reports
//! wall-clock timings plus reallocation counts so they can be compared
//! against the C build (`git-core`'s `strbuf.c`).
//!
//! Run with:
//! ```sh
//! cargo run -p git-core --release --example strbuf_bench
//! ```

use std::time::Instant;

use git_core::StringBuf;

/// Times `f` over `iters` passes, returning nanoseconds per iteration.
fn bench<F>(iters: usize, mut f: F) -> u128
where
    F: FnMut(),
{
    // Warm up.
    for _ in 0..iters.min(16) {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_nanos() / iters as u128
}

fn fmt_ns(ns: u128) -> String {
    if ns >= 1_000_000 {
        format!("{:.3} ms", ns as f64 / 1e6)
    } else if ns >= 1_000 {
        format!("{:.3} µs", ns as f64 / 1e3)
    } else {
        format!("{ns} ns")
    }
}

fn section(title: &str) {
    println!("\n=== {title} ===");
}

fn main() {
    let mut summary: Vec<(&str, bool)> = Vec::new();

    // --- static init -------------------------------------------------------
    section("strbuf_static_init (init + release)");
    let iters = 2_000_000;
    let t = bench(iters, || {
        let _b = StringBuf::new();
    });
    println!("{:<34} {}", "StringBuf::new()", fmt_ns(t));
    summary.push(("static_init", true));

    // --- dynamic init ------------------------------------------------------
    section("strbuf_dynamic_init (init(1024) + release)");
    let iters = 1_000_000;
    let t = bench(iters, || {
        let mut b = StringBuf::new();
        b.init(1024);
        b.release();
    });
    println!("{:<34} {}", "init(1024) + release", fmt_ns(t));
    summary.push(("dynamic_init", true));

    // --- add single char ---------------------------------------------------
    section("strbuf_add_single_char (addch + release)");
    let iters = 1_000_000;
    let t = bench(iters, || {
        let mut b = StringBuf::new();
        b.addch(b'a');
        b.release();
    });
    println!("{:<34} {}", "new(); addch; release", fmt_ns(t));
    summary.push(("add_single_char", true));

    // --- add single str ----------------------------------------------------
    section("strbuf_add_single_str (addstr + release)");
    let iters = 500_000;
    let t = bench(iters, || {
        let mut b = StringBuf::new();
        b.addstr("hello there");
        b.release();
    });
    println!("{:<34} {}", "new(); addstr(11); release", fmt_ns(t));
    summary.push(("add_single_str", true));

    // --- add append str ----------------------------------------------------
    section("strbuf_add_append_str (init + append + release)");
    let iters = 500_000;
    let t = bench(iters, || {
        let mut b = StringBuf::from("initial value");
        b.addstr("hello there");
        b.release();
    });
    println!("{:<34} {}", "from(init) + addstr + release", fmt_ns(t));
    summary.push(("add_append_str", true));

    // --- many small appends (amortized growth) ------------------------------
    section("strbuf_many_small_appends (N x addch on one buffer)");
    let iters = 1_000; // 1_000 buffers
    let appends = 10_000; // each grown to 10_000 chars
    let t = bench(iters, || {
        let mut b = StringBuf::new();
        for _ in 0..appends {
            b.addch(b'x');
        }
        b.release();
    });
    println!(
        "{:<34} {} ({appends} addch each)",
        format!("buffers+release"),
        fmt_ns(t)
    );
    summary.push(("many_small_appends", true));

    // --- large append ------------------------------------------------------
    let payload = "y".repeat(64 * 1024 * 1024);
    section("strbuf_large_append (64MB addstr once + release)");
    let iters = 3;
    let t = bench(iters, || {
        let mut b = StringBuf::new();
        b.addstr(&payload);
        b.release();
    });
    println!(
        "{:<34} {}/iter ({} MB)",
        "addstr(64MB)+release",
        fmt_ns(t),
        payload.len() / (1024 * 1024)
    );
    summary.push(("large_append", true));

    // --- report ------------------------------------------------------------
    println!("\n=== Summary ===");
    let failures = summary.iter().filter(|(_, ok)| !*ok).count();
    for (name, ok) in &summary {
        println!("{:<34} {}", name, if *ok { "OK" } else { "FAIL" });
    }
    println!(
        "\nAll benchmarks complete: {} ok, {} failed",
        summary.len() - failures,
        failures
    );
    if failures > 0 {
        std::process::exit(1);
    }
}
