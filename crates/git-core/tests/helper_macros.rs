// Helper macros and utilities for C-Rust equivalence
// These bridge the semantic gaps between C's pointer-based tests and Rust's memory-safe approach

/// Macro to simulate C's setup() helper pattern
/// C: static void setup(void (*f)(struct strbuf*, const void*), const void *data)
macro_rules! c_setup {
    // Empty buffer setup
    (|$buf:ident, $f:ident, $data:expr|) => {
        let mut $buf = StringBuf::new();
        $f(&mut $buf, $data);
        $buf
    };
    
    // Populated buffer setup (with initial string)
    (|$buf:ident, $f:ident, $init:expr, $data:expr|) => {
        let mut $buf = StringBuf::new();
        $buf.addstr($init);
        $f(&mut $buf, $data);
        $buf.release();
        $buf
    };
}

/// Macro to simulate C's cl_assert_equal_i for integer comparisons
macro_rules! c_assert_equal_i {
    ($left:expr, $right:expr) => {
        assert_eq!($left, $right, 
            "C assert_equal_i failed: {} != {} at line {}", 
            $left, $right, line!())
    };
}

/// Macro to simulate C's cl_assert_equal_s for string comparisons
macro_rules! c_assert_equal_s {
    ($left:expr, $right:expr) => {
        assert_eq!($left, $right,
            "C assert_equal_s failed: {} != {} at line {}",
            $left, $right, line!())
    };
}

/// Macro to simulate C's cl_assert for boolean checks
macro_rules! c_assert {
    ($condition:expr) => {
        if !$condition {
            panic!("C assert failed at line {}", line!())
        }
    };
}

/// Utility function to simulate C's strbuf_addch with macro flexibility
fn c_strbuf_addch(buf: &mut StringBuf, ch: u8) {
    buf.addch(ch as char);
}

/// Utility function to simulate C's strbuf_addstr with macro flexibility
fn c_strbuf_addstr(buf: &mut StringBuf, s: &str) {
    buf.addstr(s);
}

/// Performance comparison utilities
pub struct PerfResult {
    pub time_ns: u128,
    pub allocations: usize,
    pub copies: usize,
}

impl PerfResult {
    /// Calculates performance ratio between C and Rust implementations
    pub fn ratio_vs_c(&self, c_result: &PerfResult) -> f64 {
        if c_result.time_ns == 0 { return 0.0; }
        self.time_ns as f64 / c_result.time_ns as f64
    }
    
    /// Formats performance results for comparison
    pub fn format_comparison(&self, c_result: &PerfResult) -> String {
        format!(
            "C: {:.2}µs, Rust: {:.2}µs (x{:.2}), C: {} alloc, Rust: {} alloc",
            c_result.time_ns as f64 / 1000.0,
            self.time_ns as f64 / 1000.0,
            self.ratio_vs_c(c_result),
            c_result.allocations,
            self.allocations
        )
    }
}

/// Macro to create comprehensive test suite for C-Rust equivalence
/// Maps each C test function to equivalent Rust behavior
macro_rules! create_crust_test_suite {
    (
        test_name: $test_name:expr,
        c_tests: [
            $([
                $test_func_name:expr,
                $c_behavior:expr
            ]),* $(,)?
        ],
        rust_tests: [
            $([
                $rust_func_name:expr,
                $rust_behavior:expr
            ]),* $(,)?
        ]
    ) => {
        /// Comprehensive test suite ensuring C-Rust equivalence
        #[cfg(test)]
        mod $test_name {
            use super::*;
            
            // Test each C function equivalent
            $(
                #[test]
                fn $test_func_name() {
                    // Run C behavior simulation
                    let c_result = $c_behavior();
                    
                    // Run Rust behavior simulation  
                    let rust_result = $rust_behavior();
                    
                    // Verify equivalence
                    assert_eq!(c_result.time_ns, rust_result.time_ns, 
                        "Time complexity mismatch for {}", $test_func_name);
                    assert_eq!(c_result.allocations, rust_result.allocations,
                        "Memory allocation mismatch for {}", $test_func_name);
                    assert_eq!(c_result.copies, rust_result.copies,
                        "Data copy mismatch for {}", $test_func_name);
                }
            )*
            
            // Test each Rust-specific behavior
            $(
                #[test]
                fn $rust_func_name() {
                    $rust_behavior;
                }
            )*
        }
    };
}