# Phase 0 — Foundation (Part 1)

## Goal

Establish the standalone Rust workspace, the object-ID/hash abstraction every
later phase builds on, and the minimal repository-discovery + CLI plumbing
needed to run real `t/` tests through the shim. No repository object I/O in
this phase.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-hash`: `Oid`, `HashAlgorithm`, `CryptoHasher` (port `src/hash.rs`, **drop FFI**) | `hash.h`, `hash.c`, `hash-lookup.c`, `sha1dc/`, `sha256/` |
| `git-date`: date parse/format | `date.c`, `date.h` |
| `git-config`: `ConfigSet`, includes, conditionals, `-c` | `config.c`, `config.h` |
| `git-core`: repo discovery, common dir, env | `setup.c`, `environment.c`, `repository.c` |
| `git-cli`: dispatch, options, `--version` | `git.c`, `parse-options.c`, `parse-options.h` |

## On-disk formats / surfaces

- Git config syntax (multi-file, `[section] key = value`, quotes, `[include]`,
  `[includeIf "gitdir:..."]`, `-c` overrides).
- Repo layout: `.git` dir, `gitdir` file (worktrees/submodules), commondir,
  `GIT_DIR`/`GIT_WORK_TREE`/`GIT_COMMON_DIR` env, `--git-dir`/`--work-tree`.
- Hash algorithm ids, format ids, hex/abbrev object-id rendering.

## Porting notes

- `src/hash.rs` already defines `Oid`/`HashAlgorithm` with the dual-hash layout
  (`hash: [u8; 32]`, `algo: u32`, `repr(C)` not needed once FFI is gone).
  Port it; replace the `CryptoHasher` FFI (`c::git_hash_*`) with a pure-Rust
  sha1dc implementation (collision-detecting SHA-1) + SHA-256.
- Keep the `#[repr(C)]` `ObjectID` shape conceptually (byte[32] + algo) so the
  LMAP design in `src/loose.rs` ports cleanly in Phase 9.

## Fully automated test plan

### `git-hash`
- **Unit:** SHA-1 and SHA-256 known vectors (FIPS test vectors); `empty_blob`,
  `empty_tree`, `null_oid` constants asserted against the known C git values;
  incremental `update` == one-shot `final`; `clone` isolation; `std::io::Write`
  impl; hex/abbrev rendering and parsing; `from_u32`/`from_format_id` round-trips
  (port the tests already in `src/hash.rs`).
- **Property (`proptest`):** any byte slice length 0..=2^16 → hash equals C
  `git hash-object` output (differential property, hashed through the C binary).
- **Collision detection:** commit a SHA-1 collision-block fixture; assert the
  hasher behaves exactly as sha1dc does (mirror `t/t0013-sha1dc.sh` as a Rust
  test).

### `git-date`
- **Unit:** ISO 8601, RFC 2822, Unix epoch, relative ("2 days ago"),
  "yesterday/now/tomorrow", approxidate forms, format specifiers.
- **Differential:** via `git-test date`, replay the `t0006-date.sh` table;
  Rust == C `test-tool date` output.

### `git-config`
- **Unit:** section/key parsing, multi-value, `[include]` (file + glob),
  `[includeIf "gitdir:..."]`, `-c` overrides, `core.*` typed access, quoting,
  syntax error handling.
- **Property:** proptest-generated syntactically valid config round-trips
  `--list`; fuzz (no panic) on arbitrary garbage.
- **Differential:** `git config --list --show-origin` Rust == C on fixture
  repos covering the `t1300` scenarios.

### `git-core`
- **Unit:** `.git` dir discovery, `gitdir`-file indirection, commondir,
  worktree discovery, `GIT_DIR`/`GIT_WORK_TREE`/`--git-dir` precedence.
- **Property:** generated directory layouts → discovery agrees with C
  (`git rev-parse --git-dir` / `--show-toplevel`).

### `git-cli`
- **Unit:** dispatch table, `--version` snapshot, unknown-command error parity.

### `git-test` additions
- `test-sha1`, `test-sha256`, `test-date`, `test-config`.

### Differential harness
- Wire `tests/differential/`, `scripts/shim-git`, `cargo xtask`, CI jobs, and
  `tests/fixtures/` for the first time (see test-infrastructure.md). This
  phase pays the setup cost so later phases are green out of the box.

## Gate criteria

- `cargo xtask test` green (unit + doc + proptest).
- Coverage ≥ 90% on git-hash, git-date, git-config, git-core, git-cli.
- `cargo xtask differential` green.
- `cargo xtask scoreboard` baseline established; no regressions.
- **t/ scripts pass 100%:** `t0001` (setup subset), `t0006`, `t1013`, `t1300`
  (+ `t1308` conditional includes).

## Risks

- sha1dc port correctness (collision detection must match C behavior exactly).
- Config include semantics (glob, `gitdir` conditional, escape rules) are subtle.
- Env-var + option precedence rules in repo discovery are easy to get wrong —
  differential-test liberally.
