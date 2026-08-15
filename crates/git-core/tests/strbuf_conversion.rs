#[cfg(test)]
mod strbuf_tests {
    use super::*;

    // Helper to create test buffer with specific initialization
    fn setup<F>(f: F, data: &str) -> StringBuf
    where
        F: FnOnce(&mut StringBuf, &str),
    {
        let mut buf = StringBuf::new();
        f(&mut buf, data);
        buf
    }

    // Test 1: Static initialization
    #[test]
    fn test_strbuf_static_init() {
        let buf = StringBuf::new();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
        assert_eq!(buf.get_byte(0), Some(0));
    }

    // Test 2: Dynamic initialization with capacity
    #[test]
    fn test_strbuf_dynamic_init() {
        let mut buf = StringBuf::new();
        buf.init(1024);
        assert!(buf.alloc() >= 1024);
        assert_eq!(buf.len(), 0);
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
    }

    // Test 3: Single character addition
    #[test]
    fn test_strbuf_add_single_char() {
        let mut buf = StringBuf::new();
        buf.addch('a');
        assert_eq!(buf.len(), 1);
        assert!(buf.alloc() >= 1);
        assert_eq!(buf.get_byte(0), Some(b'a'));
        assert_eq!(buf.get_byte(buf.len()), None); // NUL terminator
        buf.release();
    }

    // Test 4: Empty character addition
    #[test]
    fn test_strbuf_add_empty_char() {
        let mut buf = StringBuf::new();
        buf.addch(0);
        assert_eq!(buf.len(), 1);
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
    }

    // Test 5: Appending to existing content
    #[test]
    fn test_strbuf_add_append_char() {
        let mut buf = StringBuf::new();
        buf.addstr("initial value");
        let orig_len = buf.len();
        let orig_alloc = buf.alloc();
        
        buf.addch('a');
        
        assert_eq!(buf.len(), orig_len + 1);
        assert!(buf.alloc() >= orig_alloc);
        buf.release();
    }

    // Test 6: Single string replacement
    #[test]
    fn test_strbuf_add_single_str() {
        let mut buf = StringBuf::new();
        let test_str = "hello there";
        buf.addstr(test_str);
        
        assert_eq!(buf.len(), test_str.len());
        assert_eq!(buf.buf()[..test_str.len()], test_str.as_bytes());
        buf.release();
    }

    // Test 7: Appended string replacement
    #[test]
    fn test_strbuf_add_append_str() {
        let mut buf = StringBuf::new();
        buf.addstr("initial value");
        let orig_len = buf.len();
        let orig_alloc = buf.alloc();
        
        buf.addstr("hello there");
        
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
    }

    // Test 8: Multiple operations consistency
    #[test]
    fn test_strbuf_sanity_after_operations() {
        let mut buf = StringBuf::new();
        
        // Multiple appends
        for i in 0..100 {
            buf.addch('a' + (i % 26) as u8);
            // Basic sanity checks
            assert!(buf.len() <= buf.alloc());
        }
        
        buf.release();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.alloc(), 0);
    }
}
