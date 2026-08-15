# Rust Conversion Test Plan for C Language Tests

## 1. Overview
This document outlines a plan to implement comprehensive Rust tests for the existing C language test suite in the `git.rs` repository. The goal is to ensure equivalent functionality and coverage in Rust while maintaining compatibility with the existing C test framework.

## 2. Current Test Structure (C)
- Tests located in `t/unit-tests/` directory, primarily in `unit-test.c`
- Uses a custom `clar_test` macro/framework for test execution
- Supports filtering via `-s` suffixes and exclusion via `-x` suffixes
- Includes extensive utility tests (strings, lists, trees, hash maps, etc.)

## 3. Conversion Approach
### 3.1 Identify Corresponding Rust Functionality
Map existing C test cases to Rust equivalents:
- String operations (`u-strvec.c`) → Rust `String`/`Vec<String>` manipulation
- List/tree operations (`u-reftable-*.c`) → Rust collections and custom structs
- ODB operations → Rust wrappers around database internals

### 3.2 Add Rust Test Files
Create `tests/unit_test.rs` for new Rust tests:
- Use standard Rust `#[test]` attribute
- Replicate C test scenarios using `assert_eq!`, `assert_matches!`

```rust
#[test]
fn test_string_vec_initialization() {
    let mut vec = StringVec::new();
    assert_eq!(vec.len(), 0);
    vec.push("test".to_string());
    assert_eq!(vec.len(), 1);
}
```

### 3.3 Integration with Build System
Add Rust tests to `Cargo.toml` test section:
```toml
[[bin]]
name = "git-core"
path = "src/main.rs"

[tests]
unit_test = "tests/unit_test.rs"
```

Or use the standard approach:
```bash
cargo test --lib --unit-test
```

### 3.4 Running Tests
- Execute Rust tests independently:
  ```bash
  cargo test --unit-test
  ```
- Run alongside C tests:
  ```bash
  ./unit-test -t --only-unit-test   # Assuming custom flag added
  ```

### 3.5 Validation and Coverage
- Run full test suite to ensure no regression:
  ```bash
  ./unit-test | grep -E "(PASS|FAIL)"
  cargo test
  ```
- Use coverage tools (e.g., `lcov` via `cargo-llvm-cov`) to compare coverage with original C tests.

### 3.6 Automation
- Add Rust tests to CI pipeline:
  ```yaml
  - name: Run Rust Tests
    run: cargo test --lib --quiet
  ```
- Monitor passing tests and adjust implementations accordingly.

## 5. Subtasks
- [ ] Analyze existing C test cases to identify Rust equivalents.
- [ ] Implement Rust versions of utility functions (e.g., `StringVec`, hash maps).
- [ ] Create comprehensive Rust test file with multiple test cases.
- [ ] Integrate Rust tests into the build system.
- [ ] Run and verify tests pass alongside C tests.
- [ ] Add coverage analysis to ensure sufficient test coverage.
- [ ] Optimize test runtime and maintain test independence.

## 6. Risks & Dependencies
- Dependencies: None beyond Rust toolchain and existing test utilities.
- Risks: Incomplete test coverage; need to monitor divergence between C and Rust behaviors.

## 7. Next Steps
1. Analyze specific C test cases for Rust equivalents.
2. Generate starter Rust test template.
3. Implement conversion of selected C test files.

*Generated on:* $(date)