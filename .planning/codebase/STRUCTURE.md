---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# Codebase Structure

**Analysis Date:** 2026-09-25

## Directory Layout

```
git.rs/                          # Repo root: UPSTREAM C git tree (reference oracle, v2.55.0-540)
├── builtin/                     # C command implementations (read-only reference)
├── t/                           # C test suite (behavior oracle, run via shim)
├── Documentation/               # C upstream docs + coding conventions
├── *.c / *.h                    # C git sources at root (object-file.c, refs.c, ...)
├── git / scalar / bin-wrappers/ # Committed C build artifacts (do not confuse with Rust binary)
├── Cargo.toml                   # STALE leftover (gitcore staticlib) — NOT the workspace
├── scripts/shim-git             # Dispatcher: ported cmds → Rust binary, rest → system git
├── watch-and-commit.sh          # Auto-commit helper — do NOT run (per AGENTS.md)
├── crates/                      # ACTIVE WORK: pure-Rust rewrite (real workspace)
│   ├── Cargo.toml               # Workspace manifest (members listed below)
│   ├── Cargo.lock               # Workspace lockfile
│   ├── scoreboard.json          # Committed t/-suite regression baseline (xtask-owned)
│   ├── git-cli/                 # `git` binary: thin entry + global-flag handling
│   ├── git-command/             # Per-command implementations + dispatch (composition root)
│   ├── git-hash/ git-varint/ git-date/          # Foundation leaves
│   ├── git-config/ git-object/ git-core/        # Mid: config, objects, repo/StringBuf
│   ├── git-commitgraph/ git-diff/ git-merge/    # Mid: graph, diff, merge
│   ├── git-index/ git-attributes/ git-pretty/   # Mid: index, attr/ignore, formatting
│   ├── git-odb/ git-refs/                       # Store: object DB + refs
│   ├── git-revision/ git-pathspec/              # Language: rev resolution, pathspec
│   ├── git-compress/ git-worktree/              # Access: zlib, worktree
│   ├── git-transport/ git-protocol/             # Access: network stubs
│   ├── git-hooks/ git-credentials/              # Access: hooks, credentials
│   ├── tests/                   # Crosswise differential suites + golden fixtures
│   └── xtask/                   # Automation (test/differential/scoreboard/gates)
├── docs/plan/                   # Canonical plan (README.md + phase blueprints + FOLLOWUPS.md)
├── graft/                       # Repo context graph (INDEX.md + per-system nodes)
└── .planning/codebase/          # This mapping output
```

## Directory Purposes

**Repo root (`*.c`, `*.h`, `builtin/`, `t/`, `Documentation/`):**

- Purpose: Upstream C git checkout — read to understand behavior, run to verify the port. Never linked, never modified for the port.
- Contains: C sources, shell test scripts (`t/t*.sh`), AsciiDoc docs.
- Key files: `builtin/<cmd>.c`, `t/README`, `Documentation/MyFirstContribution.txt`

**`crates/` (Rust workspace):**

- Purpose: All active Rust code. Run every cargo command from here (`cd crates && cargo …`).
- Contains: 24 workspace crates + integration `tests/` + `scoreboard.json` + `Cargo.lock`.
- Key files: `crates/Cargo.toml` (workspace members), `crates/Cargo.lock`, `crates/scoreboard.json`

**`crates/git-cli/`:**

- Purpose: The `git` binary. Only global flags, usage/version plumbing, and SIGPIPE handling.
- Contains: Exactly two files.
- Key files: `crates/git-cli/src/main.rs` (3-line entry), `crates/git-cli/src/lib.rs` (`VERSION`, `EXIT_*`, `PipeAwareWriter`, `run()`)

**`crates/git-command/`:**

- Purpose: Composition root. One module per ported builtin plus dispatch, context, error, and shared test helpers.
- Contains: ~45 command modules + `lib.rs` (~457 lines: trait, error, context, match table, test lock).
- Key files: `crates/git-command/src/lib.rs` (`Command`, `CommandError`, `RepoContext`, `dispatch_with`), `crates/git-command/src/checkout_core.rs` (shared checkout engine used by `checkout.rs`/`switch.rs`/`restore.rs`), `crates/git-command/src/{status,cat_file,log,rev_list,rev_parse,diff,index_pack,pack_objects,commit,init}.rs`

