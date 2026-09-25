---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# External Integrations

**Analysis Date:** 2026-09-25

## APIs & External Services

**None (no SaaS / cloud / third-party APIs):**

- No Stripe, Supabase, AWS, auth provider, analytics, or monitoring SDK in `crates/Cargo.lock` or any `crates/*/Cargo.toml`. The workspace's only crates.io dependencies are `flate2`, `regex`, `anyhow`, `thiserror` (+ `proptest` dev-only); everything else is `std` or intra-workspace `path` deps
- No HTTP client exists: `reqwest` / `hyper` / `curl` / `ureq` are all absent. Do not add one without a fetch/push plan amendment (`docs/plan/README.md:41-43` puts network/transport in the out-of-scope Phase 10+ stretch)

**Behavior oracle (the one true "external service"):**

- System C git binary - byte-identical oracle for every crosswise suite
  - Default path: `/usr/bin/git`, overridable via `SYSTEM_GIT` env var (`scripts/shim-git:10`)
  - How tests use it: `crates/git-odb/tests/pack_crosswise.rs`, `crates/git-odb/tests/graph_midx_crosswise.rs`, `crates/git-command/tests/phase*_crosswise.rs` shell out to both binaries on identical inputs and assert identical stdout/stderr/exit code
  - Candidate discovery in automation: `["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"]` probed in `crates/xtask/src/main.rs:336-343` (`git_binary()`)
  - Fixture generation shells out to it: `run_ok(git, &dir, &args)` in `crates/xtask/src/main.rs:345-352` (`init`, `add`, `commit`, `repack`, `multi-pack-index write`, `commit-graph write`)
  - Validation direction is bidirectional: C git must accept Rust-written artifacts (`git fsck`, `verify-pack`, `commit-graph verify`, `multi-pack-index verify`) and Rust must read C-written repos

**C build tree (reference side only):**

- Upstream C sources at repo root (`*.c`, `builtin/`, `*.h`) plus `t/` shell suite and `Documentation/`
- Optional system libs for the C build (configured via root `Makefile` knobs `NO_CURL`, `NEEDS_SSL_WITH_CRYPTO`, `NO_GETTEXT`, `CURL_LDFLAGS`): `libcurl`, OpenSSL/libcrypto, `libexpat`, `libpcre2`, `zlib`/`zlib-ng`, `libiconv`/gettext. These link into C git only - the Rust workspace links none of them (pure-Rust `miniz_oxide` backend via `flate2` instead of system zlib; vendored SHA in `crates/git-hash/src/` instead of OpenSSL)

## Data Storage

**Databases:**

- None. No SQLite/Postgres/MySQL/ORM. The "database" is the git on-disk format itself, accessed via filesystem I/O:
  - Loose objects + packs/idx/revindex/delta: `crates/git-odb/src/lib.rs`
  - MIDX / bitmaps / commit-graph / cruft: `crates/git-odb/src/lib.rs` + `crates/git-commitgraph/src/lib.rs`
  - Refs files backend + `packed-refs`: `crates/git-refs/src/lib.rs`
  - Index v2 read/write: `crates/git-index/src/lib.rs`
  - Connection: no connection string; repository located by directory discovery in `crates/git-core/src/lib.rs` (including `gitdir:` files)
  - Client: `std::fs` / `std::io` only

**File Storage:**

- Local filesystem only. Work-tree materialization (file creation, modes, symlinks, stat refresh) in `crates/git-worktree/src/lib.rs`; hook scripts resolved under `$GIT_DIR/hooks/` in `crates/git-hooks/src/lib.rs:83-100`
- Golden fixtures (generated, committed): `crates/tests/fixtures/` via `cargo xtask gen-fixtures` (`crates/xtask/src/main.rs:281-334`); regression baseline `crates/scoreboard.json` (committed, machine-updated by `cargo xtask scoreboard` - never hand-edit)

**Caching:**

- None. No Redis/Memcached/in-process cache layer. Pack negotiation state (`Negotiation` in `crates/git-transport/src/lib.rs:126-163`) and progress sinks (`Progress` in `crates/git-transport/src/lib.rs:102-120`) are per-operation structs, not shared caches

## Authentication & Identity

**Auth Provider:**

- None (no OAuth/OIDC/password provider, no token service)

**Credential handling (C-git helper scope, no secrets stored):**

- Implementation: `crates/git-credentials/src/lib.rs` - "secret lookup, caching, prompting, and redaction (C-git helper scope)". Models the credential-helper request/response to shell out to configured helpers; redacts secrets from output. No `*.pem`/`*.key`/token files are read or committed
- Commit identity (author/committer name+email+timestamp) parsed/applied via `crates/git-command/src/ident.rs` and formatted by `crates/git-date/src/lib.rs` + `crates/git-pretty/src/lib.rs` - local config/env, not an identity provider

## Monitoring & Observability

**Error Tracking:**

