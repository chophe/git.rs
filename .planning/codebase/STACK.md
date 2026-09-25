---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# Technology Stack

**Analysis Date:** 2026-09-25

## Languages

**Primary:**

- Rust (edition 2021, MSRV 1.74) - everything under `crates/` (24 workspace members)
- Workspace pins in `crates/Cargo.toml`: `edition = "2021"`, `rust-version = "1.74"`, `resolver = "2"`, `license = "GPL-2.0-only"`
- Installed toolchain observed: `rustc 1.97.1`, `cargo 1.97.1` (MSRV floor enforced by `cargo msrv verify` in `ci/run-rust-checks.sh`)
- Hashers are vendored pure-Rust, not external crypto crates: `crates/git-hash/src/sha1.rs`, `crates/git-hash/src/sha1dc.rs`, `crates/git-hash/src/sha256.rs` (collision-detecting SHA-1 backend selected in `crates/git-hash/src/lib.rs:298-307`)

**Secondary:**

- C (GNU dialect, upstream git `v2.55.0-824` per `git describe`) - reference implementation / behavior oracle only; root `*.c`, `builtin/`, `*.h`. Never linked into the Rust build (no FFI by design, see `docs/plan/README.md:12-15`)
- Shell (`sh`) - test oracle harness `t/*.sh` plus the port dispatcher `scripts/shim-git`
- Perl / Python - upstream C-git build-time and `t/` helpers (reference side only; the Rust workspace has zero Perl/Python)

## Runtime

**Environment:**

- Stable Rust via `rustup`; CI installs with `dtolnay/rust-toolchain@stable` (see `.github/workflows/rust-port.yml:13`)
- No async runtime: zero `tokio`/`async-std`/`futures` in `crates/Cargo.lock`. All I/O is synchronous `std::io` / `std::fs` / `std::process`
- No garbage collector; `unsafe` is gated to near-zero by `cargo xtask safety` (`crates/xtask/src/safety.rs`)

**Package Manager:**

- `cargo` (invoked **from `crates/`**, never from repo root)
- Lockfile: present at `crates/Cargo.lock` (76 packages, `version = 3` format). Commit it; `cargo xtask scoreboard` and CI assume reproducible builds
- Local alias only: `crates/.cargo/config.toml` defines `xtask = "run --package xtask --"` so `cargo xtask <cmd>` works from `crates/`. There is no `.cargo/config` at repo root and no `rust-toolchain*.toml` pin file

## Frameworks

**Core:**

- None. This is a CLI binary plus library crates with no web/app framework. The binary entry is trivial: `crates/git-cli/src/main.rs` calls `git_cli::run(std::env::args())`; all dispatch lives in `crates/git-command/src/lib.rs` (`dispatch` / `dispatch_with` returning `Option<Result<(), CommandError>>`)

**Testing:**

- Built-in `#[test]` harness (unit tests co-located in `src/lib.rs` per crate) - primary unit mechanism
- `proptest 1.11.0` (dev-dependency in 13 crates: `git-hash`, `git-object`, `git-odb`, `git-config`, `git-date`, `git-index`, `git-varint`, `git-revision`, `git-pretty`, `git-compress`, `git-credentials`, `git-pathspec`, `git-attributes`) - parser/serializer round-trip + never-panic properties. Example: `crates/git-compress/src/lib.rs:282-303`
- Custom crosswise suites under `crates/git-odb/tests/` (`pack_crosswise.rs`, `graph_midx_crosswise.rs`) and `crates/git-command/tests/phase*_crosswise.rs` - byte-identical stdout/stderr/exit-code vs system C git
- `cargo llvm-cov` - coverage gate (≥90% per phase per `docs/plan/README.md:75`); invoked manually, no config file in repo
- Upstream `t/` shell suite via `scripts/shim-git` + `cargo xtask scoreboard` baseline in `crates/scoreboard.json`

**Build/Dev:**