**`crates/git-odb/` (+ `src/pack/`):**

- Purpose: Object database: loose store + unified `Odb` reader + pack subsystem.
- Contains: `lib.rs` (`LooseStore`, `OdbError`) and `pack/{mod,file,index,midx,delta,write,crc32}.rs`.
- Key files: `crates/git-odb/src/lib.rs`, `crates/git-odb/src/pack/mod.rs`, `crates/git-odb/src/pack/file.rs`, `crates/git-odb/src/pack/index.rs`, `crates/git-odb/src/pack/midx.rs`, `crates/git-odb/src/pack/delta.rs`, `crates/git-odb/src/pack/write.rs`

**`crates/git-object/`, `crates/git-revision/`, `crates/git-refs/`, `crates/git-index/`:**

- Purpose: Parsed object model; revision resolution + history walking; ref storage; index file.
- Contains: Object parsers/serializers; `resolve.rs` + `rev_info.rs` + walk; loose/packed-refs backend; v2 index + cache tree.
- Key files: `crates/git-object/src/{lib,commit,tree}.rs`, `crates/git-revision/src/{lib,resolve,rev_info}.rs`, `crates/git-refs/src/lib.rs`, `crates/git-index/src/lib.rs`, `crates/git-index/src/cache_tree.rs`

**`crates/git-diff/`, `crates/git-merge/`, `crates/git-pretty/`:**

- Purpose: Diff algorithms + rendering; merge bases + 3-way merge; log/show formatting.
- Contains: Myers/tree/unified/userdiff; reachability + `merge3`; pretty date/format layer.
- Key files: `crates/git-diff/src/{lib,myers,tree,unified,userdiff}.rs`, `crates/git-merge/src/lib.rs`, `crates/git-pretty/src/{lib,date}.rs`

**`crates/git-hash/`, `crates/git-varint/`, `crates/git-date/`, `crates/git-config/`, `crates/git-core/`:**

- Purpose: Foundation: OIDs/hashing, varints, dates/tz, config parsing, repo discovery + `StringBuf`.
- Contains: Leaf codecs plus `Repository`/`RepoEnv` and the strbuf port.
- Key files: `crates/git-hash/src/{lib,sha1,sha1dc,sha256}.rs`, `crates/git-varint/src/lib.rs`, `crates/git-date/src/{lib,tz}.rs`, `crates/git-config/src/lib.rs`, `crates/git-core/src/lib.rs`, `crates/git-core/src/strbuf.rs`

**`crates/git-compress/`, `crates/git-commitgraph/`, `crates/git-attributes/`:**

- Purpose: zlib inflate/deflate; commit-graph files; attributes/ignore/wildmatch.
- Contains: Streaming codec; chunk format + bloom; attr/ignore matchers.
- Key files: `crates/git-compress/src/lib.rs`, `crates/git-commitgraph/src/{lib,commit_graph,chunk_format,bloom}.rs`, `crates/git-attributes/src/{lib,attributes,ignore,wildmatch}.rs`

**`crates/git-worktree/`, `crates/git-pathspec/`, `crates/git-transport/`, `crates/git-protocol/`, `crates/git-hooks/`, `crates/git-credentials/`:**

- Purpose: Access layer: worktree helpers, pathspec matching, fetch/push transport + protocol, hook execution, credential helpers.
- Contains: Mostly small single-`lib.rs` crates at varying maturity (transport/protocol are early stubs).
- Key files: `crates/git-worktree/src/lib.rs`, `crates/git-pathspec/src/lib.rs`, `crates/git-transport/src/lib.rs`, `crates/git-protocol/src/lib.rs`, `crates/git-hooks/src/lib.rs`, `crates/git-credentials/src/lib.rs`

**`crates/tests/`:**

- Purpose: Integration differential suites (Rust vs system C git, byte-identical assertions) + golden fixtures.
- Contains: `phase*_crosswise.rs`, `phaseA*_crosswise.rs`, `followups_crosswise.rs`, `fixtures/`.
- Key files: `crates/tests/phase4_crosswise.rs` … `crates/tests/phaseA12_crosswise.rs`, `crates/tests/fixtures/`

