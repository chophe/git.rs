---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# Testing Patterns

**Analysis Date:** 2026-09-25

## Test Framework

**Runner:**

- `cargo test` (Rust built-in test harness; edition 2021, rust-version 1.74 per `crates/Cargo.toml`). No custom runner, no async runtime.
- Config: no `jest/vitest` equivalent, no `[lib] test = false` overrides, no `nextest` config — each crate's tests are the default `#[test]` harness plus `proptest` cases. Workspace manifest: `crates/Cargo.toml`.
- Property framework: `proptest = "1"` as `[dev-dependencies]` in `crates/git-hash/Cargo.toml`, `crates/git-varint/Cargo.toml`, `crates/git-date/Cargo.toml`, `crates/git-config/Cargo.toml`, `crates/git-object/Cargo.toml`, `crates/git-odb/Cargo.toml`, `crates/git-compress/Cargo.toml`, `crates/git-credentials/Cargo.toml`, `crates/git-pathspec/Cargo.toml`, `crates/git-pretty/Cargo.toml`, `crates/git-attributes/Cargo.toml`, `crates/git-index/Cargo.toml`, `crates/git-revision/Cargo.toml`.
- No mocking frameworks (`mockall`, `mockito`), no snapshot frameworks (`insta`, `snapbox`), no `tempfile`/`assert_cmd` helpers — tests hand-roll tempdirs with `std::env::temp_dir()` + `AtomicU32` counters and shell out to the real system git as the oracle.
- CI: `.github/workflows/rust-port.yml` with three jobs — `unit` (`cargo test --workspace --manifest-path crates/Cargo.toml` + clippy), `differential` (`cargo run -p xtask --manifest-path crates/Cargo.toml -- differential`), `scoreboard` (`... -- scoreboard`).

**Assertion Library:**

- Standard `assert!` / `assert_eq!` / `assert!(matches!(...))` for unit and integration tests; `prop_assert_eq!` / bare `let _ =` no-panic calls inside `proptest!` blocks.

**Run Commands:**

```bash
cd crates && cargo test --workspace              # Run all tests (unit + property + integration)
cd crates && cargo test -p git-hash              # Run one crate's tests
cd crates && cargo test -p git-odb --test pack_crosswise            # One crosswise suite
cd crates && cargo test -p git-command --test phaseA01_crosswise    # One phase suite
cd crates && cargo xtask test                    # Same as cargo test --workspace (via crates/.cargo/config.toml alias)
cd crates && cargo xtask differential            # All crosswise suites vs system C git
cd crates && cargo xtask scoreboard              # Differential + rewrite crates/scoreboard.json, fail on regression
cd crates && cargo xtask gen-fixtures            # Regenerate crates/tests/fixtures with system git
cd crates && cargo xtask gates                   # Structural gates: depcheck + safety + placement
cd crates && cargo xtask isolation               # Unit tests in bare dir with scrubbed env
```

## Test File Organization

**Location:**

- Unit tests are co-located inline: `#[cfg(test)] mod tests` at the bottom of the source file (`crates/git-hash/src/lib.rs`, `crates/git-compress/src/lib.rs`, `crates/git-core/src/strbuf.rs`, `crates/git-cli/src/lib.rs`).
- Property tests are either an inline `#[cfg(test)] mod props` in the same file (`crates/git-hash/src/lib.rs`, `crates/git-compress/src/lib.rs`) or a dedicated integration file (`crates/git-attributes/tests/wildmatch_proptest.rs`).
- Differential/compat tests live in separate integration targets under `crates/*/tests/`: `crates/git-odb/tests/pack_crosswise.rs`, `crates/git-odb/tests/graph_midx_crosswise.rs`, `crates/git-command/tests/phase{4,5,6,7,8,9,10}_crosswise.rs`, `crates/git-command/tests/phaseA{01..13}_crosswise.rs`, `crates/git-command/tests/phaseB{01,03,04,05,06,07,08}_crosswise.rs`, `crates/git-command/tests/followups_crosswise.rs`, `crates/git-command/tests/plumbing_stability.rs`, `crates/git-command/tests/probe_command.rs`, `crates/git-core/tests/strbuf_conversion.rs`.

**Naming:**

