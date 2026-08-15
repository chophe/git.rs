//! A string buffer mirroring git's C `strbuf` API.
//!
//! This module implements the same semantics as `strbuf.c`/`strbuf.h` so that
//! the Rust port produces identical results. The invariants match the C one:
//!
//! - `buf` is never NULL and always NUL-terminated (we store an explicit NUL
//!   padding byte at the end of the backing `Vec`).
//! - `alloc` is the capacity of the backing store (bytes), and is at least
//!   `len + 1` whenever the buffer holds data.
//! - `len` is the current logical length, excluding the terminating NUL.

/// A growable, NUL-terminated byte buffer.
#[derive(Debug, Clone)]
pub struct StringBuf {
    /// Backing storage. `len` leading bytes are live data; we keep one extra
    /// NUL slot at the end be maintained by the mutators.
    data: Vec<u8>,
    /// Current logical length in bytes (excluding the trailing NUL).
    len: usize,
}

impl StringBuf {
    /// Equivalent to `STRBUF_INIT` / a fresh, empty strbuf.
    pub fn new() -> Self {
        // Reserve one NUL slot so `buf[0]` is always readable.
        StringBuf {
            data: vec![0],
            len: 0,
        }
    }

    /// Equivalent to `strbuf_init(sb, hint)`: allocate `hint` bytes up front
    /// to avoid later reallocs.
    pub fn init(&mut self, hint: usize) {
        if hint > 0 {
            self.grow(hint);
        }
    }

    /// Equivalent to `strbuf_release()`: free the backing memory and reset to
    /// the freshly-initialised state.
    pub fn release(&mut self) {
        self.data = vec![0];
        self.len = 0;
    }

    /// Equivalent to `strbuf_reset()`: empty the buffer without freeing.
    pub fn reset(&mut self) {
        self.len = 0;
        self.data[0] = 0;
    }

    /// Ensure at least `amount` unused bytes are available after `len`.
    /// Equivalent to `strbuf_grow(sb, amount)`.
    pub fn grow(&mut self, amount: usize) {
        let needed = self.len + amount + 1; // +1 for the NUL
        if needed > self.data.len() {
            let mut new_cap = self.data.len().max(32);
            while new_cap < needed {
                new_cap *= 2;
            }
            self.data.resize(new_cap, 0);
        }
    }

    /// Available, unused capacity (equal to `strbuf_avail()`).
    pub fn avail(&self) -> usize {
        self.data.len().saturating_sub(self.len + 1)
    }

    /// Current length, excluding the terminating NUL. Equal to `sb->len`.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Allocated size (equal to `sb->alloc`).
    pub fn alloc(&self) -> usize {
        self.data.len()
    }

    /// `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Borrow the contents as raw bytes, excluding the NUL terminator.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Borrow the whole backing store including the trailing NUL.
    pub fn buf(&self) -> &[u8] {
        &self.data[..=self.len]
    }

