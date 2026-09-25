---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# Coding Conventions

**Analysis Date:** 2026-09-25

## Naming Patterns

**Files:**

- Use `snake_case` for Rust source files. Command modules mirror the hyphenated git subcommand name with underscores: `cat_file.rs`, `rev_parse.rs`, `ls_tree.rs`, `commit_graph.rs`, `merge_base.rs` in `crates/git-command/src/`.
- One subcommand per file. Shared logic gets a suffixed helper module: `crates/git-command/src/checkout_core.rs`, `crates/git-command/src/ignore_util.rs`.
- Crate directories use `kebab-case`: `crates/git-command/`, `crates/git-commitgraph/`, `crates/git-odb/`. Crate names match directory names.
- Test suites under `crates/*/tests/` use `snake_case` with a kind suffix: `pack_crosswise.rs`, `phaseA01_crosswise.rs`, `wildmatch_proptest.rs`, `plumbing_stability.rs`, `probe_command.rs`, `strbuf_conversion.rs`.
- The C side keeps upstream git names verbatim (`builtin/hash-object.c`, `t/t0013/*.sh`); never rename those to match Rust conventions.

**Functions:**

- Use `snake_case` for functions and methods, verb-first for actions: `from_hex`, `from_hex_abbrev`, `compute_id`, `dispatch_with`, `resolve_arg`, `repository`, `read_tree`, `write_pack` (`crates/git-hash/src/lib.rs`, `crates/git-command/src/lib.rs`, `crates/git-odb/src/pack/mod.rs`).
- Constructors are `new` / `from_*` / `at`: `StringBuf::new()`, `Oid::new()`, `CryptoHasher::new()`, `RepoContext::at()` (`crates/git-core/src/strbuf.rs`, `crates/git-hash/src/lib.rs`, `crates/git-command/src/lib.rs`).
- Predicate helpers end in a question word, not `is_` prefix exceptions aside: `is_empty`, `is_safe` (`crates/git-core/src/strbuf.rs`, `crates/git-hash/src/lib.rs`).
- Test functions are descriptive `snake_case` sentences: `hash_object_shattered_pdf_matches`, `pathname_is_narrower`, `no_panic_on_arbitrary_input`, `display_config_cannot_alter_plumbing_output` (`crates/git-command/tests/phaseA01_crosswise.rs`, `crates/git-attributes/tests/wildmatch_proptest.rs`, `crates/git-command/tests/plumbing_stability.rs`).
- Use `const fn` for pure compile-time-computable accessors: `raw_len`, `hex_len`, `name`, `format_id`, `null_oid` (`crates/git-hash/src/lib.rs`).

**Variables:**

- Use `snake_case` locals, short but meaningful: `odb`, `repo`, `ctx`, `out`, `oid`, `algo`, `rest`, `sub` (`crates/git-command/src/cat_file.rs`, `crates/git-command/src/hash_object.rs`).
- Name the caller-supplied output writer `out: &mut dyn Write` in every command — never `stdout` — to keep commands unit-testable (`crates/git-command/src/lib.rs`, `crates/git-command/src/cat_file.rs`).
- Name the repository context `ctx: &RepoContext`, never a global or a freshly discovered `Repository` inside `run` (`crates/git-command/src/lib.rs`).
- Error bindings are `e`: `.map_err(|e| CommandError::fatal(e.to_string()))`.

**Types:**

- Use `CamelCase` for types and traits: `CommandError`, `RepoContext`, `StringBuf`, `HashAlgorithm`, `OdbError`, `RepoError`, `CryptoHasher`, `PipeAwareWriter` (`crates/git-command/src/lib.rs`, `crates/git-core/src/strbuf.rs`, `crates/git-hash/src/lib.rs`, `crates/git-cli/src/lib.rs`).
- Error enums end in `Error`: `CommandError`, `OdbError`, `RepoError`, `HashError`, `HexParseError`, `CompressError`, `ObjectError` (`crates/git-command/src/lib.rs`, `crates/git-odb/src/lib.rs`, `crates/git-core/src/lib.rs`, `crates/git-hash/src/lib.rs`, `crates/git-compress/src/lib.rs`).
- Unit command structs are `CamelCase` without suffix: `CatFile`, `HashObject`, `RevList`, `LsTree` (`crates/git-command/src/cat_file.rs`, `crates/git-command/src/hash_object.rs`).
- Constants are `SCREAMING_SNAKE_CASE`: `VERSION`, `EXIT_USAGE`, `EXIT_NOT_FOUND`, `EXIT_SIGPIPE`, `GIT_MAX_RAWSZ` (`crates/git-cli/src/lib.rs`, `crates/git-hash/src/lib.rs`).
- Property-test modules are named `props`; unit-test modules are named `tests` (`crates/git-hash/src/lib.rs`, `crates/git-compress/src/lib.rs`).

