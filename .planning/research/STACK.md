# Stack Research

**Domain:** Pure-Rust git reimplementation (git.rs port of C git v2.55.0)
**Researched:** 2026-09-25
**Confidence:** HIGH (versions verified via crates.io registry + Context7 official docs; see Sources)

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| Rust toolchain (stable via rustup) | edition 2021, MSRV 1.74, observed rustc 1.97.1 | Language for the entire port | Locked project decision: byte-identical C parity needs zero-cost byte control + `cargo xtask` gates (`depcheck`, `safety`, `msrv verify`). Edition 2021 is the workspace pin (`crates/Cargo.toml`); MSRV 1.74 is the CI floor — every new dep must support it. Do not bump edition/MSRV without a plan amendment. |
| Sync `std::io` / `std::fs` / `std::process`, no async runtime | std only | All I/O in library + CLI + test harness | Git is a single-invocation stateless CLI; C git is synchronous. Async (`tokio`/`async-std`/`futures`) adds a reactor, MSRV risk, and `Send`-plumbing for zero benefit — none is in `Cargo.lock` today. Keep it that way through all remaining-builtin phases. |
| Vendored pure-Rust hashers in `git-hash` (`sha1.rs` + `sha1dc.rs` + `sha256.rs`) | workspace-internal (no version) | SHA-1 (collision-detecting) + SHA-256 object IDs | This is the single most deliberate stack choice. C git links `sha1dc` for SHAttered-attack detection and the port selects that backend (`git-hash/src/lib.rs:298-307`); external `sha1`/`sha2` crates do not give C-exact collision semantics or C-exact error text. Vendoring keeps hashing at layer rank 0 with zero deps and zero MSRV exposure. |
| `flate2` 1.x, owned **exclusively** by `git-compress` facade | **1.1.10** (latest per crates.io 2026-09-25; lock currently 1.1.9 → bump) | Sole zlib/deflate provider for loose objects + packs | 2025 standard for DEFLATE in Rust: default `rust_backend` = pure-Rust `miniz_oxide 0.8.9` + `crc32fast`/`adler2`, no C toolchain, no system zlib — exactly what a self-contained `git` binary needs. `depcheck` enforces single ownership; all crates use `encode_all` / `decode_bounded` / streaming `Encoder`/`Decoder` through the facade. |
| Hand-rolled CLI parsing in `git-cli` + `git-command` dispatch | workspace-internal (no version) | Global flags (`-C`, `-c`, `--git-dir`, `--work-tree`, `--bare`) + ~45 per-builtin parsers + C-exact exit codes (129/128/1/141) | C git's CLI contract (abbreviated flags, per-command bespoke `usage:` text, `fatal:` prefixes, SIGPIPE→141 via `PipeAwareWriter`) is not expressible in a derive framework without breaking byte-identical stderr. Hand-rolling also keeps `RepoContext::from_global_args` testable (`RepoContext::at(dir)`) and avoids a heavy proc-macro dep across MSRV. |
| Hand-rolled typed error enums per crate → `CommandError` | workspace-internal (no version) | `OdbError`, `RepoError`, `PackError`, `CompressError`, … converted at the command boundary to `{message, code}` | Exit-code fidelity (129 usage / 128 die / 1 error / silent diff-code) requires typed errors with `From` funnels. `anyhow` erases the code mapping; `thiserror` 2.x is a proc-macro MSRV liability. The one legacy exception (`anyhow 1.0.104` + `thiserror 1.0.69` in `git-attributes` only) stays quarantined — do not propagate the pattern. |
| Hand-rolled config + index + object parsers (no serde) | workspace-internal (no version) | `git-config` layering, v2 index, commit/tree/tag wire formats | Git's on-disk formats are not serde-shaped (config layering with `-c`/`GIT_CONFIG_COUNT` overlays, NUL-delimited index, varint pack headers). `serde` is absent from `Cargo.lock` by design; `xtask`'s minimal JSON reader stays hand-rolled. Adding serde buys nothing and widens the dep surface every phase must audit. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `proptest` | **1.11.0** (dev-only; lock matches latest per crates.io) | Property tests: round-trip + never-panic on arbitrary input | Already in 13 crates' `[dev-dependencies]`. Use for every new parser/serializer a phase adds (`proptest!` + `prop_assert_eq!`, `decode_never_panics` pattern). Keep fork/timeout defaults; custom `ProptestConfig::with_cases` only where a corpus needs it. |
| `regex` | **1.11.1** (exact `=1.11.1` pin) | Userdiff funcname patterns (`git-diff/src/userdiff.rs`, `regex::bytes`) | Keep scoped to `git-diff` only. The exact pin avoids regex-automata churn breaking MSRV 1.74. Do not reach for regex where `memchr`-style byte scans or hand-rolled matchers suffice. |
| `cargo-llvm-cov` | latest stable (tool, not a crate dep) | ≥90% line-coverage gate per phase (`docs/plan/README.md`) | Invoke manually (`cargo llvm-cov --workspace`); gate wiring is still a FOLLOWUPS item, not CI-enforced. Wire it before claiming coverage gates green. |
| `cargo xtask` (workspace-internal binary, zero external deps) | n/a | `test` / `differential` / `gen-fixtures` / `scoreboard` / `gates` (`depcheck`, `safety`, `placement`) | The harness for every remaining phase. `differential` + `scoreboard` + `t/`-via-`shim-git` is the parity proof; `depcheck`/`safety`/`placement` are the structural proof. No replacement needed. |
| System C git (`/usr/bin/git`) + `scripts/shim-git` | upstream v2.55.0 oracle | Differential/crosswise comparator + `t/` suite runner | Test-only oracle, never a build dep. Keep `RUST_GIT`/`SYSTEM_GIT` overrides; keep per-test `tempdir()` + `CWD_LOCK` discipline. |