**`crates/xtask/` (+ `src/`):**

- Purpose: Workspace automation run as `cargo xtask <cmd>` from `crates/`.
- Contains: One module per subcommand/gate.
- Key files: `crates/xtask/src/main.rs`, `crates/xtask/src/{depcheck,drills,fixtures,isolation,placement,safety}.rs`, `crates/xtask/Cargo.toml`

**`docs/plan/`:**

- Purpose: Canonical phased roadmap and backlog. Start at `README.md`; actionable backlog in `FOLLOWUPS.md`.
- Contains: Phase blueprints `phase-0-foundation.md` … `phase-9-…md`, `phase-a/`, `phase-b/`, summaries, `gap-analysis.md`, `conversion-plan.md`, `test-infrastructure.md`, `REMAINING_TASKS.md`.
- Key files: `docs/plan/README.md`, `docs/plan/FOLLOWUPS.md`, `docs/plan/test-infrastructure.md`

**`graft/`:**

- Purpose: Prebuilt repo context graph (per-system nodes with file:line spans).
- Contains: `INDEX.md` plus topic nodes (e.g. `abspath.md`, `add-patch.md`, `apply.md`, …).
- Key files: `graft/INDEX.md`

**`scripts/` + `crates/.cargo/`:**

- Purpose: Test dispatcher and local cargo config.
- Contains: `scripts/shim-git` (routes ported commands to `RUST_GIT`, rest to `SYSTEM_GIT`).
- Key files: `scripts/shim-git`, `crates/.cargo/` config

## Key File Locations

**Entry Points:**

- `crates/git-cli/src/main.rs`: Process entry (`main` → `git_cli::run` → exit code).
- `crates/git-cli/src/lib.rs`: `VERSION` (= `"2.55.0-540"`, pinned to the C tree), `EXIT_USAGE`/`EXIT_NOT_FOUND`/`EXIT_SIGPIPE`, `run()`.
- `crates/git-command/src/lib.rs`: `Command` trait, `CommandError`, `RepoContext`, `dispatch`/`dispatch_with`, `resolve_arg`, test `CWD_LOCK`.
- `crates/xtask/src/main.rs`: `cargo xtask` subcommands.

**Configuration:**

- `crates/Cargo.toml`: Workspace members, `workspace.package` (edition 2021, rust 1.74, GPL-2.0-only), release profile.
- `crates/<name>/Cargo.toml`: Per-crate deps — the source of truth `depcheck` enforces layering from.
- `crates/.cargo/`: Local cargo configuration.
- Root `Cargo.toml`: STALE — do not use; always run cargo from `crates/`.

**Core Logic:**

- `crates/git-core/src/lib.rs`: `RepoEnv`, `Repository::discover_from`, config/hash-algo wiring.
- `crates/git-odb/src/lib.rs` + `crates/git-odb/src/pack/`: All object storage.
- `crates/git-revision/src/resolve.rs`: Revision-name → oid resolution.
- `crates/git-command/src/checkout_core.rs`: Shared checkout engine (used by checkout/switch/restore, not a user command itself).

**Testing:**

- `crates/tests/*crosswise.rs`: Differential suites (registered for `cargo xtask differential`).
- `crates/tests/fixtures/`: Golden fixtures (regenerate with `cargo xtask gen-fixtures`).
- `crates/scoreboard.json`: Committed `t/`-suite baseline (regenerate with `cargo xtask scoreboard`; never hand-edit).
- Unit tests: inline `#[cfg(test)] mod tests` per file (e.g. `crates/git-cli/src/lib.rs:122`); property tests co-located with `proptest` dev-deps.

## Naming Conventions

**Files:**

- Crate dirs are kebab-case matching the import name: `crates/git-commitgraph/` → `git_commitgraph` (`crates/git-commitgraph/src/commit_graph.rs` shows the snake_case module-file rule).
- Command modules are snake-case of the C builtin: `git show-ref` → `crates/git-command/src/show_ref.rs`; hyphenated builtins map `-` → `_` (`merge-base` → `merge_base.rs`, `commit-graph` → `commit_graph.rs`).
- Non-command helpers use descriptive snake_case: `crates/git-command/src/checkout_core.rs`, `crates/git-command/src/ignore_util.rs`; pack subsystem splits by concern: `crates/git-odb/src/pack/{file,index,midx,delta,write,crc32}.rs`.
- Every library crate has `src/lib.rs` with crate-level `//!` docs naming the C file it ports (e.g. "subset of `revision.c`", "port of `read-cache.c`"); the binary crate adds `src/main.rs`.
- `xtask` automation modules match subcommand names: `crates/xtask/src/{depcheck,drills,fixtures,isolation,placement,safety}.rs`.

