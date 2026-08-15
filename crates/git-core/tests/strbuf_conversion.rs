//! Conversion of `t/unit-tests/u-strbuf.c` to Rust integration tests.
//!
//! These tests exercise the public `StringBuf` API exported by the Rust crate
//! and assert the same invariants the C unit tests check (length, allocation,
//! NUL-termination, and content).

use git_core::StringBuf;

/// Equivalent of the C `assert_sane_strbuf` helper: a live buffer always
/// points at NUL-terminated data and `alloc >= len + 1` when it holds bytes.
fn assert_sane_strbuf(buf: &StringBuf) {
    // The backing store is always NUL-terminated at index `len`.
    assert_eq!(buf.buf()[buf.len()], 0);
    // Whenever data is present, allocation is at least len + 1.
    if buf.len() > 0 {
        assert!(buf.len() < buf.alloc());
    }
}

#[test]
fn test_strbuf_static_init() {
    let buf = StringBuf::new();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.alloc(), 1); // slopbuf: 1 NUL byte
    assert_eq!(buf.buf(), b"\0");
}

#[test]
fn test_strbuf_dynamic_init() {
    let mut buf = StringBuf::new();
    buf.init(1024);
    assert!(buf.alloc() >= 1024);
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.buf()[0], 0);
    buf.release();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.alloc(), 1);
}

#[test]
fn test_strbuf_add_single_char() {
    let mut buf = StringBuf::new();
    let orig_alloc = buf.alloc();
    buf.addch(b'a');
    assert_sane_strbuf(&buf);
    assert_eq!(buf.len(), 1);
    assert!(buf.alloc() >= orig_alloc);
    assert_eq!(buf.buf(), b"a\0");
}

#[test]
fn test_strbuf_add_empty_char() {
    // Adding NUL is allowed; the buffer ends up with one NUL data byte, and a
    // trailing terminator.
    let mut buf = StringBuf::new();
    buf.addch(0);
    assert_sane_strbuf(&buf);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.buf(), b"\0\0");
    buf.release();
}

#[test]
fn test_strbuf_add_append_char() {
    let mut buf = StringBuf::from("initial value");
    let orig_len = buf.len();
    let orig_alloc = buf.alloc();
    buf.addch(b'a');
    assert_sane_strbuf(&buf);
    assert_eq!(buf.len(), orig_len + 1);
    assert!(buf.alloc() >= orig_alloc);
    assert_eq!(buf.as_bytes()[buf.len() - 1], b'a');
}

#[test]
fn test_strbuf_add_single_str() {
    let text = "hello there";
    let mut buf = StringBuf::new();
    buf.addstr(text);
    assert_sane_strbuf(&buf);
    assert_eq!(buf.len(), text.len());
    assert_eq!(buf.as_str(), text);
}

#[test]
fn test_strbuf_add_append_str() {
    let mut buf = StringBuf::from("initial value");
    buf.addstr("hello there");
    assert_sane_strbuf(&buf);
    assert_eq!(buf.as_str(), "initial valuehello there");
}

#[test]
fn test_strbuf_release_clears() {
    let mut buf = StringBuf::from("testing");
    buf.addstr(" more");
    assert_eq!(buf.len(), 12);
    buf.release();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.alloc(), 1);
    assert_eq!(buf.buf(), b"\0");
}

#[test]
fn test_strbuf_reset_keeps_alloc() {
    let mut buf = StringBuf::from("keep capacity");
    let cap = buf.alloc();
    assert!(cap > 1);
    buf.reset();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.alloc(), cap);
    assert_eq!(buf.as_bytes(), b"");
    // Still usable after reset.
    buf.addstr("new");
    assert_eq!(buf.as_str(), "new");
}

#[test]
fn test_strbuf_many_appends_stay_consistent() {
    let mut buf = StringBuf::new();
    for i in 0..1000u32 {
        buf.addstr(&format!("{i}-"));
        assert_sane_strbuf(&buf);
        assert!(buf.len() <= buf.alloc());
    }
    assert!(buf.len() > 0);
    buf.release();
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.alloc(), 1);
}