## Installation

```bash
# From crates/ (the real workspace — never repo root)
cd crates

# Keep the compression backend current (only direct infra dep that moves)
cargo update --registry crates-io -p flate2   # 1.1.9 -> 1.1.10

# Dev harness (already pinned; no action unless proptest releases a major)
cargo test --workspace
cargo xtask differential
cargo xtask scoreboard
cargo xtask gates   # depcheck + safety + placement

# No new installs required for remaining-builtin phases:
#   no clap, no serde, no tokio, no sha1/sha2, no tempfile/assert_cmd/insta
```

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| Vendored SHA-1/sha1dc + SHA-256 | `sha1 0.11.0` / `sha2 0.11.0` (RustCrypto) | Only if the project abandons C-exact collision-attack text — i.e. never under the current parity contract. RustCrypto is the right default for *new* crypto code, not for a bug-compatible git port. |
| Vendored hashers | `sha1_smol 1.0.1`, `ring`, `openssl` | Never: `sha1_smol` lacks collision detection; `ring`/`openssl` drag in C/asm toolchains and kill the pure-Rust static-binary story. |
| `flate2 1.1.10` default (`miniz_oxide`) backend | `flate2` with `zlib-ng` / `zlib` / `libz-sys` features | Never for the shipped binary (C toolchain + system libz dependency). Revisit only if pack-write benchmarks prove a bottleneck — and then only behind the `git-compress` facade, never as a second dep path. |
| `flate2` via facade | `zlib-rs` direct, `async-compression`, `miniz_oxide` direct | `zlib-rs` (fastest pure-Rust backend, `unsafe`-Rust) is acceptable *as a flate2 feature* inside `git-compress` if benchmarks justify it — not as a new direct dep. `async-compression` is out (no runtime). `miniz_oxide` direct bypasses the facade and `depcheck` will fail it. |
| Hand-rolled CLI | `clap 4.6.7` (current standard for new Rust CLIs) | For greenfield Rust CLIs — not here. `clap` 4.x derive/builder would need per-command `debug_assert` + custom error rendering to fake C's `usage:`/`fatal:` text, and rewriting ~45 commands buys negative parity value. If a *new companion tool* (not `git` itself) is ever added, clap 4.x is the right choice for that tool only. |
| Hand-rolled typed errors | `anyhow 1.0.104`, `thiserror 2.0.21`, `miette` | `anyhow` for application-level prototyping only — it cannot carry exit codes. `thiserror` 2.x for libraries that don't need C-exact codes — the port does. Quarantine stays: `git-attributes` keeps its existing pair; new crates hand-roll. |
| Hand-rolled config/JSON | `serde` + `serde_json` / `toml` | Never for git formats. Serde fits JSON/TOML services, not NUL-delimited index files and layered git-config semantics. |
| `#[test]` + `proptest 1.11.0` + crosswise suites | `assert_cmd 2.2.2` / `insta`+`insta-cmd` / `tempfile 3.27.0` / `cargo-nextest` | These are fine general-purpose choices (`tempfile` is already transitively in the lock via proptest) but the port's hand-rolled `tempdir()` + in-process `dispatch_with` + real-system-git comparison is *stronger* than `assert_cmd` (tests the trait boundary, not just the binary) and snapshot crates would fight byte-parity rather than prove it. Adopt only if a phase shows concrete harness pain. |
| `gix 0.88.0` (gitoxide) as reading material | adding `gix-*` crates as dependencies | Never as a dependency: it would make the port a wrapper instead of a standalone rewrite, violate the no-FFI-equivalent independence decision, and its error/output types are not C-byte-identical. As a *design reference* for tricky algorithms (delta, negotiate, pathspec edge cases) it is the best 2025 peer implementation to read. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `clap` / `lexopt` / `pico-args` for `git` builtins | Framework-rendered help/usage/error text cannot match C's per-command `usage:` strings and exit-129 contract; global `-C`/`-c` threading through `RepoContext` would be re-implemented anyway | Hand-rolled parsers per command module + `CommandError::usage` |
| `tokio` / `async-std` / `futures` / `async-compression` | No async runtime exists; sync `std::io` matches C's single-invocation model; async infects every layer rank and breaks the MSRV floor | Sync `std::fs`/`std::io`/`std::process`; `CWD_LOCK` for test serialization |
| `serde` / `serde_json` / `toml` / `bincode` | Git formats are not serde formats; adds proc-macro + MSRV weight to every phase for zero parity gain | Hand-rolled parsers already in `git-config`, `git-object`, `git-index`, `xtask` JSON reader |
| `sha1` / `sha2` / `ring` / `openssl` / `sha1_smol` crates | No collision-attack parity (`sha1dc` semantics + `OdbError::Collision` text), extra audit/MSRV surface, toolchain deps for the C-linked ones | Vendored `git-hash` (`sha1.rs`/`sha1dc.rs`/`sha256.rs`) |
| `zlib` / `libz-sys` / `zlib-ng` C backends; second compression dep beside `flate2` | Breaks pure-Rust static binary, needs C toolchain, `depcheck` forbids a second owner | `flate2 1.1.10` default backend via `git-compress` only |
| `anyhow` in new crates; `thiserror 2.x` upgrade without MSRV check | `anyhow` erases exit-code types at exactly the boundary that must preserve them; `thiserror` 2.x bumps proc-macro MSRV vs locked 1.0.69 | Hand-rolled `*Error` enums + `From` → `CommandError::fatal` (see `git-odb/src/lib.rs` pattern) |
| `reqwest` / `hyper` / `curl` / `ssh2` / `openssl` (any network/TLS) | Network/transport is explicitly out of scope until fetch/push phases; `git-transport`/`git-protocol` are offline state machines by design | Byte-buffer state machines already in `git-transport`/`git-protocol`; revisit only via plan amendment when Phase 10+ opens |
| `gix-*` / `git2` (libgit2 bindings) as dependencies | `git2` links C libgit2 (violates standalone/pure-Rust); `gix` output types aren't C-byte-identical and would hollow out the port | Read `gix 0.88.0` source as a peer reference; depend on nothing |
| `tempfile` / `assert_cmd` / `insta` direct deps | Hand-rolled `tempdir()` + `AtomicU32` counters + in-process dispatch already cover hermetic + parity needs with fewer MSRV edges | Existing `crates/*/tests/` helpers (`tempdir()`, `real()`/`ours()`, `RepoContext::at`) |
| `log` / `tracing` / `env_logger` | No logging framework exists; diagnostics discipline is `out: &mut dyn Write` for machine output + `eprintln!` for human progress, keeping plumbing byte-stable | Existing convention (`plumbing_stability.rs` guards it) |