**Directories:**

- `crates/<crate-name>/src/`: All Rust sources; `crates/<crate-name>/tests/` (when present): integration tests for that crate.
- `crates/tests/`: Workspace-level crosswise suites named `<phase>_crosswise.rs` (`phase4_crosswise.rs` … `phase9_crosswise.rs`, `phaseA01_crosswise.rs` … `phaseA12_crosswise.rs`).
- `docs/plan/phase-<n>-<topic>.md`: Phase blueprints; `docs/plan/archive/`: superseded drafts (do not work from these).

## Where to Add New Code

**New Feature (ported builtin `git <name>`):**

- Primary code: new module `crates/git-command/src/<snake_name>.rs` with `pub struct <Name>; impl Command for <Name>` following e.g. `crates/git-command/src/cat_file.rs`.
- Tests: inline `#[cfg(test)]` unit tests in the module + a crosswise suite in `crates/tests/` (register in xtask suites), run C-side via `t/` through the shim.

**New Component/Module:**

- Implementation: new file under the owning crate's `src/` (e.g. pack feature → `crates/git-odb/src/pack/<topic>.rs`, re-export from `crates/git-odb/src/pack/mod.rs`); declare `pub mod <topic>;` in that crate's `lib.rs`.
- Wiring checklist for a new command (all required): `pub mod` + match arm in `crates/git-command/src/lib.rs` (`dispatch_with`), case entry in `scripts/shim-git`, crosswise suite in `crates/tests/`, keep `crates/git-cli/src/lib.rs:VERSION` in sync on upstream rebases.

**Utilities:**

- Shared helpers: `crates/git-core/src/` (repo-wide, e.g. `strbuf.rs`) or the closest domain crate (`crates/git-command/src/ignore_util.rs`, `crates/git-command/src/ident.rs` for command-shared code; `crates/git-diff/src/userdiff.rs` for diff-shared code). Prefer `StringBuf` (`crates/git-core/src/strbuf.rs`) where strbuf semantics are needed, per `STRINGBUF_IMPLEMENTATION.md`.

**New library crate:**

- Implementation: `crates/<kebab-name>/src/lib.rs` + `crates/<kebab-name>/Cargo.toml`, add to `members` in `crates/Cargo.toml`. Pick the `depcheck` layer deliberately (`crates/xtask/src/depcheck.rs: `git-hash|git-varint|git-date`→0, domain→1, `git-odb|git-refs`→2, `git-revision|git-pathspec`→3, access→4, `git-command|git-cli`→5) and depend only downward. `git-cli` may depend only on `git-command`; nothing may depend on `xtask`.

## Special Directories

**`crates/target/`:**

- Purpose: Rust build output (`crates/target/debug/git` is the built binary).
- Generated: Yes
- Committed: No

**Repo-root build artifacts (`*.o`, `.depend/`, `bin-wrappers/`, `scalar`, `git` binary at root):**

- Purpose: Leftovers from a previous in-tree C `make` — the C oracle binaries.
- Generated: Yes
- Committed: Yes (present in working tree) — do not confuse `git` at root with `crates/target/debug/git`.

**`crates/tests/fixtures/`:**

- Purpose: Golden on-disk fixtures for differential tests.
- Generated: Yes (via `cargo xtask gen-fixtures`)
- Committed: Yes

**`graft/`:**

- Purpose: Indexed context graph for navigation (`graft ask`, `graft skeleton`, `graft callers`).
- Generated: Yes (refresh with `graft build` after big changes; auto-refreshes per query)
- Committed: Yes

**`docs/plan/archive/`:**

- Purpose: Superseded drafts (`rust_conversion_plan.md`, `rust_test_template.md`).
- Generated: No
- Committed: Yes — read-only history, never build from these.

---

*Structure analysis: 2026-09-25*