    /// Return the backing store as a UTF-8 string view (best effort).
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
    }

    /// Set the length directly (`strbuf_setlen`). Caller must ensure the new
    /// length does not exceed `len + avail()`.
    pub fn setlen(&mut self, len: usize) {
        assert!(
            len <= self.alloc().saturating_sub(1),
            "strbuf_setlen() beyond buffer"
        );
        self.len = len;
        self.data[len] = 0;
    }

    /// Append a single character (`strbuf_addch`).
    pub fn addch(&mut self, byte: u8) {
        if self.avail() == 0 {
            self.grow(1);
        }
        self.data[self.len] = byte;
        self.len += 1;
        self.data[self.len] = 0;
    }

    /// Append `n` copies of a character (`strbuf_addchars`).
    pub fn addchars(&mut self, byte: u8, n: usize) {
        let start = self.len;
        self.grow(n);
        for i in 0..n {
            self.data[start + i] = byte;
        }
        self.len += n;
        self.data[self.len] = 0;
    }

    /// Append `len` bytes of raw data (`strbuf_add`).
    pub fn add(&mut self, data: &[u8]) {
        self.grow(data.len());
        self.data[self.len..self.len + data.len()].copy_from_slice(data);
        self.len += data.len();
        self.data[self.len] = 0;
    }

    /// Append a NUL-terminated string (`strbuf_addstr`).
    pub fn addstr(&mut self, s: &str) {
        self.add(s.as_bytes());
    }

    /// Append the contents of another buffer (`strbuf_addbuf`).
    pub fn addbuf(&mut self, other: &StringBuf) {
        self.add(other.as_bytes());
    }

    /// Compare two buffers (`strbuf_cmp`): negative, zero, or positive.
    pub fn cmp(&self, other: &StringBuf) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }

    /// Trim trailing whitespace (`strbuf_rtrim`).
    pub fn rtrim(&mut self) {
        while self.len > 0 && is_ascii_space(self.data[self.len - 1]) {
            self.len -= 1;
        }
        self.data[self.len] = 0;
    }

    /// Trim leading whitespace (`strbuf_ltrim`).
    pub fn ltrim(&mut self) {
        let mut start = 0;
        while start < self.len && is_ascii_space(self.data[start]) {
            start += 1;
        }
        if start > 0 {
            self.data.copy_within(start..self.len, 0);
            self.len -= start;
        }
        self.data[self.len] = 0;
    }

    /// Trim both ends (`strbuf_trim`).
    pub fn trim(&mut self) {
        self.rtrim();
        self.ltrim();
    }

    /// Convert the whole buffer to lowercase (`strbuf_tolower`).
    pub fn tolower(&mut self) {
        for b in &mut self.data[..self.len] {
            *b = b.to_ascii_lowercase();
        }
    }

    /// Append a formatted string, well-known subset of `strbuf_addf` that
    /// interpolates `{}` placeholders.
    pub fn addf(&mut self, fmt: &str, args: &[&str]) {
        let mut rest = fmt;
        for arg in args {
            match rest.find("{}") {
                Some(pos) => {
                    self.addstr(&rest[..pos]);
                    self.addstr(arg);
                    rest = &rest[pos + 2..];
                }
                None => break,
            }
        }
        self.addstr(rest);
    }

    /// Last byte, if any (sugar for trimming logic).
    pub fn last_byte(&self) -> Option<u8> {
        if self.len == 0 {
            None
        } else {
            Some(self.data[self.len - 1])
        }
    }
}

impl Default for StringBuf {
    fn default() -> Self {
        StringBuf::new()
    }
}

impl From<&str> for StringBuf {
    fn from(s: &str) -> Self {
        let mut sb = StringBuf::new();
        sb.addstr(s);
        sb
    }
}

impl std::fmt::Display for StringBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_bytes()))
    }
}

fn is_ascii_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == b'\x0b' || b == b'\x0c'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_init() {
        let buf = StringBuf::new();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 1);
        assert_eq!(buf.as_bytes(), b"");
    }

    #[test]
    fn dynamic_init() {
        let mut buf = StringBuf::new();
        buf.init(1024);
        assert!(buf.alloc() >= 1024);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn add_single_char() {
        let mut buf = StringBuf::new();
        let orig = buf.alloc();
        buf.addch(b'a');
        assert_eq!(buf.len(), 1);
        assert!(buf.alloc() >= orig);
        assert_eq!(buf.buf(), b"a\0");
    }

    #[test]
    fn add_and_release() {
        let mut buf = StringBuf::new();
        buf.addstr("hello there");
        assert_eq!(buf.len(), 11);
        assert_eq!(buf.as_str(), "hello there");
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 1);
    }

    #[test]
    fn append_char() {
        let mut buf = StringBuf::from("initial value");
        let orig_len = buf.len();
        let orig_alloc = buf.alloc();
        buf.addch(b'a');
        assert_eq!(buf.len(), orig_len + 1);
        assert!(buf.alloc() >= orig_alloc);
        assert_eq!(buf.as_bytes()[buf.len() - 1], b'a');
    }

    #[test]
    fn reset_keeps_capacity() {
        let mut buf = StringBuf::from("some content");
        let cap = buf.alloc();
        buf.reset();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), cap);
        assert_eq!(buf.as_bytes(), b"");
    }

    #[test]
    fn rtrim_whitespace() {
        let mut buf = StringBuf::from("  padded text  \n");
        buf.rtrim();
        assert_eq!(buf.as_str(), "  padded text");
        buf.ltrim();
        assert_eq!(buf.as_str(), "padded text");
    }

    #[test]
    fn tolower_converts() {
        let mut buf = StringBuf::from("HeLLo WoRLD");
        buf.tolower();
        assert_eq!(buf.as_str(), "hello world");
    }
}