- `cargo-xtask` binary (`crates/xtask/src/main.rs`, crate `xtask` with `publish = false`) - subcommands `test | differential | gen-fixtures | scoreboard | gates | depcheck | safety | placement | drills | oversized | isolation`. Zero external dependencies (hand-rolled `serde_lite` JSON reader and `GitSha1` in `crates/xtask/src/main.rs:361-500`)
- `cargo fmt --all --check` + `cargo clippy --all-targets --all-features -- -Dwarnings` + `cargo msrv verify`, all wired in `ci/run-rust-checks.sh` (surfaced in CI as `rust-analysis` job in `.github/workflows/main.yml:473-487`)
- `cargo run -p xtask --manifest-path crates/Cargo.toml -- <cmd>` is the CI spelling (see `.github/workflows/rust-port.yml:26,36`)

## Key Dependencies

**Critical (direct, declared in crate manifests):**

- `flate2 1.1.9` - sole deflate provider, owned exclusively by `git-compress` (`crates/git-compress/Cargo.toml:14`). Architectural rule (enforced by `cargo xtask depcheck`): no other crate may depend on `flate2` directly; all compression goes through the `git-compress` facade (`encode_all`, `decode_bounded`, streaming `Encoder`/`Decoder`, cap-enforcing `Inflater` in `crates/git-compress/src/lib.rs`)
- `regex =1.11.1` (exact pin, note the `=`) - used only by `git-diff` for userdiff funcname patterns (`crates/git-diff/Cargo.toml:12`, `crates/git-diff/src/userdiff.rs:3`: `use regex::bytes::{Regex, RegexBuilder}`)
- `anyhow 1.0.104` + `thiserror 1.0.69` - used **only** by `git-attributes` (`crates/git-attributes/Cargo.toml:9-10`). Every other crate uses hand-rolled `std::error::Error` enums (e.g. `TransportError` in `crates/git-transport/src/lib.rs:18-37`, `CompressError` in `crates/git-compress/src/lib.rs:17-24`). Do not add `anyhow` to new crates; follow the typed-error-enum pattern
- `proptest 1.0` (caret, resolves to `1.11.0` per lock) - dev-only, see Testing above

**Infrastructure (transitive via `crates/Cargo.lock`, do not add directly):**

- `miniz_oxide 0.8.9` + `crc32fast 1.5.0` + `adler2 2.0.1` + `simd-adler32 0.3.10` - via `flate2` (pure-Rust zlib backend; this is why the port needs no system zlib)
- `memchr 2.8.3` + `aho-corasick 1.1.5` + `regex-automata 0.4.18` + `regex-syntax 0.8.11` - via `regex`
- `rand 0.9.5` + `rand_chacha` + `rand_core` + `rand_xorshift` + `bit-set`/`bit-vec` + `rusty-fork 0.3.1` + `tempfile 3.27.0` + `wait-timeout` + `quick-error` + `unarray` - via `proptest`. Note: `tempfile` appears in the lock **only** transitively; no workspace crate declares it - test helpers that need temp dirs roll their own or go through `xtask` isolation helpers
- `syn 2.0.119` + `quote` + `proc-macro2` + `unicode-ident` - via `thiserror-impl` / proptest derives
- Platform shims (`libc`, `rustix`, `windows-sys`, `wasi`/`wit-bindgen`, `r-efi`, `zerocopy`) - transitive, no direct use in workspace code

**Deliberately absent (do not introduce without a plan amendment):**

- No TLS/HTTP/SSH stack (`curl`, `openssl`, `reqwest`, `hyper`, `tokio`, `ssh2` all absent from `crates/Cargo.lock`). Network I/O is out of scope until fetch/push phases; `git-transport`/`git-protocol` are offline state machines over byte buffers (see INTEGRATIONS.md)
- No serialization framework (`serde` absent; `xtask` hand-rolls minimal JSON in `crates/xtask/src/main.rs:462-500`)
- No CLI-arg framework (`clap` absent; global-option parsing is hand-rolled in `crates/git-cli/src/lib.rs:77-91` and `crates/git-command/src/lib.rs`)
- No crypto crates (`sha2`, `ring`, `openssl` absent; SHA lives in `crates/git-hash/src/`)