## Stack Patterns by Variant

**If the phase adds a new object/index/pack reader or writer:**
- Add the parser to the owning layer crate with `proptest` round-trip + no-panic props, typed errors, zero new deps.
- Because every format must crosswise-verify both directions (`C reads ours` via `fsck`/`verify-pack`, `ours reads C`).

**If pack-write performance becomes a bottleneck:**
- Flip a feature *inside* `git-compress` (`zlib-rs` backend benchmark) — never add a second compression crate or leak `flate2` out of the facade.
- Because `depcheck` single-ownership is load-bearing for auditability.

**If network/transport phases (10+) open:**
- Propose the transport stack via plan amendment (sync I/O first; `reqwest`/`tokio` only with MSRV + static-binary + parity justification).
- Because the current lockfile provably contains no TLS/HTTP/SSH, and sneaking it in per-phase would invalidate every done-gate.

**If MSRV pressure appears (new dep needs >1.74):**
- Reject or vendor the code — `cargo msrv verify` in `ci/run-rust-checks.sh` is the floor.
- Because the project promises 1.74 and CI-adjacent contributors build there.

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `flate2 1.1.10` | `miniz_oxide 0.8.9`, `crc32fast 1.5.0`, `adler2 2.0.1`, rustc ≥1.74 | Default features only. Do not enable `zlib`/`zlib-ng` (C toolchain) or `zlib-rs` without a benchmark-backed amendment. |
| `proptest 1.11.0` | `rand 0.9.5`, `tempfile 3.27.0` (transitive), `rusty-fork`, `bit-set`/`bit-vec` | Dev-only; transitive `tempfile` must not become a direct dep. `proptest = "1"` caret in manifests resolves to 1.11.0 — correct as-is. |
| `regex =1.11.1` (exact) | `memchr 2.8.3`, `aho-corasick 1.1.5`, `regex-automata 0.4.18` | Exact pin is intentional — keep the `=`. Loosening to caret risks automata/MSRV churn. |
| `anyhow 1.0.104` + `thiserror 1.0.69` | quarantined to `git-attributes` | Do not upgrade `thiserror` to 2.0.21 workspace-wide without MSRV verification; do not spread either crate to new members. |
| `gix 0.88.0` | reference only, not in `Cargo.lock` | Reading the peer implementation is encouraged; depending on it is a plan-amendment-level decision (effectively never). |
| `clap 4.6.7`, `serde 1.x`, `tokio 1.x` | not in `Cargo.lock`, must stay absent | Any PR adding them fails review without a plan amendment per PROJECT.md constraints. |