- None (no Sentry/Datadog/PagerDuty). Errors are typed enums implementing `std::error::Error` per crate (e.g. `CompressError` in `crates/git-compress/src/lib.rs:17-36`, `TransportError` in `crates/git-transport/src/lib.rs:18-37`, `ProtocolError` in `crates/git-protocol/src/lib.rs:13-20`) surfaced as `CommandError { code, message }` with C-matching exit codes via `crates/git-command/src/lib.rs` and `crates/git-cli/src/lib.rs`

**Logs:**

- stderr messages + exit codes only (byte-compared against C git in crosswise tests). No structured logging framework (`tracing`/`log`/`env_logger` all absent). Progress reporting shape mirrors C git's `Counting/Compressing/Receiving objects` percent+counts via `ProgressEvent` in `crates/git-transport/src/lib.rs:81-94`, rendered at the CLI edge

## CI/CD & Deployment

**Hosting:**

- No app hosting (no Vercel/Fly/AWS). Artifact is the `git` binary from `crates/git-cli` (`[[bin]] name = "git"`, `crates/git-cli/Cargo.toml:12-14`)

**CI Pipeline:**

- GitHub Actions, Rust side (`.github/workflows/rust-port.yml`): three jobs on `ubuntu-latest` with `dtolnay/rust-toolchain@stable` -
  1. `unit`: `cargo test --workspace --manifest-path crates/Cargo.toml` + `cargo clippy ... -D warnings || true`
  2. `differential`: `apt-get install -y git` then `cargo run -p xtask --manifest-path crates/Cargo.toml -- differential`
  3. `scoreboard`: same setup, `... -- scoreboard` (fails on regression vs `crates/scoreboard.json`)
- Shared style gates (`.github/workflows/main.yml:473-487` `rust-analysis` job): `ci/run-rust-checks.sh` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -Dwarnings`, `cargo msrv verify`
- C-side CI untouched by the port: `.gitlab-ci.yml`, `.cirrus.yml`, plus `check-style.yml`, `check-whitespace.yml`, `coverity.yml`, `l10n.yml` under `.github/workflows/`
- Deployment mechanism while the port is incomplete: `scripts/shim-git` routes the 40+ ported commands (`version|init|add|commit|...|check-ignore|check-attr`, full list at `scripts/shim-git:15`) to `$RUST_GIT` (`crates/target/debug/git` default) and everything else to `$SYSTEM_GIT`. When porting a command, add it to that `case` list, wire `git-command::dispatch`, and register a crosswise suite in `suites()` (`crates/xtask/src/main.rs:102-225`)

## Environment Configuration

**Required env vars:**

- None. The binary runs with zero mandatory environment. All of the following are optional overrides:
  - `RUST_GIT`, `SYSTEM_GIT` - shim/test dispatcher paths (`scripts/shim-git:9-10`)
  - `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY` (+ `--git-dir`/`--work-tree`/`--bare` flags) - repository location, read in `crates/git-core/src/lib.rs:64-79`; `GIT_DIR`/`GIT_WORK_TREE` interplay validated in `crates/git-command/src/init.rs:540-563`
  - `GIT_DIR`, `GIT_INDEX_FILE`, etc. re-exported to hook children in `crates/git-hooks/src/lib.rs:102-150`
  - Crosswise tests scrub/isolate env (`crates/xtask/src/isolation.rs:67` covers `GIT_DIR/WORK_TREE/CEILING_DIRECTORIES` + config overrides; `crates/git-command/tests/phaseB01_crosswise.rs:50-58` enumerates the scrubbed set)

**Secrets location:**

- Nowhere in-repo. No `.env*` files exist (`ls .env*` empty). Never create or quote `credentials.*`, `*secret*`, `*.pem`/`*.key`, or `serviceAccountKey.json`-style files - note existence only per policy (leaked values would be committed to git)

## Webhooks & Callbacks

**Incoming:**

- None. No HTTP server, no webhook endpoints, no socket listeners. `git-transport` explicitly owns "negotiation state machine and progress reporting so both can be tested without a connection" (`crates/git-transport/src/lib.rs:1-10`); `Endpoint::parse` (`crates/git-transport/src/lib.rs:50-58`) only *classifies* `https://...` / `user@host:path` selectors as `Url` vs `LocalPath` - no connection is opened

**Outgoing:**

- None. No HTTP calls, no API posts, no telemetry. The closest outbound mechanism is process spawning, all local:
  - Hooks: `crates/git-hooks/src/lib.rs:143-150` spawns `$GIT_DIR/hooks/<name>` via `std::process::Command` with `GIT_DIR` in env
  - Credential helpers / editors / pagers: spawned via `std::process::Command` from `crates/git-credentials/src/lib.rs`, `crates/git-command/src/ident.rs`, `crates/git-core/src/lib.rs`, and command modules (`init.rs`, `commit.rs`, `checkout_core.rs`, `cat_file.rs`, ...). No network traffic results from any of these
- Wire framing without I/O: `crates/git-protocol/src/lib.rs` implements packet-line encode/decode, capability advertisement, and v0/v1/v2 negotiation over plain byte buffers ("canned streams in tests ... verified without any connection"). Actual fetch/push transport is Phase 10+ stretch scope, not implemented

---

*Integration audit: 2026-09-25*