## Configuration

**Environment:**

- No `.env` files exist and none are used. Configuration is: (a) process env vars read in `crates/git-core/src/lib.rs:64-79` (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, ...), (b) repo config files parsed by `git-config` (`crates/git-config/src/lib.rs`), (c) CLI flags (`--git-dir`, `--work-tree`, `--bare`, ... in `crates/git-command/src/lib.rs`)
- Test/oracle overrides only: `RUST_GIT` / `SYSTEM_GIT` paths consumed by `scripts/shim-git:6-10` (defaults: `crates/target/debug/git` and `/usr/bin/git`)
- Forbidden-file hygiene: never read or quote `.env`, `*.pem`/`*.key`, `credentials*`, `*secret*`, or `serviceAccountKey.json`-style files; note existence only

**Build:**

- Real workspace: `crates/Cargo.toml` (`resolver = "2"`, 24 members). Run `cargo` with `--manifest-path crates/Cargo.toml` or `cd crates`
- Stale leftover: root `Cargo.toml` (`gitcore` staticlib, `edition = "2018"`) - ignore it; it is not a workspace and is never built by CI
- `[profile.release] debug = true` in `crates/Cargo.toml:37-38` (symbolized release builds)
- `crates/.cargo/config.toml` - cargo alias only (see Runtime). No target-dir, linker, or registry overrides
- `.editorconfig` - `utf-8` + final newline everywhere; **tabs width 8** for C/shell/make (`*.{c,h,sh,bash,perl,pl,pm,txt,adoc}`, `Makefile`, `config.mak.*`), **4 spaces** for `*.py`. Rust formatting is governed by `cargo fmt` defaults (no `rustfmt.toml`)
- C-side build (reference only): root `Makefile` + `config.mak.uname` with `NO_CURL` / `NEEDS_SSL_WITH_CRYPTO` / `NO_GETTEXT` / `CURL_LDFLAGS` knobs; optional system libs `libcurl`, OpenSSL/libcrypto, `libexpat`, `libpcre2`, `zlib`/`zlib-ng`, `libiconv`. None of these link into the Rust build

## Platform Requirements

**Development:**

- Rust stable ≥ 1.74 (`cargo msrv verify` gate), `cargo` + `rustc`
- System C git present (default `/usr/bin/git`; CI does `apt-get install -y git` in `.github/workflows/rust-port.yml:25,35`) for `differential` / `scoreboard` / `gen-fixtures`
- `make`-built C git at repo root optional (fresh oracle binaries); `t/` harness runs from `t/` (`./t1234-name.sh`, `GIT_TEST_INSTALLED` injection per `AGENTS.md`)
- Commands (all from `crates/`): `cargo build --workspace` → `crates/target/debug/git`; `cargo test --workspace`; `cargo xtask differential | gen-fixtures | scoreboard | gates`

**Production:**

- Single self-contained binary `git` built from the `git-cli` crate (`[[bin]] name = "git"`, `crates/git-cli/Cargo.toml:12-14`); no runtime, sidecar, database, or network service required
- Reports `git version 2.55.0-540` pinned in `crates/git-cli/src/lib.rs:8` - keep in sync when rebasing upstream
- Exit-code contract matches C git: usage errors `129` (`EXIT_USAGE`), unknown command `1` (`EXIT_NOT_FOUND`), broken pipe `141` (`EXIT_SIGPIPE`), all defined in `crates/git-cli/src/lib.rs:10-17`
- Deployment target: anywhere the static binary + `scripts/shim-git` dispatcher runs; unported commands fall through to system git via the `case` list in `scripts/shim-git:15`

---

*Stack analysis: 2026-09-25*
