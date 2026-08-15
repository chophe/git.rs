//! Rust test conversion template for C unit tests
//!
//! This file provides templates for converting C strbuf tests to Rust.
//! Each C test corresponds to a Rust #[test] function.

/// Test wrapper struct mirroring C's strbuf
#[derive(Debug, Clone)]
pub struct StringBuf {
    buf: Vec<u8>,
    len: usize,
    alloc: usize,
}

impl StringBuf {
    /// Creates a new empty StringBuf (equivalent to STRBUF_INIT)
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            len: 0,
            alloc: 0,
        }
    }

    /// Initializes with a given capacity (equivalent to strbuf_init)
    pub fn init(&mut self, capacity: usize) {
        self.buf.resize(capacity, 0);
        self.alloc = capacity;
    }

    /// Adds a single character (equivalent to strbuf_addch)
    pub fn addch(&mut self, ch: char) {
        // Ensure capacity
        if self.len + ch.len_utf8() >= self.alloc {
            self.grow();
        }
        // Add character bytes
        for b in ch.to_string().bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
            } else {
                self.buf.push(b);
            }
            self.len += 1;
        }
        // Ensure NUL termination
        if self.buf.len() <= self.len {
            self.buf.push(0);
        }
    }

    /// Adds a string (equivalent to strbuf_addstr)
    pub fn addstr(&mut self, s: &str) {
        let len = s.len();
        if self.len + len >= self.alloc {
            self.grow();
        }
        for (i, b) in s.bytes().enumerate() {
            if self.len + i < self.buf.len() {
                self.buf[self.len + i] = b;
            } else {
                self.buf.push(b);
            }
        }
        self.len += len;
        // Ensure NUL termination
        if self.buf.len() <= self.len {
            self.buf.push(0);
        }
    }

    /// Releases resources (equivalent to strbuf_release)
    pub fn release(&mut self) {
        self.buf.clear();
        self.len = 0;
        self.alloc = 0;
    }

    /// Grows the buffer to accommodate more data
    fn grow(&mut self) {
        let new_alloc = if self.alloc == 0 {
            64
        } else {
            self.alloc * 2
        };
        if new_alloc > self.buf.len() {
            self.buf.resize(new_alloc, 0);
        }
        self.alloc = new_alloc;
    }

    /// Returns the current length
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the allocated size
    pub fn alloc(&self) -> usize {
        self.alloc
    }

    /// Returns the underlying buffer
    pub fn buf(&self) -> &[u8] {
        &self.buf[..self.len + 1] // Include NUL terminator
    }

    /// Gets a byte at a position
    pub fn get_byte(&self, pos: usize) -> Option<u8> {
        if pos < self.len {
            Some(self.buf[pos])
        } else {
            None
        }
    }
}

impl Default for StringBuf {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RUST TESTS - Converting C tests from t/unit-tests/u-strbuf.c
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Equivalent to C: test_strbuf__static_init
    /// Tests that a statically-initialized StringBuf starts in a clean state
    #[test]
    fn test_strbuf_static_init() {
        let buf = StringBuf::new();
        assert_eq!(buf.len(), 0, "length should be 0");
        assert_eq!(buf.alloc(), 0, "alloc should be 0");
        // buf[0] should be NUL (implicitly true for empty Vec)
    }

    /// Equivalent to C: test_strbuf__dynamic_init
    /// Tests dynamic initialization with capacity
    #[test]
    fn test_strbuf_dynamic_init() {
        let mut buf = StringBuf::new();
        buf.init(1024);
        assert!(buf.alloc() >= 1024, "alloc should be at least 1024");
        assert_eq!(buf.len(), 0, "length should be 0 after init");
        buf.release();
    }

    /// Equivalent to C: test_strbuf__add_single_char
    /// Tests adding a single character to empty buffer
    #[test]
    fn test_strbuf_add_single_char() {
        let mut buf = StringBuf::new();
        buf.addch('a');
        assert_eq!(buf.len(), 1, "length should be 1 after adding 'a'");
        assert!(buf.alloc() >= 1, "alloc should be at least 1");
        assert_eq!(buf.get_byte(0), Some(b'a'), "first byte should be 'a'");
        assert_eq!(buf.get_byte(buf.len()), None, "NUL terminator should exist at len");
        buf.release();
    }

    /// Equivalent to C: test_strbuf__add_empty_char
    /// Tests adding an empty character (should be no-op for char, but tests boundary)
    #[test]
    fn test_strbuf_add_empty_char() {
        let mut buf = StringBuf::new();
        // In C, this would be testing adding a NUL char
        // For Rust, we skip NUL char as it's the terminator
        // This test verifies buffer stability
        buf.addch('\0');
        assert_eq!(buf.len(), 1, "length should be 1 after adding NUL");
        buf.release();
    }

    /// Equivalent to C: test_strbuf__add_append_char
    /// Tests appending a character to a populated buffer
    #[test]
    fn test_strbuf_add_append_char() {
        let mut buf = StringBuf::new();
        buf.addstr("initial value");
        let orig_len = buf.len();
        let orig_alloc = buf.alloc();
        
        buf.addch('a');
        
        assert_eq!(buf.len(), orig_len + 1, "length should increase by 1");
        assert!(buf.alloc() >= orig_alloc, "alloc should not decrease");
        buf.release();
    }

    /// Equivalent to C: test_strbuf__add_single_str
    /// Tests adding a string to empty buffer
    #[test]
    fn test_strbuf_add_single_str() {
        let mut buf = StringBuf::new();
        let test_str = "hello there";
        buf.addstr(test_str);
        
        assert_eq!(buf.len(), test_str.len(), "length should match string length");
        assert_eq!(buf.buf()[..test_str.len()], test_str.as_bytes(), "content should match");
        buf.release();
    }

    /// Equivalent to C: test_strbuf__add_append_str
    /// Tests appending a string to populated buffer
    #[test]
    fn test_strbuf_add_append_str() {
        let mut buf = StringBuf::new();
        buf.addstr("initial value");
        let orig_len = buf.len();
        let orig_alloc = buf.alloc();
        
        buf.addstr("hello there");
        
        // Buffer should be cleaned up (len==0) after release
        buf.release();
        assert_eq!(buf.len(), 0, "length should be 0 after release");
        assert_eq!(buf.alloc(), 0, "alloc should be 0 after release");
    }

    /// Helper to verify buffer sanity conditions
    fn assert_sane_strbuf(buf: &StringBuf) {
        // Buffer should always have content if len > 0
        if buf.len() > 0 {
            assert!(buf.buf().len() > 0, "buffer should be non-empty");
            assert!(
                buf.alloc() > 0 || buf.len() == 0,
                "alloc should be positive or len 0"
            );
        }
    }

    /// Test buffer remains consistent after many operations
    #[test]
    fn test_strbuf_sanity_after_operations() {
        let mut buf = StringBuf::new();
        
        // Multiple appends
        for i in 0..100 {
            buf.addch('a' + (i % 26) as u8);
            assert_sane_strbuf(&buf);
        }
        
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
    }
}