---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
<!-- refreshed: 2026-09-25 -->

# Architecture

**Analysis Date:** 2026-09-25

## System Overview

```text
┌─────────────────────────────────────────────────────────────┐
│                    Surface (CLI entry)                       │
│  `crates/git-cli/src/main.rs` → `crates/git-cli/src/lib.rs` │
│  `git_cli::run(args) -> i32`                                 │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│              Composition root (command dispatch)             │
│  `crates/git-command/src/lib.rs`                             │
│  `RepoContext` → `dispatch_with()` → `Command::run()`        │
│  one module per builtin: `cat_file.rs`, `status.rs`, …       │
└───────┬──────────────┬───────────────┬──────────────────────┘
        │              │               │
        ▼              ▼               ▼
┌──────────────┐ ┌────────────┐ ┌──────────────────┐
│  Language /  │ │  Domain /  │ │  Access /        │
│  query layer │ │  model     │ │  presentation    │
│`crates/git-  │ │`crates/git-│ │`crates/git-diff/`│
│revision/`    │ │object/`,   │ │`crates/git-      │
│`crates/git-  │ │`crates/git-│ │pretty/`,         │
│pathspec/`    │ │merge/`     │ │`crates/git-      │
└──────┬───────┘ └─────┬──────┘ │worktree/`        │
       │               │        └──────────────────┘
       ▼               ▼
┌─────────────────────────────────────────────────────────────┐
│                    Store layer                               │
│  `crates/git-odb/` (loose + `pack/`: file/idx/midx/delta)   │
│  `crates/git-refs/` (loose refs + packed-refs)              │
│  `crates/git-index/` (v2 index + cache_tree)                │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Foundation / mid libraries                   │
│  `crates/git-core/` (`Repository`, `RepoEnv`, `StringBuf`)  │
│  `crates/git-hash/` (`Oid`, `HashAlgorithm`)                │
│  `crates/git-config/` (`ConfigSet`) `crates/git-date/`      │
│  `crates/git-varint/` `crates/git-compress/`                │
│  `crates/git-commitgraph/` `crates/git-attributes/`         │
└──────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│  On-disk state (.git/) + C git oracle at repo root          │
│  `objects/`, `refs/`, `index`, `config`, `HEAD`              │
│  C tree: `builtin/`, `t/`, `Documentation/` (reference only) │
│  Automation: `crates/xtask/src/`, `scripts/shim-git`         │
└─────────────────────────────────────────────────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| `git-cli` binary entry | Process entry, exit-code plumbing only | `crates/git-cli/src/main.rs` |
| `git_cli::run` | Global flags (`--version`, `--help`, `--exec-path`), global-option parsing, stdout `PipeAwareWriter` (SIGPIPE→141), calls `dispatch_with` | `crates/git-cli/src/lib.rs` |
| `git-command` dispatch | `RepoContext`, `Command` trait, `CommandError`, `dispatch`/`dispatch_with` match table over ~45 subcommands | `crates/git-command/src/lib.rs` |
| Per-command modules | One builtin each (`Status`, `CatFile`, `Log`, …), arg parsing + output to caller-supplied writer | `crates/git-command/src/*.rs` (e.g. `status.rs`, `cat_file.rs`, `log.rs`, `rev_list.rs`) |
| `git-core` repo layer | `Repository::discover_from`, `RepoEnv` overrides, `StringBuf` (strbuf port) | `crates/git-core/src/lib.rs`, `crates/git-core/src/strbuf.rs` |
| `git-odb` object store | `LooseStore`, `Odb` unified reader, pack reading/writing, idx/midx, delta resolution | `crates/git-odb/src/lib.rs`, `crates/git-odb/src/pack/mod.rs`, `crates/git-odb/src/pack/{file,index,midx,delta,write,crc32}.rs` |
| `git-object` model | `ObjectKind`, loose header, blob/tree/commit/tag parse + serialize | `crates/git-object/src/lib.rs`, `crates/git-object/src/{commit,tree}.rs` |
| `git-revision` queries | `Resolver` (get_oid subset), `RevWalk`, `rev_info` | `crates/git-revision/src/{lib,resolve,rev_info}.rs` |
| `git-refs` | Loose + packed-refs reader/writer, symref resolution | `crates/git-refs/src/lib.rs` |
| `git-index` | v2 index read/write, `IndexEntry`, `CacheTree` | `crates/git-index/src/lib.rs`, `crates/git-index/src/cache_tree.rs` |
| `git-diff` / `git-merge` | Tree compare, Myers line diff, unified render, userdiff drivers; merge bases + 3-way `merge3` | `crates/git-diff/src/{lib,myers,tree,unified,userdiff}.rs`, `crates/git-merge/src/lib.rs` |
| Foundation leaves | Hashing, varints, dates, config, compression, commit-graph, attributes/ignore | `crates/git-hash/src/lib.rs`, `crates/git-varint/src/lib.rs`, `crates/git-date/src/lib.rs`, `crates/git-config/src/lib.rs`, `crates/git-compress/src/lib.rs`, `crates/git-commitgraph/src/lib.rs`, `crates/git-attributes/src/lib.rs` |
| Access helpers | Worktree/checkout support, pathspec, transport/protocol stubs, hooks, credentials, pretty formatting | `crates/git-worktree/src/lib.rs`, `crates/git-pathspec/src/lib.rs`, `crates/git-transport/src/lib.rs`, `crates/git-protocol/src/lib.rs`, `crates/git-hooks/src/lib.rs`, `crates/git-credentials/src/lib.rs`, `crates/git-pretty/src/lib.rs` |
| `xtask` automation | `test`, `differential`, `gen-fixtures`, `scoreboard`, `gates` (`depcheck`, `safety`, `placement`), `drills`, `isolation` | `crates/xtask/src/main.rs`, `crates/xtask/src/{depcheck,drills,fixtures,isolation,placement,safety}.rs` |
| Shim dispatcher | Routes ported commands to Rust binary, rest to system C git (how `t/` runs during the port) | `scripts/shim-git` |

## Pattern Overview

**Overall:** Layered library workspace + thin CLI + composition-root dispatcher (mirrors C `builtin/` layout, no FFI).

**Key Characteristics:**

- `crates/git-cli/src/main.rs` is 3 lines; all logic lives in `git_cli::run` and `git_command::dispatch_with` — commands are unit structs implementing the `Command` trait (`crates/git-command/src/lib.rs:333`).
- Strict dependency layering enforced mechanically by `cargo xtask depcheck` (`crates/xtask/src/depcheck.rs`): edges must point downward foundation → mid → store → language → access → surface, acyclic, `git-cli` depends only on `git-command`, nothing depends on `xtask`.
- Byte-compatibility is the contract: on-disk formats must round-trip with C git both directions; differential tests assert byte-identical stdout/stderr/exit codes (`crates/tests/*crosswise.rs`).
- C tree at the repo root is a read-only behavior oracle, never linked or called.

## Layers

**Foundation (rank 0):**

- Purpose: Leaf primitives with zero internal dependencies.
- Location: `crates/git-hash/`, `crates/git-varint/`, `crates/git-date/`
- Contains: `Oid`/`HashAlgorithm`/`CryptoDigest` (SHA-1 incl. `sha1dc`, SHA-256), varint codec, date parsing/timezones.
- Depends on: std + (proptest for dev).
- Used by: everything above.

**Mid / domain model (rank 1):**

- Purpose: Core git data types and pure algorithms.
- Location: `crates/git-config/`, `crates/git-object/`, `crates/git-core/`, `crates/git-commitgraph/`, `crates/git-diff/`, `crates/git-merge/`, `crates/git-index/`, `crates/git-attributes/`, `crates/git-pretty/`
- Contains: `ConfigSet`, `Object`/`Commit`/`TreeEntry`, `Repository`/`RepoEnv`/`StringBuf`, commit-graph chunks, Myers/unified diff, merge bases, index entries, wildmatch/ignore.
- Depends on: foundation only.
- Used by: store, language, and command layers.

**Store (rank 2):**

- Purpose: Persistent state: object database, refs, index I/O against `.git/`.
- Location: `crates/git-odb/`, `crates/git-refs/` (index lives in `crates/git-index/` but is consumed here)
- Contains: `LooseStore`, `Odb`, `PackFile`/`PackIndex`/`Midx`, delta codec, pack writer; `RefStore` loose + packed-refs.
- Depends on: mid + foundation (`git-odb` → `git-hash`, `git-object`, `git-core`, `git-commitgraph`, `git-compress`).
- Used by: `git-revision`, `git-command`.

**Language / query (rank 3):**

- Purpose: Revision expressions, history walking, pathspec matching.
- Location: `crates/git-revision/`, `crates/git-pathspec/`
- Contains: `Resolver`/`ResolveError` (full/abbrev oid, ref names, `~`/`^` peels, ambiguity detection), `RevWalk`/`WalkOptions`, pathspec matcher.
- Depends on: store + mid + foundation.
- Used by: `git-command` modules (`rev-list`, `log`, `show`, `diff-tree`) via `resolve_arg` helper (`crates/git-command/src/lib.rs:353`).

**Access (rank 4):**

- Purpose: I/O helpers around the store: compression, worktree/checkout, transport/protocol, hooks, credentials.
- Location: `crates/git-compress/`, `crates/git-worktree/`, `crates/git-transport/`, `crates/git-protocol/`, `crates/git-hooks/`, `crates/git-credentials/`
- Contains: zlib inflate/deflate (`git-compress`), worktree layout helpers, fetch/push stubs, hook runner, credential helper.
- Depends on: anything at or below store.
- Used by: `git-command`.

**Surface / composition root (rank 5):**

- Purpose: CLI parsing and command composition; the only place that wires everything together.
- Location: `crates/git-cli/`, `crates/git-command/`
- Contains: `git_cli::run`, `RepoContext::from_global_args`, `Command` trait, `dispatch_with` match table, `CommandError` with exit codes.
- Depends on: all layers below (sole allowed upward-composition exception).
- Used by: the `git` binary and `scripts/shim-git`; tests call `dispatch`/`dispatch_with` directly with an in-memory writer.

**Automation (exempt):**

- Purpose: Dev tooling, not shipped: differential/scoreboard harness, fixture generation, structural gates.
- Location: `crates/xtask/`
- Contains: `differential()`, `scoreboard()`, `depcheck::run()`, `safety::run()`, `placement::run()`.
- Depends on: anything (exempt from layer rule); nothing may depend on it.

## Data Flow

### Primary Request Path

1. Process entry — `fn main` forwards `std::env::args()` and exits with the returned code (`crates/git-cli/src/main.rs:1`).
2. Global handling — `git_cli::run` handles `--version`/`help`/`--exec-path` inline, else parses `-C`/`-c`/`--git-dir`/`--work-tree`/`--common-dir`/`--bare` into a `RepoContext` and splits off the subcommand (`crates/git-cli/src/lib.rs:52`, `crates/git-command/src/lib.rs:178`).
3. Dispatch — `dispatch_with(&ctx, &cmd, &sub, &mut stdout)` matches the subcommand name to a `&dyn Command` and calls `cmd.run(ctx, args, out)` with a `PipeAwareWriter` for SIGPIPE→141 semantics (`crates/git-command/src/lib.rs:373`, `crates/git-cli/src/lib.rs:93`).
4. Repository resolution — the command calls `ctx.repository()`, which builds a `RepoEnv` (including `GIT_*` env overrides) and runs `Repository::discover_from` (walk up for `.git`/`gitdir:` files, read `commondir`, load `config`, apply `-c` overlays) (`crates/git-command/src/lib.rs:271`, `crates/git-core/src/lib.rs:115`).
5. Store access — the command opens `Odb`/`LooseStore`/`PackFile`, `RefStore`, and/or `Index` rooted at the discovered paths, resolves revisions via `git_revision::Resolver`, walks with `RevWalk`, diffs with `git-diff`, and writes primary output to `out: &mut dyn Write` (`crates/git-odb/src/lib.rs`, `crates/git-revision/src/resolve.rs`, e.g. `crates/git-command/src/status.rs`).
6. Exit-code return — `Ok(())` → `0` (or `141` if the pipe broke); `Err(CommandError)` → `e.code` after printing `e.message` to stderr; unknown command → `1` with `git: '…' is not a git command` (`crates/git-cli/src/lib.rs:98`, `crates/git-command/src/lib.rs:67`).

### Revision Resolution Flow

1. Command calls `resolve_arg(repo, s)` helper (`crates/git-command/src/lib.rs:353`).
2. `git_revision::Resolver::new(repo)` builds a resolver over `Odb` + `RefStore` (`crates/git-revision/src/resolve.rs`).
3. Resolution tries full oid → abbrev prefix (with ambiguity scan) → ref name → `<rev>~n`/`^n` peels; failures render C-exact stderr text via `ResolveError::render()` and surface as exit-128 `CommandError::fatal`.

### Diff / Log Flow

1. `log`/`rev-list` build a `RevWalk` with a commit-loader closure over `Odb` (`crates/git-revision/src/lib.rs`, `crates/git-command/src/log.rs`, `crates/git-command/src/rev_list.rs`).
2. Tree pairs are compared with `git_diff::tree::compare_trees`, blobs with Myers `diff_lines` and rendered by `render_unified`, formatted by `git-pretty` (`crates/git-diff/src/lib.rs`, `crates/git-pretty/src/lib.rs`).

**State Management:**

- No server, no long-lived process: each invocation is stateless; all state is on disk (`.git/objects`, `refs/`, `index`, `config`, `HEAD`) plus per-invocation `RepoContext`/`Repository` values threaded explicitly — commands must take `&RepoContext` and call `ctx.repository()`, never process-global state.
- Test-only shared state: `CWD_LOCK` serializes tests that mutate cwd/env (`crates/git-command/src/lib.rs:437`); object-file temp naming uses an `AtomicU32 TEMP_COUNTER` (`crates/git-odb/src/lib.rs`).

## Key Abstractions

**Repository / RepoEnv / RepoContext:**

- Purpose: Explicit, testable repository location + configuration.
- Examples: `crates/git-core/src/lib.rs:56` (`RepoEnv`), `crates/git-core/src/lib.rs:88` (`Repository`), `crates/git-command/src/lib.rs:144` (`RepoContext`)
- Pattern: Builder-from-environment + explicit threading; `RepoContext::at(dir)` for tests, `from_global_args` for CLI parity with C global flags.

**Command + CommandError:**

- Purpose: Uniform subcommand interface with C-exact exit codes.
- Examples: `crates/git-command/src/lib.rs:333` (trait), `crates/git-command/src/lib.rs:67` (error), every `crates/git-command/src/*.rs`
- Pattern: `fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError>`; `usage()`→129, `fatal()`→128, `error()`→1, `silent(code)` for diff-style non-zero success.

**Odb (unified object reader):**

- Purpose: Single read path across loose objects, packs, idx, midx, commit-graph.
- Examples: `crates/git-odb/src/lib.rs` (`LooseStore`, `OdbError`), `crates/git-odb/src/pack/mod.rs` (`Odb`, `PackError`), `crates/git-odb/src/pack/{file,index,midx,delta,write}.rs`
- Pattern: `Odb::for_repo(&repo)`-style construction, then `read(oid) -> Object`; pack layer handles `EntryKind`, delta base resolution, checksum validation.

**Object model:**

- Purpose: Parsed, hash-verified git objects.
- Examples: `crates/git-object/src/lib.rs` (`ObjectKind`, header), `crates/git-object/src/commit.rs` (`Commit`, `Tag`, `parse_commit`), `crates/git-object/src/tree.rs` (`TreeEntry`, `parse_tree`/`serialize_tree`)
- Pattern: Pure parse/serialize functions over bytes; hashing via `git-hash::Oid`/`HashAlgorithm`.

**Revision language:**

- Purpose: Turn user strings into oids and oid sets.
- Examples: `crates/git-revision/src/resolve.rs` (`Resolver`, `ResolveError`), `crates/git-revision/src/lib.rs` (`RevWalk`), `crates/git-revision/src/rev_info.rs`
- Pattern: Loader-callback design (`CommitLoader`) so walkers stay storage-agnostic.

**RefStore / Index:**

- Purpose: Mutable git state besides objects.
- Examples: `crates/git-refs/src/lib.rs` (`RefStore`, symref depth limit 10), `crates/git-index/src/lib.rs` (`Index`, `IndexEntry`), `crates/git-index/src/cache_tree.rs`
- Pattern: C-format-faithful readers/writers (v2 index, loose + packed-refs); reftable explicitly deferred.

**ConfigSet / StringBuf:**

- Purpose: Faithful ports of C `config.c` layering and `strbuf`.
- Examples: `crates/git-config/src/lib.rs`, `crates/git-core/src/strbuf.rs` (see `STRINGBUF_IMPLEMENTATION.md` convention: prefer `StringBuf` where strbuf semantics are needed)
- Pattern: Layered config (repo file + `-c`/`GIT_CONFIG_COUNT` overlays via `set_cli`); growable buffer with C-compatible semantics.

## Entry Points

**`git` binary:**

- Location: `crates/git-cli/src/main.rs`
- Triggers: Every `git <command>` invocation (directly or via `scripts/shim-git` for ported commands).
- Responsibilities: Forward args to `git_cli::run`, exit with its return code. Build with `cargo build --workspace` from `crates/` → `crates/target/debug/git`.

**`git_cli::run`:**

- Location: `crates/git-cli/src/lib.rs:52`
- Triggers: Binary entry and unit tests.
- Responsibilities: Built-in flags, global-option parsing, `PipeAwareWriter` setup, dispatch, exit-code mapping (`129` usage, `1` not-found, `141` SIGPIPE).

**`git_command::dispatch` / `dispatch_with`:**

- Location: `crates/git-command/src/lib.rs:366`
- Triggers: `git_cli::run`, tests, shim harness.
- Responsibilities: Name→implementation routing; `None` for unknown commands. Add new ports here plus the module file.

**`cargo xtask <cmd>`:**

- Location: `crates/xtask/src/main.rs`
- Triggers: Developer/CI automation.
- Responsibilities: `test`, `differential`, `gen-fixtures`, `scoreboard`, `gates`/`depcheck`/`safety`/`placement`, `drills`, `isolation`. Run from `crates/` (the real workspace; root `Cargo.toml` is a stale `gitcore` leftover).

**Crosswise test suites:**

- Location: `crates/tests/*crosswise.rs` (e.g. `phase4_crosswise.rs` … `phaseA12_crosswise.rs`), golden fixtures in `crates/tests/fixtures/`
- Triggers: `cargo xtask differential` / `cargo test --workspace`.
- Responsibilities: Run Rust vs system C git on identical inputs, assert byte-identical stdout/stderr/exit; verify both read directions of on-disk artifacts with C `fsck`/`verify-pack`/etc.

## Architectural Constraints

- **Threading:** Single-threaded, single-invocation CLI. No threads, no async runtime, no background tasks. Parallelism exists only in the test harness (hence the `CWD_LOCK` for cwd/env-mutating tests).
- **Global state:** Forbidden in library code. The only module-level shared mutables are `TEMP_COUNTER: AtomicU32` for temp object files (`crates/git-odb/src/lib.rs`) and test-only `CWD_LOCK: Mutex<()>` (`crates/git-command/src/lib.rs:437`). Per-invocation context (`RepoContext`, `Repository`, `Odb`) is constructed fresh and passed explicitly.
- **Circular imports:** None permitted — `cargo xtask depcheck` fails the build on cycles or upward edges. The single exception is `git-command` as composition root, which may depend upward.
- **No FFI:** The Rust workspace never links C git and C git never links Rust. Interop is via byte-compatible on-disk formats and byte-identical CLI behavior only.
- **C is the spec:** Exit codes (`129` usage, `128` fatal/die, `1` not-found), stderr text (e.g. `ResolveError::render`), and output formats must match C exactly; when C source and `t/` disagree, `t/` wins. Intentional deviations are logged in `docs/plan/FOLLOWUPS.md`, never silent.
- **Stale root manifest:** The repo-root `Cargo.toml` (`gitcore` staticlib) is not the workspace — all cargo commands run from `crates/`.

## Anti-Patterns

### Discovering the repository inside a command instead of using the context

**What happens:** A command module calls `Repository::discover()` (process cwd + env) directly instead of `ctx.repository()`.
**Why it's wrong:** It silently ignores `-C`, `--git-dir`, `--work-tree`, `--bare`, and `-c` overrides parsed in `RepoContext::from_global_args`, breaking C global-flag parity and making the command untestable via `RepoContext::at(dir)`.
**Do this instead:** Take `ctx: &RepoContext` (as the `Command` trait requires) and call `ctx.repository()` — see `crates/git-command/src/lib.rs:271`.

### Writing to stdout/stderr directly instead of the injected writer

**What happens:** A command uses `println!`/`print!` for primary output.
**Why it's wrong:** Output becomes uncapturable in unit and crosswise tests and bypasses the `PipeAwareWriter` SIGPIPE→141 handling in `crates/git-cli/src/lib.rs:23`.
**Do this instead:** Write primary output to `out: &mut dyn Write` passed to `Command::run` — see the trait at `crates/git-command/src/lib.rs:333` and any command e.g. `crates/git-command/src/cat_file.rs`.

### Adding a dependency that points up the layer stack

**What happens:** A lower-layer crate (e.g. `git-odb`) gains a dependency on a higher-layer one (e.g. `git-revision`, `git-command`).
**Why it's wrong:** It creates a cycle or upward edge that `cargo xtask depcheck` rejects, and inverts the foundation→surface layering the whole workspace relies on.
**Do this instead:** Keep edges downward per `crates/xtask/src/depcheck.rs` layer ranks; put composition logic in `git-command`, the sole composition-root exception.

## Error Handling

**Strategy:** Typed errors per crate converted at the command boundary into `CommandError { message, code }` with C-exact exit codes and stderr text.

**Patterns:**

- `CommandError::usage(msg)` → `129`, `::fatal(msg)` → `128` (C `die`), `::error(msg)` → `1`, `::silent(code)` → bare code with no message (e.g. `git diff` differs). `git-cli` prints `message` to stderr and returns `code` (`crates/git-command/src/lib.rs:67`, `crates/git-cli/src/lib.rs:107`).
- `From<RepoError|OdbError|PackError|MidxError|GraphError> for CommandError` funnels storage errors to `fatal` (`crates/git-command/src/lib.rs:106`).
- Revision failures carry C-verbatim text via `ResolveError::render()` (ambiguous vs unknown argument) so crosswise stderr matches byte-for-byte (`crates/git-revision/src/resolve.rs`, `crates/git-command/src/lib.rs:349`).
- Closed-pipe writes are swallowed and converted to exit `141` at the end, emulating C dying of SIGPIPE (`PipeAwareWriter` in `crates/git-cli/src/lib.rs:23`).

## Cross-Cutting Concerns

**Logging:** No logging framework. Diagnostics go to stderr via `eprintln!` at the `git-cli` boundary (`crates/git-cli/src/lib.rs:108`); commands return messages in `CommandError` rather than logging mid-path.
**Validation:** Parsers are total and property-tested (proptest round-trips, no-panic on arbitrary input); on-disk readers validate magic/version/checksum and return `Corrupt`/`BadChecksum`/`Truncated` errors (`crates/git-odb/src/pack/mod.rs`, `crates/git-index/src/lib.rs`).
**Authentication:** Out of scope for the core-object-layer port (network/transport phases deferred); `GIT_DIR`/`GIT_*` env handling and `-c`/`GIT_CONFIG_COUNT` overlays are the only ambient inputs, parsed explicitly in `RepoContext` (`crates/git-command/src/lib.rs:178`).

---

*Architecture analysis: 2026-09-25*