## Code Style

**Formatting:**

- No `rustfmt.toml` / `.rustfmt.toml` / `clippy.toml` exists at repo root or in `crates/` — rustfmt defaults apply. Run `cargo fmt` from `crates/` before committing; do not add a config file to "fix" style.
- Default 4-space indentation, 100-column soft target, trailing commas in multi-line calls. Follow what rustfmt produces; the codebase is uniformly rustfmt-clean.
- Workspace inherits `edition = "2021"` and `rust-version = "1.74"` from `crates/Cargo.toml` (`[workspace.package]`); per-crate manifests use `edition.workspace = true`. The stale root `Cargo.toml` (`gitcore` staticlib, edition 2018) is not the workspace — ignore it.
- C files follow upstream git style, not Rust style: tabs, width 8, mirrored in `.clang-format` and `.editorconfig`. See `Documentation/MyFirstContribution.txt` before touching `*.c`/`*.h`.
- Cargo alias lives in `crates/.cargo/config.toml`: `cargo xtask <cmd>` expands to `cargo run --package xtask -- <cmd>`. Always run cargo from `crates/`.

**Linting:**

- CI runs `cargo clippy --workspace --manifest-path crates/Cargo.toml -- -D warnings || true` (`.github/workflows/rust-port.yml`). Treat clippy warnings as errors locally even though CI currently tolerates them (`|| true`).
- The structural gates in `crates/xtask/src/` enforce what clippy does not: `depcheck` (dependency cycles/layering), `safety` (zero unjustified `unsafe`), `placement` (21-boundary ownership). Run `cargo xtask gates` from `crates/` before large changes.
- `unsafe` is effectively banned: the only allowed hits are `unsafe` substrings inside regex string literals in `crates/git-diff/src/userdiff.rs` (C# / Rust keyword lists mirroring C git userdiff patterns), allowlisted in `crates/xtask/src/safety.rs`. Never add real `unsafe` blocks.

## Import Organization

**Order:**

1. `std::` imports first, one item per line grouped by module: `use std::error::Error;`, `use std::fmt;`, `use std::io::Write;`, `use std::path::PathBuf;` (`crates/git-command/src/lib.rs`, `crates/git-odb/src/lib.rs`).
2. External third-party crates next (`flate2`, `proptest`): `use proptest::prelude::*;` inside `mod props` / test files only (`crates/git-compress/src/lib.rs`, `crates/git-attributes/tests/wildmatch_proptest.rs`).
3. Workspace `git-*` path dependencies next, alphabetical: `use git_core::{RepoEnv, RepoError, Repository};`, `use git_hash::Oid;`, `use git_object::{parse_tree, Object, ObjectKind};`, `use git_odb::Odb;` (`crates/git-command/src/cat_file.rs`).
4. `crate::` imports last within the file's own crate: `use crate::{Command, CommandError, RepoContext};` (`crates/git-command/src/cat_file.rs`, `crates/git-command/src/hash_object.rs`).

**Path Aliases:**

- No `[lints]` path aliases, no `pub use` renaming tricks, no `extern crate` aliases. Refer to workspace crates by their real names (`git_hash::Oid`, `git_odb::Odb`, `git_core::Repository`).
- One exception: tests alias the trait to avoid clashing with `std::process::Command`: `use git_command::{Command as GitCommand, CommandError, RepoContext};` (`crates/git-command/tests/probe_command.rs`). Use exactly this alias when both are in scope.

## Error Handling

**Patterns:**

- Define a per-crate error enum deriving `Debug, Clone, PartialEq, Eq`, implement `fmt::Display` with git-style message text, implement `std::error::Error`, and add `From` conversions toward `CommandError::fatal`. Example (`crates/git-odb/src/lib.rs`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum OdbError {
      Io(String),
      Corrupt(String),
      NotFound,
      Collision(String),
  }

  impl fmt::Display for OdbError {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          match self {
              OdbError::Io(e) => write!(f, "object store I/O error: {e}"),
              OdbError::Corrupt(e) => write!(f, "corrupt object: {e}"),
              OdbError::NotFound => write!(f, "object not found"),
              OdbError::Collision(hex) => {
                  write!(f, "SHA-1 appears to be part of a collision attack: {hex}")
              }
          }
      }
  }

  impl Error for OdbError {}
  ```
- At the command boundary use `CommandError { message, code }` with these constructors (`crates/git-command/src/lib.rs`):
  ```rust
  CommandError::usage(...)   // exit 129 — bad flags, missing operands
  CommandError::fatal(...)   // exit 128 — repo/IO/odb failures, prefix "fatal: "
  CommandError::error(...)   // exit 1   — bad object names, type mismatches
  CommandError::silent(code) // no message — e.g. `git diff` exits 1 when files differ
  ```
- Convert lower layers with `From` (`RepoError`, `OdbError`, `PackError`, `MidxError`, `GraphError` → `CommandError::fatal`) or inline `.map_err(|e| CommandError::fatal(e.to_string()))` for `std::io` errors (`crates/git-command/src/lib.rs`, `crates/git-command/src/cat_file.rs`, `crates/git-command/src/hash_object.rs`).
- Propagate with `?`; never `unwrap()` on a fallible path in command code. `unwrap()`/`expect()` appear only in tests and in provably-total setup (e.g. `AtomicU32` tempdir counters, `canonicalize().unwrap()` in test helpers).
- Match C git's stderr text exactly, including the `fatal: ` prefix where C prints it, because crosswise tests compare combined stdout+stderr byte-for-byte (`crates/git-command/src/lib.rs::config_count_overrides`, `crates/git-command/src/cat_file.rs`).
- Exit-code constants live in `crates/git-cli/src/lib.rs`: `EXIT_USAGE = 129`, `EXIT_NOT_FOUND = 1`, `EXIT_SIGPIPE = 141`. `PipeAwareWriter` there converts EPIPE into exit 141 (C dies of SIGPIPE); copy that wrapper instead of inventing new pipe handling.
- Do not introduce `anyhow`/`thiserror`/`miette` for new error types. (`git-attributes` lists `anyhow`/`thiserror` in `crates/git-attributes/Cargo.toml`, but every other crate hand-rolls its error enum — follow the hand-rolled pattern.)
- In tests assert on the error value, not just failure: `assert_eq!(err.code, 129)` and `assert_eq!(err, CompressError::TooLarge { limit: 10 })` (`crates/git-command/tests/probe_command.rs`, `crates/git-compress/src/lib.rs`).

## Logging

**Framework:** No logging framework. There is no `log`, `tracing`, or `env_logger` dependency in any workspace `Cargo.toml`. Use `eprintln!`/`println!` directly.

**Patterns:**

- Primary command output goes to the injected `out: &mut dyn Write` parameter, never to `println!` inside `run` (`crates/git-command/src/cat_file.rs::CatFile::run`, `crates/git-command/src/hash_object.rs`).
- Human progress/status text goes to stderr via `eprintln!`: `Switched to branch '...'`, `Updated N paths from ...`, `HEAD is now at ...`, `warning: ...`, `error: ...` (`crates/git-command/src/checkout.rs`, `crates/git-cli/src/lib.rs`).
- Plumbing output must stay byte-identical under display configs; never gate machine-readable output on `color.ui` / `format.pretty` / `core.abbrev` (regression test: `crates/git-command/tests/plumbing_stability.rs`).
- IO failures on `out` map to `CommandError::fatal(e.to_string())`: `writeln!(out, ...).map_err(|e| CommandError::fatal(e.to_string()))?` (`crates/git-command/src/cat_file.rs`).
- Keep the `fatal: ` prefix inside the message string when C git prints it (e.g. `CommandError::fatal(format!("fatal: {e}"))` in `From<RepoError>`, `crates/git-command/src/lib.rs`).

## Comments

**When to Comment:**

- Every file starts with a `//!` crate/module doc stating what it ports or implements, usually naming the C counterpart: `//! \`git hash-object\`: compute (and optionally store) ... A port of \`builtin/hash-object.c\`.` (`crates/git-command/src/hash_object.rs`), `//! A string buffer mirroring git's C \`strbuf\` API.` (`crates/git-core/src/strbuf.rs`).
- Document C-equivalence on non-obvious items: which C function a method mirrors (`Equivalent to \`strbuf_grow(sb, amount)\``), which invariant is preserved (`alloc >= len + 1`), which exit-code rule applies (`A usage error (git exits 129)`) (`crates/git-core/src/strbuf.rs`, `crates/git-command/src/lib.rs`).
- Explain why, not what, inline: `// The store is only needed to actually write objects; without -w the hash is computed with SHA-1 (git's default outside a repository).` (`crates/git-command/src/hash_object.rs`), `// Accepted for compatibility; paging is not implemented.` (`crates/git-command/src/lib.rs`).
- Crosswise/integration test files open with a `//!` block stating direction of compatibility and skip behavior (`crates/git-odb/tests/pack_crosswise.rs`, `crates/git-command/tests/plumbing_stability.rs`, `crates/git-command/tests/probe_command.rs`).

**JSDoc/TSDoc:**

- Rust `///` doc comments on every public item (structs, enums, methods, functions, trait methods). Private helpers get `///` too when the contract matters (`split_record`, `emit_batch`, `expand_format` in `crates/git-command/src/cat_file.rs`).
- Document panics, error cases, and C-text compatibility in the doc comment: `/// Parse an abbreviated lowercase hex id of at least min_len characters.` plus usage guidance (`crates/git-hash/src/lib.rs::Oid::from_hex_abbrev`).
- No doctests (no ``` examples in doc comments): runnable examples are plain `#[test]` functions, not doc-code. The only ``` fences in `*.rs` are in `crates/git-core/examples/strbuf_bench.rs` and a comment in `crates/git-index/src/cache_tree.rs`.

## Function Design

**Size:** Keep functions focused and short; split batch/format/emit loops into small free helpers. `CatFile::run` delegates to `split_record`, `emit_batch`, `expand_format`, `expand_object_atom`, `pretty_print` rather than inlining everything (`crates/git-command/src/cat_file.rs`).

**Parameters:** Take `(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write)` for every command via the `Command` trait (`crates/git-command/src/lib.rs`):

```rust
pub trait Command {
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError>;
}
```

Pass repository context explicitly; commands never call `Repository::discover()` or read process env directly except through `ctx.repository()` and the `config_count_overrides()` helper. Take slices (`&[String]`, `&[u8]`, `&Path`) over owned collections for inputs; return `Result<_, CommandError>` (commands) or `Result<_, <Crate>Error>` (libraries).

**Return Values:** Return `Result<(), CommandError>` from commands and `Result<T, SpecificError>` from library code; use `Option<Result<(), CommandError>>` only for `dispatch`/`dispatch_with` where `None` means "unknown command" (`crates/git-command/src/lib.rs`). Prefer domain error enums over `String` errors and over `bool` success flags. `Object::from_data` + `try_compute_id`/`compute_id` is the canonical construct-then-hash return pair (`crates/git-command/src/hash_object.rs`, `crates/git-hash/src/lib.rs`).

## Module Design

**Exports:** Declare submodules with `pub mod <name>;` in `lib.rs`, then re-export the narrow public surface with `pub use`: `pub use myers::{diff as diff_lines, split_lines, Op};`, `pub use pack::{Odb, PackError};` (`crates/git-diff/src/lib.rs`, `crates/git-odb/src/lib.rs`). Keep `lib.rs` itself thin — types and logic live in the submodule files (`sha1.rs`, `sha256.rs`, `sha1dc.rs` under `crates/git-hash/src/`).

- To add a ported command: create `crates/git-command/src/<snake_name>.rs` with `pub struct <Camel>; impl Command for <Camel>`, add `pub mod <snake_name>;` to `crates/git-command/src/lib.rs`, and add the match arm in `dispatch_with` plus the `case` entry in `scripts/shim-git` (see also the dispatcher wiring in `crates/git-cli/src/lib.rs`).

**Barrel Files:** `lib.rs` is the barrel for every crate: `pub mod` declarations plus `pub use` re-exports, plus shared types (`CommandError`, `RepoContext`, `Command`, `dispatch`) for `git-command` (`crates/git-command/src/lib.rs`); `pub mod myers/tree/unified/userdiff` plus `pub use` for `git-diff` (`crates/git-diff/src/lib.rs`); `pub mod sha1/sha1dc/sha256` plus `Oid`/`HashAlgorithm`/`CryptoHasher` for `git-hash` (`crates/git-hash/src/lib.rs`). Tests import through the barrel (`use git_command::{Command as GitCommand, CommandError, RepoContext};`, `use git_core::StringBuf;`), never via deep `crate::module::inner` paths from outside the crate.

---

*Convention analysis: 2026-09-25*