- Integration file: `<topic>_<kind>.rs` where kind is `crosswise`, `proptest`, `stability`, or a C-test conversion name (`strbuf_conversion.rs` mirrors `t/unit-tests/u-strbuf.c`).
- Test function: descriptive `snake_case`: `reads_and_verifies_real_git_pack`, `real_git_reads_our_pack`, `hash_object_shattered_pdf_matches`, `no_panic_on_arbitrary_input`, `display_config_cannot_alter_plumbing_output` (`crates/git-odb/tests/pack_crosswise.rs`, `crates/git-command/tests/phaseA01_crosswise.rs`, `crates/git-command/tests/plumbing_stability.rs`).

**Structure:**

```
crates/
├── Cargo.toml                      # workspace (run cargo from crates/)
├── .cargo/config.toml              # xtask alias
├── scoreboard.json                 # committed crosswise regression baseline
├── tests/fixtures/                 # golden fixtures (xtask gen-fixtures)
├── git-<crate>/src/*.rs            # inline `mod tests` + `mod props` at file bottom
└── git-<crate>/tests/*.rs          # integration suites (one file per suite)
```

## Test Structure

**Suite Organization:**

- Inline unit suites follow this shape (`crates/git-compress/src/lib.rs`, `crates/git-core/src/strbuf.rs`, `crates/git-cli/src/lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_round_trip() {
        let data = b"hello world, hello world, hello world";
        let enc = encode_all(data);
        assert_eq!(decode_all(&enc).unwrap(), data);
    }

    #[test]
    fn bounded_rejects_oversize_output() {
        let enc = encode_all(&vec![7u8; 100_000]);
        let err = decode_bounded(&enc, 10).unwrap_err();
        assert_eq!(err, CompressError::TooLarge { limit: 10 });
    }
}
```

- Property suites follow this shape (`crates/git-compress/src/lib.rs`, `crates/git-hash/src/lib.rs`, `crates/git-attributes/tests/wildmatch_proptest.rs`):

```rust
#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// One-shot encode/decode round-trips arbitrary bytes at every level.
        #[test]
        fn round_trip(data: Vec<u8>, level in 0u32..12u32) {
            let enc = encode_all_level(&data, level);
            let back = decode_bounded(&enc, (data.len() as u64).saturating_add(16)).unwrap();
            prop_assert_eq!(back, data);
        }

        /// Decoding arbitrary bytes never panics.
        #[test]
        fn decode_never_panics(data: Vec<u8>) {
            let _ = decode_all(&data);
            let _ = decode_bounded(&data, 1024);
        }
    }
}
```

- Crosswise suites follow this shape (`crates/git-command/tests/phaseA01_crosswise.rs`, `crates/git-odb/tests/pack_crosswise.rs`): helper fns (`git()`, `tempdir()`, `real()`, `ours()` / `run()`), then one `#[test]` per behavior asserting identical exit code plus identical combined stdout+stderr:

```rust
#[test]
fn hash_object_shattered_pdf_matches() {
    if git().is_none() {
        return; // skip when no system git is available
    }
    let dir = tempdir();
    let pdf = repo_root().join("t/t0013/shattered-1.pdf").to_string_lossy().into_owned();
    let (rtext, rcode) = real(&dir, &["hash-object", &pdf]);
    let (otext, ocode) = ours(&dir, &["hash-object", &pdf]);
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}");
}
```

**Patterns:**

- Setup pattern: build a fresh tempdir per test with a process-scoped `AtomicU32` counter, seed it with the system git binary (`init`, `config user.*`, `add`, `commit`, `repack -ad`), then exercise the Rust code against it. Always `canonicalize()` the tempdir (`crates/git-odb/tests/pack_crosswise.rs::tempdir`, `crates/git-command/tests/phaseA01_crosswise.rs::tempdir`).
- Teardown pattern: `let _ = std::fs::remove_dir_all(&dir);` at the end of the test (best-effort, no assert). `tempdir()` itself removes any stale dir before creating (`crates/git-odb/tests/pack_crosswise.rs`).
- Assertion pattern: compare exit codes first, then byte-identical combined output with both sides interpolated into the failure message (`assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}")`). For content checks compare `Vec<u8>` stdout directly (`assert_eq!(out.stdout, obj.data, ...)` in `crates/git-odb/tests/pack_crosswise.rs::odb_reads_from_real_pack`).
- Shared-mutation guard: tests that `set_current_dir` or touch process env serialize on a `static CWD_LOCK: Mutex<()>`; reuse `git_command::tests::with_cwd` / `serialized` (`crates/git-command/src/lib.rs`) or a per-file `static CWD_LOCK` (`crates/git-command/tests/phase5_crosswise.rs`, `crates/git-command/tests/phase6_crosswise.rs`).

## Mocking

**Framework:** None. There are no mock libraries in any `Cargo.toml`. External interaction is tested against the real thing (real tempdirs, real system git, real `Repository` on disk).

**Patterns:**

- In-process invocation instead of subprocess spawning: build `RepoContext` with `RepoContext::at(&dir)` or `RepoContext::from_global_args(...)`, call `git_command::dispatch_with(&ctx, &cmd, &sub, &mut out)`, and map the result to `(text, code)` exactly as the binary does (`crates/git-command/tests/phaseA01_crosswise.rs::ours`, `crates/git-command/tests/probe_command.rs`):

```rust
let ctx = RepoContext::at(&dir);
let mut out = Vec::new();
ProbeRefs.run(&ctx, &[], &mut out).expect("probe command runs");
```

- Resolver/lookup stubs are plain closures, not mocks: `let mut resolver = |_: &git_hash::Oid| -> Option<Object> { None };` (`crates/git-odb/tests/pack_crosswise.rs`).
- The `sys()` helper wraps `std::process::Command` on the system git and asserts success; the `real()` helper captures combined stdout+stderr plus exit code for comparison (`crates/git-command/tests/probe_command.rs`, `crates/git-command/tests/phaseA01_crosswise.rs`).
- `plumbing_stability.rs` spawns the built binary instead (`crates/target/debug/git` via `env!("CARGO_MANIFEST_DIR")`) because it must test global `-c` flag handling end-to-end (`crates/git-command/tests/plumbing_stability.rs::rust_git`, `::ours`).

**What to Mock:**

- Nothing with a framework. Isolate with real tempdirs (`tempdir()` per test), hermetic contexts (`RepoContext::at(&dir)`), and `CWD_LOCK` serialization for cwd/env mutation.

**What NOT to Mock:**

- Never mock `Odb`/`LooseStore`/`RefStore`/`Repository`. Seed a repo with system git and read through the real store interfaces — the probe drill (`crates/git-command/tests/probe_command.rs`) exists precisely to prove new commands compile against the real `Odb`/`RefStore` APIs with zero changes.
- Never fake the C-git oracle: crosswise tests must run the real `git` binary found at `/usr/bin/git`, `/usr/local/bin/git`, or `/opt/homebrew/bin/git`, and skip (early `return`) when none exists.

## Fixtures and Factories

**Test Data:**

- Golden fixtures are generated, not hand-written, by `cargo xtask gen-fixtures` using the system C git: a small repo with 3 commits, repacked, with `multi-pack-index` and `commit-graph` written, plus a SHA-1 `.checksums` file. Regenerate; do not hand-edit (`crates/xtask/src/main.rs::gen_fixtures`, output under `crates/tests/fixtures/` with `README.md` + `repo/`).
- Collision fixtures come from the C tree: `t/t0013/shattered-1.pdf` for SHA-1 collision-detection tests (`crates/git-command/tests/phaseA01_crosswise.rs`).
- Inline factories build objects directly: `Object::from_data(ObjectKind::Blob, b"...".to_vec())` + `PackObject { oid, kind, data }` + `write_pack(&pos, algo)` for pack round-trips (`crates/git-odb/tests/pack_crosswise.rs::real_git_reads_our_pack`).
- The strbuf conversion suite ports the C helper as a Rust helper: `fn assert_sane_strbuf(buf: &StringBuf)` asserting NUL-termination and `alloc >= len + 1` after every mutation (`crates/git-core/tests/strbuf_conversion.rs`).

**Location:**

- Generated goldens: `crates/tests/fixtures/` (`crates/tests/fixtures/README.md`, `crates/tests/fixtures/repo/`).
- C-side oracle data: `t/t0013/`, `t/unit-tests/u-strbuf.c` (ported by `crates/git-core/tests/strbuf_conversion.rs`).
- Per-test scratch repos: `std::env::temp_dir().join(format!("git-<suite>-{}-{n}", pid))`, removed after the test.

## Coverage

**Requirements:** Target is ≥90% line coverage on each phase's crates via `cargo llvm-cov` (invoked as `cargo n --fail-under-lines 90` in planning docs), but the gate is **NOT DONE / not wired into CI** (`docs/plan/FOLLOWUPS.md` item B7, `docs/plan/README.md` phase-done gates, `docs/plan/test-completion-plan.md`). Treat "no enforced minimum" as current state; do not claim coverage is gated.

**View Coverage:**

```bash
cd crates && cargo llvm-cov --workspace        # planned tool; gate not yet enforced (FOLLOWUPS B7)
```

## Test Types

**Unit Tests:**

- Scope: pure logic with no filesystem or subprocess — hashing, varint, date parsing, config parsing, strbuf invariants, compress round-trips, CLI dispatch exit codes. Location: inline `mod tests` in the source file. Approach: table-driven table asserts over known vectors (empty blob/tree oids in `crates/git-hash/src/lib.rs`) plus boundary checks (`TooShort`/`TooLong`/`InvalidHex` in `crates/git-hash/src/lib.rs::hex_abbrev_matches_by_prefix`).

**Integration Tests:**

- Scope: on-disk compatibility in both directions and CLI parity. Approach is always differential — run Rust and C git on identical inputs, assert identical stdout/stderr/exit code. Suites: `pack_crosswise` (Rust reads C packs; C `index-pack --verify` / `verify-pack` / `fsck` accept Rust packs), `graph_midx_crosswise`, `phase*_crosswise` (one file per plan phase), `followups_crosswise`, `plumbing_stability` (display configs must not alter plumbing bytes), `probe_command` (compile-against-interfaces drill). All skip gracefully without system git.
- The full C shell suite runs separately: `scripts/shim-git` routes ported commands to `crates/target/debug/git` and everything else to the system git, so `t/*.sh` exercises the port in place. `crates/scoreboard.json` is the committed regression baseline rewritten by `cargo xtask scoreboard`, which fails on any previously-passing suite turning red (`crates/xtask/src/main.rs::scoreboard`).

**E2E Tests:**

- No browser/E2E framework. The closest equivalents are `cargo xtask differential` (all crosswise suites vs system git), `cargo xtask scoreboard` (differential + baseline regression check), `cargo xtask isolation` (unit targets `--lib --bins --doc --offline` per crate in a bare dir with scrubbed env; integration `tests/` are explicitly excluded as non-hermetic by contract — `crates/xtask/src/isolation.rs`), and the structural gates (`depcheck`, `safety`, `placement`, `drills`, `oversized` in `crates/xtask/src/`).

## Common Patterns

**Async Testing:**

- Not applicable. The workspace is fully synchronous (`std::fs`, `std::process::Command`, `std::io::Read/Write`); no `tokio`/`async-std` dependency exists. For concurrent-adjacent needs (parallel test interference), serialize with `CWD_LOCK: Mutex<()>` instead of async primitives (`crates/git-command/src/lib.rs::tests`).

**Error Testing:**

```rust
// Unit: assert the typed error value (crates/git-compress/src/lib.rs)
let err = decode_bounded(&enc, 10).unwrap_err();
assert_eq!(err, CompressError::TooLarge { limit: 10 });
assert!(matches!(decode_all(b"not deflate at all!!"), Err(CompressError::Corrupt(_))));

// Command boundary: assert the exit-code class (crates/git-command/tests/probe_command.rs)
let err = ProbeRefs
    .run(&ctx, &["--bogus".to_string()], &mut Vec::new())
    .expect_err("bogus arg is a usage error");
assert_eq!(err.code, 129);
```

- Property tests assert round-trip and no-panic invariants with `prop_assert_eq!` rather than asserting specific errors (`crates/git-hash/src/lib.rs::props::incremental_equals_oneshot`, `crates/git-compress/src/lib.rs::props::decode_never_panics`, `crates/git-attributes/tests/wildmatch_proptest.rs::no_panic_on_arbitrary_input`).

---

*Testing analysis: 2026-09-25*
