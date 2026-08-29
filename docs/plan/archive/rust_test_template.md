# Rust Test Template for C Conversion

## Template Structure
This template converts C unit tests to Rust, maintaining identical test results and coverage.

## Conversion Pattern
1. **Identify C test function** → Create equivalent Rust `#[test]` function
2. **Map C assertions** → Use `assert_eq!`, `assert!`, `assert_matches!` 
3. **Maintain test independence** → Each Rust test should be self-contained
4. **Run both test suites** → Ensure C and Rust produce same results

## Example: Converting strbuf Tests

### C Version (t/unit-tests/u-strbuf.c):
```c
void test_strbuf__add_single_char(void){
    setup(t_addch, "a");
}
```

### Rust Equivalent:
```rust
#[test]
fn test_strbuf_add_single_char() {
    let mut buf = StringBuf::new();  // Rust equivalent of STRBUF_INIT
    t_addch(&buf, "a");  // Rust wrapper for strbuf_addch
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.alloc, /* expected alloc */ );
    assert_eq!(buf.buf[buf.len], '\0');  // NUL termination
}
```

## Test Infrastructure

### 1. Rust Wrapper Types
```rust
#[derive(Debug)]
struct StringBuf {
    buf: Vec<u8>,
    len: usize,
    alloc: usize,
}

impl StringBuf {
    pub const fn new() -> Self {
        Self { buf: Vec::new(), len: 0, alloc: 0 }
    }
    
    pub fn init(&mut self, capacity: usize) {
        self.buf.resize(capacity, 0);
        self.alloc = capacity;
    }
    
    pub fn addch(&mut self, ch: char) {
        // Implementation matching C strbuf_addch
        // ... 
    }
    
    pub fn addstr(&mut self, s: &str) {
        // Implementation matching C strbuf_addstr
        // ...
    }
    
    pub fn release(&mut self) {
        self.buf.clear();
        self.len = 0;
        self.alloc = 0;
    }
}
```

### 2. Test Helper Functions
```rust
fn setup<F>(f: F, data: &str)
where
    F: FnMut(&mut StringBuf, &str),
{
    let mut buf = StringBuf::new();
    f(&mut buf, data);
    // Verify post-conditions matching C cl_assert_equal_i
    assert_eq!(buf.len, /* expected */ );
    assert_eq!(buf.alloc, /* expected */ );
    assert_eq!(buf.buf[buf.len], b'\0');
}

fn setup_populated<F>(f: F, init: &str, append: &str)
where
    F: FnMut(&mut StringBuf, &str),
{
    let mut buf = StringBuf::new();
    strbuf_addstr(&mut buf, init);  // Or equivalent
    f(&mut buf, append);
    // Verify: len should return to 0, alloc should be 0
    assert_eq!(buf.len, 0);
    assert_eq!(buf.alloc, 0);
}
```

### 3. Specific Test Conversions

#### test_strbuf__static_init
```rust
#[test]
fn test_strbuf_static_init() {
    let buf = StringBuf::new();
    assert_eq!(buf.len, 0);
    assert_eq!(buf.alloc, 0);
    assert_eq!(buf.buf[0], b'\0');
}
```

#### test_strbuf__dynamic_init
```rust
#[test]
fn test_strbuf_dynamic_init() {
    let mut buf = StringBuf::new();
    buf.init(1024);
    assert!(buf.alloc >= 1024);
    assert_eq!(buf.len, 0);
    assert_eq!(buf.buf[0], b'\0');
    buf.release();
}
```

#### test_strbuf__add_single_char
```rust
#[test]
fn test_strbuf_add_single_char() {
    let mut buf = StringBuf::new();
    t_addch(&mut buf, 'a');
    assert_eq!(buf.len, 1);
    assert!(buf.alloc >= 1);  // alloc should be at least 1
    assert_eq!(buf.buf[buf.len - 1], b'a');
    assert_eq!(buf.buf[buf.len], b'\0');  // NUL terminated
    buf.release();
}
```

## Running Tests

### Execute Rust Tests:
```bash
cargo test --lib  # or cargo test specific test name
```

### Compare with C Tests:
```bash
# Run C test
./unit-test -t strbuf  # or appropriate filter

# Verify same results
# Both should pass/fail identically
```

## Migration Strategy

### Phase 1: Critical Path Tests
Convert the most-used C test functions first:
- strbuf initialization and manipulation
- String list operations
- Hash map operations  
- ODB operations

### Phase 2: Full Coverage
Convert remaining test functions to ensure:
- 100% test coverage match
- Identical pass/fail results
- No regression in functionality

### Phase 3: Automation
- Add to CI pipeline
- Monitor coverage metrics
- Maintain synchronization between C and Rust test suites

## Verification Checklist
- [ ] All critical C test functions have Rust equivalents
- [ ] Test results match (pass/fail identical)
- [ ] No performance regression
- [ ] CI integration complete
- [ ] Coverage metrics documented