## Sources

- crates.io registry via `cargo search --registry crates-io` (2026-09-25) — `flate2 1.1.10`, `proptest 1.11.0`, `clap 4.6.7`, `sha1 0.11.0`, `sha2 0.11.0`, `thiserror 2.0.21`, `tempfile 3.27.0`, `assert_cmd 2.2.2`, `gix 0.88.0` — HIGH confidence
- Context7 `/rust-lang/flate2-rs` — default `miniz_oxide` backend, `zlib-ng`/`zlib` feature table, `ZlibEncoder`/`ZlibDecoder` API — HIGH confidence
- Context7 `/proptest-rs/proptest` + proptest book — `proptest!` macro, `prop_compose!`, `ProptestConfig`, `TestRunner` API current — HIGH confidence
- Context7 `/clap-rs/clap` — 4.x derive/builder current API — HIGH confidence (used to justify rejection, not adoption)
- Context7 `/websites/rs_sha2` — RustCrypto SHA-2 current scope — HIGH confidence (used to justify rejection, not adoption)
- `.planning/codebase/STACK.md` (2026-09-25) — workspace pins, facade rule, deliberately-absent list — HIGH confidence (first-party map)
- `crates/Cargo.lock` (76 packages) + `crates/Cargo.toml` (edition 2021, rust-version 1.74) — HIGH confidence (read directly)
- `.planning/PROJECT.md` constraints (no new crypto/CLI/serde/network deps without amendment) — HIGH confidence

---
*Stack research for: pure-Rust git reimplementation (remaining builtins + parity hardening)*
*Researched: 2026-09-25*
