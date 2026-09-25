# Architecture Research: Git Reimplementation Systems

**Domain:** Pure-Rust reimplementation of git (git.rs port of C git v2.55.0)
**Researched:** 2026-09-25
**Confidence:** HIGH (workspace verified from `.planning/codebase/` mapping of commit a157715; ecosystem structure cross-checked against gitoxide and libgit2 via Context7)

## Standard Architecture

How production git reimplementations converge on the same shape — and how this workspace maps to it.

### System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│ SURFACE — thin CLI entry (process boundary, exit codes only)    │
│  ┌──────────┐                                                   │
│  │ git-cli  │  main.rs → git_cli::run(args) -> i32              │
│  └────┬─────┘  --version/help, -C/-c/--git-dir/--work-tree,      │
│       │        PipeAwareWriter (SIGPIPE → 141)                   │
├───────┴─────────────────────────────────────────────────────────┤
│ COMPOSITION ROOT — command dispatch (sole upward-dependency      │
│ exception; mirrors C builtin/ layout)                           │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ git-command: RepoContext → dispatch_with → Command::run  │   │
│  │ one module per builtin (~45 ported) + checkout_core,     │   │
│  │ ident, ignore_util shared engines                        │   │
│  └───┬──────────┬──────────────┬──────────────┬─────────────┘   │
│      │          │              │              │                  │
├──────┴──────────┴──────────────┴──────────────┴─────────────────┤
│ LANGUAGE / QUERY — turn user strings into oid sets              │
│  ┌──────────────┐  ┌──────────────┐                             │
│  │ git-revision │  │ git-pathspec │                             │
│  │ Resolver,    │  │ matcher      │                             │
│  │ RevWalk      │  │              │                             │
│  └──────┬───────┘  └──────┬───────┘                             │
│         │                 │                                      │
├─────────┴─────────────────┴─────────────────────────────────────┤
│ DOMAIN MODEL / OPERATIONS — pure algorithms over parsed objects │
│  ┌────────────┐ ┌───────────┐ ┌───────────┐ ┌──────────────┐    │
│  │ git-object │ │ git-diff  │ │ git-merge │ │ git-pretty   │    │
│  │ parse/ser  │ │ Myers,    │ │ bases,    │ │ formatting   │    │
│  │            │ │ tree cmp  │ │ merge3    │ │              │    │
│  └─────┬──────┘ └─────┬─────┘ └─────┬─────┘ └──────────────┘    │
│        │              │             │                            │
├────────┴──────────────┴─────────────┴───────────────────────────┤
│ STORE — persistent .git/ state (the crosswise contract lives    │
│ here: every byte must round-trip with C git both directions)    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │
│  │ git-odb      │  │ git-refs     │  │ git-index    │            │
│  │ Loose+Odb+   │  │ loose +      │  │ v2 r/w +     │            │
│  │ pack/idx/    │  │ packed-refs  │  │ cache_tree   │            │
│  │ midx/delta   │  │              │  │              │            │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘            │
│         │                 │                 │                    │
├─────────┴─────────────────┴─────────────────┴───────────────────┤
│ ACCESS / I-O HELPERS — worktree materialization + stubs         │
│  ┌──────────┐ ┌───────────┐ ┌──────────┐ ┌────────┐ ┌─────────┐  │
│  │worktree  │ │ transport │ │ protocol │ │ hooks  │ │creds    │  │
│  │checkout  │ │ (stubs)   │ │pkt-line  │ │ spawn  │ │helpers  │  │
│  │helpers   │ │           │ │(no I/O)  │ │        │ │         │  │
│  └──────────┘ └───────────┘ └──────────┘ └────────┘ └─────────┘  │
├─────────────────────────────────────────────────────────────────┤
│ FOUNDATION — leaf primitives, zero intra-workspace deps         │
│  ┌────────┐ ┌─────────┐ ┌─────────┐ ┌──────────┐ ┌───────────┐   │
│  │git-hash│ │git-core │ │git-conf │ │git-date  │ │git-compr- │   │
│  │Oid     │ │Repo,    │ │ig       │ │tz        │ │ess, varint│   │
│  │sha1dc  │ │StringBuf│ │ConfigSet│ │          │ │commitgraph│   │
│  │sha256  │ │         │ │         │ │          │ │attrs      │   │
│  └────────┘ └─────────┘ └─────────┘ └──────────┘ └───────────┘   │
├─────────────────────────────────────────────────────────────────┤
│ ON-DISK STATE + ORACLE (not shipped, never linked)              │
│  .git/ (objects, refs, index, config, HEAD)                     │
│  C tree at repo root: builtin/ t/ Documentation/ (read-only)    │
│  Automation: xtask (exempt from layering), scripts/shim-git     │
└─────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| `git-cli` (surface) | Process entry, global flags, exit-code mapping (129/128/1/141) | 3-line `main` + `run()`; never contains git logic |
| `git-command` (composition root) | `Command` trait, `CommandError`, `RepoContext`, name→impl dispatch; one module per builtin | Match table over `&dyn Command`; output to injected `&mut dyn Write`, never `println!` |
| `git-revision` (language) | Rev strings → oids (`resolve`), history sets (`RevWalk`, `rev_info`) | Loader-callback design so walkers stay storage-agnostic |
| `git-pathspec` (language) | Pathspec patterns → tree/index matcher | Pure matcher over entry paths |
| `git-object` (model) | `ObjectKind`, blob/tree/commit/tag parse + serialize, hash verification | Total functions over bytes; proptest round-trips |
| `git-diff` / `git-merge` (operations) | Tree compare, Myers line diff, unified render, userdiff drivers; merge bases + 3-way `merge3` | Pure algorithms; storage injected via closures |
| `git-pretty` (presentation) | Log/show formatting, date rendering | Pure string building over parsed commits |
| `git-odb` (store) | Unified object reads: loose + pack/idx/midx + delta + commit-graph | `Odb::for_repo(&repo)` then `read(oid)`; pack layer owns `EntryKind`, delta base chains, checksums |
| `git-refs` (store) | Loose refs + `packed-refs`, symref resolution (depth limit 10) | C-format-faithful reader/writer; reftable explicitly deferred |
| `git-index` (store) | v2 index read/write, `IndexEntry`, `CacheTree` | C-format-faithful reader/writer |
| `git-worktree` (access) | Checkout materialization: file creation, modes, symlinks, stat refresh | Filesystem I/O helpers, no revision logic |
| `git-transport` / `git-protocol` (access) | Fetch/push negotiation state machine + progress shape (stubs); packet-line encode/decode + v0/v1/v2 negotiation over byte buffers | State structs testable without a connection; **no socket/HTTP client exists** |
| `git-hooks` / `git-credentials` (access) | Hook spawning (`$GIT_DIR/hooks/<name>`), credential-helper request/response + redaction | `std::process::Command` only; no secrets stored |
| Foundation leaves | `Oid`/`HashAlgorithm` (sha1dc vendored, SHA-256), varint codec, date/tz, `ConfigSet` layering, zlib via `flate2`/`miniz_oxide`, commit-graph chunks, wildmatch/ignore, `Repository`/`RepoEnv`/`StringBuf` | Zero intra-workspace deps; `StringBuf` used wherever C `strbuf` semantics are needed |
| `xtask` (automation, exempt) | `differential`, `scoreboard`, `gen-fixtures`, gates (`depcheck`, `safety`, `placement`), `drills`, `isolation` | May depend on anything; nothing may depend on it |

### Ecosystem cross-check: this is the industry-standard shape

- **gitoxide** (verified via Context7, HIGH confidence): identical layering — low-level **plumbing crates** (`gix-object`, `gix-odb`, `gix-pack`, `gix-ref`, `gix-config`, `gix-diff`, `gix-merge`, `gix-status`) that "take references, expose mutable parts as arguments", topped by a **porcelain hub crate (`gix`)** that is "high-level, convenient" plus a `gitoxide-core` shared-CLI layer. This workspace's `git-*` leaves + `git-command` composition root is the same pattern under different names.
- **libgit2** (verified via Context7, HIGH confidence): same internal subsystems — `git_odb` (thread-safe object store with internal locking), `revwalk` (push/hide, sorting, pathspec filtering), `merge.c`/`graph.c` built *on top of* revwalk (merge bases, ahead/behind by flag-marking walks), `blame` as a consumer of walk+diff, diff internals as augmented file-content/patch objects. Confirms the dependency direction **walk → merge/blame/porcelain**, never the reverse — matching this workspace's rank order.
- **C git itself** (the oracle): `builtin/` commands are thin wrappers over `lib/` subsystems (`object-file.c`, `refs.c`, `read-cache.c`, `revision.c`, `diff-*`, `merge-*`), exactly mirrored by `git-command/*.rs` over the `git-*` library crates.

**Opinion:** keep the current layering. It is not an accident — it is what every mature reimplementation converges on. Do not merge crates to "simplify" and do not let porcelain logic leak downward.

## Recommended Project Structure

The structure already exists and matches conventions. For remaining work, new code goes here:

```
crates/
├── git-cli/src/              # DONE — do not extend (main.rs, lib.rs only)
├── git-command/src/          # ← nearly all remaining work lands here
│   ├── lib.rs                # dispatch table, Command trait, RepoContext, resolve_arg
│   ├── <new_builtin>.rs      # one module per ported builtin (snake_case of C name)
│   ├── checkout_core.rs      # PATTERN: shared engine used by checkout/switch/restore
│   ├── ident.rs              # PATTERN: command-shared helper (identity resolution)
│   └── ignore_util.rs        # PATTERN: command-shared helper (ignore plumbing)
├── git-odb/src/pack/         # pack-write deltification, bitmap, cruft (remaining store gaps)
├── git-refs/src/             # reftable backend (when scheduled)
├── git-transport/            # negotiation → real I/O (Phase 10+ only)
├── git-protocol/             # pkt-line framing already buffer-level; add I/O edge later
├── tests/                    # <phase>_crosswise.rs per new suite + fixtures/
└── xtask/src/                # register new suites in suites()
```

### Structure Rationale

- **`git-command/` per-builtin modules:** mirrors C `builtin/<cmd>.c` 1:1, so reviewers diff Rust against C file-by-file and `t/` scripts map to modules. Proven by ~45 existing ports.
- **Shared engines (`checkout_core.rs`) over duplication:** checkout/switch/restore share one engine with three thin wrappers — the pattern to copy for families like branch/checkout `-b`, stash (needs reflog+merge+checkout), cherry-pick/revert (needs merge machinery + sequencer).
- **New library crate only for new on-disk formats or protocols:** pack-write improvements stay inside `git-odb`; only genuinely new subsystems (reftable, real transport I/O) justify new crates, each assigned a `depcheck` rank with strictly downward deps.
- **Wiring checklist is mechanical and must stay so:** `pub mod` + dispatch arm + `scripts/shim-git` case + crosswise suite + `suites()` registration. Roadmap phases should treat this as a per-command fixed cost.

## Architectural Patterns

### Pattern 1: Thin surface + composition root (mandatory)

**What:** `main.rs` (3 lines) → `git_cli::run` (flags + exit codes) → `dispatch_with` (routing) → `Command::run(ctx, args, out)`. All git logic lives in library crates; commands only compose them.
**When to use:** Every new builtin, no exceptions.
**Trade-offs:** Slight boilerplate per command (struct + trait impl + match arm) buys unit-testability (`dispatch` with in-memory writer, `RepoContext::at(dir)`) and byte-identical crosswise testing. There is no cheaper structure that preserves the done-gates.

**Example:**
```rust
// Every ported builtin follows this shape (see cat_file.rs, status.rs)
pub struct Blame;
impl Command for Blame {
    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write)
        -> Result<(), CommandError>
    {
        let repo = ctx.repository()?;   // never Repository::discover() directly
        // ... compose git-odb / git-revision / git-diff, write to `out`
        Ok(())
    }
}
```

### Pattern 2: Storage-agnostic query layer via loader callbacks

**What:** `RevWalk` and diff/merge operations take caller-supplied loader closures over `Odb` instead of owning the store (see `log.rs`/`rev_list.rs` building `RevWalk` with a commit-loader closure).
**When to use:** Any history walk, merge-base computation, blame walk, or tree comparison in new commands.
**Trade-offs:** One extra closure parameter per call site; in return the algorithm crates stay at rank 1 (no dependency on `git-odb`), `depcheck` stays green, and algorithms are unit-testable against synthetic loaders without touching disk.

### Pattern 3: Shared porcelain engines for command families

**What:** Non-user-facing engine module in `git-command/` (e.g. `checkout_core.rs`) implementing the operation once; thin `Command` wrappers for each builtin spelling (`checkout`, `switch`, `restore`).
**When to use:** Next families — sequencer-backed commands (cherry-pick/revert share commit-replay), ref-mutating families (branch/tag share create/verify/delete flows), stash (needs its own engine over reflog + merge + checkout).
**Trade-offs:** Engine API design costs one thinking step up front; pays back by making the 2nd/3rd command in a family nearly free and keeping C-parity fixes in one place.

### Pattern 4: Buffer-level protocol without I/O (for transport work)

**What:** `git-protocol` implements packet-line encode/decode and v0/v1/v2 negotiation over plain byte buffers ("canned streams in tests … verified without any connection"); `git-transport` owns negotiation state + progress shape without opening connections.
**When to use:** When Phase 10+ network work starts — add the socket/HTTP edge *outside* these crates' core logic, keeping the framing/state machine testable offline.
**Trade-offs:** Defers the hardest part (real I/O, auth, smart-HTTP) while keeping everything built so far. Do not add `reqwest`/`hyper`/`tokio` without the plan amendment the constraints require.

## Data Flow

### Request Flow (read path — e.g. `git log -- <path>`)

```
CLI argv
  ↓  (git_cli::run: global flags → RepoContext; subcommand split)
dispatch_with("log") → Log::run(ctx, args, out)
  ↓  ctx.repository() → RepoEnv + discover_from (.git walk-up, gitdir: files,
  ↓  commondir, config load, -c overlays)
resolve_arg(repo, rev) → Resolver::new(repo) over Odb + RefStore
  ↓  full oid → abbrev scan → ref name → ~n/^n peels
RevWalk::new(loader_closure_over_Odb) → commit oid stream
  ↓  per commit: Odb::read → git-object parse → tree oids
compare_trees / diff_lines (git-diff) → render_unified → git-pretty format
  ↓  write to out: &mut dyn Write (PipeAwareWriter handles SIGPIPE→141)
exit code: Ok→0, CommandError{code,message}→code w/ message on stderr
```

### Mutation Flow (write path — e.g. `git stash`, `git commit`, future `branch`)

```
Command::run
  ↓  ctx.repository() (same discovery as reads)
read current state: RefStore (HEAD/symref) + Index + Odb
  ↓  revision/merge/diff computation (pure, loader-callback style)
write new objects: LooseStore / pack writer (fsync + atomic rename;
  ↓  TEMP_COUNTER for temp names)
update refs: RefStore write + reflog append (loose + packed-refs)
  ↓  update index: v2 write + cache_tree refresh
run hooks (pre-commit/post-commit/...) via git-hooks spawn — AFTER core
  ↓  write, failures mapped to C-exact codes/messages
primary output → out; diagnostics → stderr via CommandError
```

**Direction rule:** data always flows **downward** surface → language → model/operations → store → foundation, and **back up** as parsed values. The only upward edge in the workspace is composition (commands *calling* downward APIs), never a lower crate importing a higher one — enforced by `cargo xtask depcheck`.

### State Management

```
Per-invocation (stateless process, no server, no threads):
  RepoContext → Repository → Odb / RefStore / Index handles
  (constructed fresh, threaded explicitly as &params)

On-disk (the only durable state):
  .git/objects (loose + pack/idx/midx) ← git-odb
  .git/refs + packed-refs               ← git-refs
  .git/index (+ cache_tree)             ← git-index
  config, HEAD, hooks/                  ← git-core / git-config / git-hooks

Forbidden: process-global mutable state in library code.
Only exceptions: TEMP_COUNTER (AtomicU32, temp object names),
CWD_LOCK (test-only mutex for cwd/env-mutating tests).
```

### Key Data Flows for remaining subsystems

1. **Porcelain worktree flows** (stash, future merge/rebase/cherry-pick porcelain): `refs + index + worktree files + Odb` all participate; flow is read-current → compute (merge3/diff) → write objects → update refs+index → materialize worktree → hooks. These are the most integration-heavy commands because they touch every store crate.
2. **Blame / history annotation:** `RevWalk` (topo/date order) × per-commit tree/blob reads × Myers diff per step, newest-to-oldest line-origin attribution. Pure consumer of existing layers — no new store code needed.
3. **Remaining plumbing** (reflog, gc/prune/repack, notes, archive): each touches one store crate plus dispatch; independent of each other. Ideal parallel fill-in work.
4. **Transport (Phase 10+):** `protocol` framing + `transport` negotiation + new I/O edge → pack download → `index-pack` path → ref update. Blocked on nothing already built (stubs are buffer-level); blocked only by scope decision.

## Scaling Considerations

This is a single-invocation CLI, not a service — "scaling" means repository size and object counts, not users.

| Scale | Architecture Adjustments |
|-------|--------------------------|
| Small repos (<10k objects) | Current architecture handles trivially; loose reads dominate, no tuning needed |
| Medium repos (10k–1M objects, packs) | Pack idx/midx + commit-graph (already present) carry the load; keep delta resolution streaming, avoid full-pack materialization in new commands |
| Large repos (1M+ objects, monorepos) | Bitmaps, multi-pack-index, commit-graph bloom (partially present), partial-clone/cruft (gaps) matter; new porcelain must reuse `Odb` unified reader rather than re-scanning packs |

### Scaling Priorities

1. **First bottleneck:** per-command pack re-scanning — new commands must go through `Odb`/`PackFile` shared readers, never ad-hoc pack directory walks. (Already the pattern; enforce in review.)
2. **Second bottleneck:** non-deltified pack writes (`pack-objects` known deviation in FOLLOWUPS.md) — output size and C-side `verify-pack` acceptance, not speed, is the risk; deltified writes are the scheduled fix, not new-command work.

## Anti-Patterns

### Anti-Pattern 1: Discovering the repository inside a command

**What people do:** Call `Repository::discover()` (process cwd + env) directly instead of `ctx.repository()`.
**Why it's wrong:** Silently ignores `-C`, `--git-dir`, `--work-tree`, `--bare`, `-c` overrides; breaks C global-flag parity and `RepoContext::at(dir)` testability.
**Do this instead:** Take `ctx: &RepoContext`, call `ctx.repository()` (`git-command/src/lib.rs:271`).

### Anti-Pattern 2: Writing to stdout directly

**What people do:** `println!`/`print!` for primary output.
**Why it's wrong:** Uncapturable in unit/crosswise tests; bypasses `PipeAwareWriter` SIGPIPE→141 handling.
**Do this instead:** Write to `out: &mut dyn Write` from `Command::run`.

### Anti-Pattern 3: Upward or lateral crate dependencies

**What people do:** Lower crate imports a higher one (e.g. `git-odb` → `git-revision`), or porcelain logic (output formatting, arg parsing) sinks into a store/model crate.
**Why it's wrong:** `depcheck` fails the build; inverts the layering every reimplementation converges on; makes store crates untestable in isolation.
**Do this instead:** Composition lives only in `git-command`; pass data down as values, callbacks, and plain structs.

### Anti-Pattern 4: New command reimplementing an existing engine

**What people do:** Copy-paste checkout/status/merge logic into a new builtin instead of extracting a shared engine.
**Why it's wrong:** C-parity fixes then land in N places; `t/` suite exposes the drift immediately.
**Do this instead:** Follow `checkout_core.rs`: engine module + thin wrappers from the start for command families.

### Anti-Pattern 5: Adding network/async/CLI deps casually

**What people do:** Pull in `tokio`, `reqwest`, `clap`, `serde` to move faster on transport or arg parsing.
**Why it's wrong:** Violates locked constraints (no async runtime, `flate2`-only compression, no `clap`/`serde`/network without plan amendment); MSRV 1.74 and `cargo xtask safety` gates assume the current set.
**Do this instead:** Hand-rolled arg parsing per command (C-parity demands it anyway); buffer-level protocol work needs no I/O deps.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| System C git (`/usr/bin/git`, `$SYSTEM_GIT`) | Byte-identical oracle: crosswise suites shell out to both binaries, assert identical stdout/stderr/exit; C verifies Rust artifacts (`fsck`, `verify-pack`, `commit-graph verify`) | Bidirectional contract; `t/` wins on C-source-vs-test disagreement |
| C build tree (repo root) | Read-only reference; never linked, never called | Rebuild with `make` when oracle binaries go stale |
| Hook/credential/editor child processes | `std::process::Command` spawn with `GIT_DIR` in env | Local only; no network results from any spawn |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| `git-command` ↔ all layers | Direct function calls, `&RepoContext`, loader closures, `&mut dyn Write` | Sole composition root; only crate allowed upward deps |
| Commands ↔ store (`odb`/`refs`/`index`) | Construct handles from `&Repository`, call read/write methods, map errors via `From<…> for CommandError` | Storage errors funnel to exit-128 `fatal` |
| Language ↔ store | `Resolver::new(repo)` borrows `Odb`+`RefStore`; `RevWalk` takes loader closure | Keeps rank-1 crates store-free |
| Operations ↔ model | Pure values (`Commit`, `TreeEntry`, oid lists); no handles cross | Enables proptest round-trips without disk |
| `xtask`/shim ↔ everything | Out-of-band: shell out to built binaries, never linked | `shim-git` case list + `suites()` registration per new command |

## Suggested Build Order (dependencies between components)

For the remaining subsystems (porcelain worktree flows + leftover plumbing), dependencies dictate this order:

1. **Single-store plumbing first** (no cross-crate deps; parallelizable): reflog read/append (`git-refs` extension), `gc`/`prune`/`repack` orchestration over existing `git-odb` readers, notes, `archive`. Each is one store crate + one dispatch module. Lowest risk, fills `t/` coverage fastest.
2. **Read-only multi-store consumers next** (compose existing layers, no new store code): `blame` (RevWalk × tree/blob reads × Myers), `grep` (revision + pathspec + worktree/index blob sources), `for-each-ref` extensions. Proves the query layer under porcelain load.
3. **Ref-mutating families** (need shared engines + reflog from step 1): `branch`/`tag` create/verify/delete flows as one engine; `stash` engine over reflog + merge + checkout_core. Do reflog before stash — stash is blocked on it.
4. **Worktree-mutating porcelain** (touch every store crate; heaviest integration): porcelain `merge`, `cherry-pick`/`revert` (sequencer + merge3 + checkout engine), `rebase`/`am` (sequencer on top). Build the sequencer engine once, then the three wrappers — repeat of the `checkout_core` pattern.
5. **Store-format gaps** (orthogonal track, any time after step 1): deltified pack writes, bitmaps, cruft packs, reftable backend. These change what the store *writes*, so land them before or with the commands that will exercise them crosswise — never after a porcelain phase "completes" against the old format.
6. **Transport/network last** (explicitly Phase 10+, out of current scope): real I/O edge on top of buffer-level `git-protocol` + `git-transport` negotiation; `clone`/`fetch`/`push`/`pull` compose everything above. Blocked on scope decision, not on architecture — the seams already exist.

**Phase-structure implication for the roadmap:** order phases by *integration surface*, not by C builtin number — single-store plumbing (many small wins) → read-only consumers → ref-mutating families → full worktree porcelain → transport. Each phase's done-gates (differential + crosswise + scoreboard) are cheapest on step 1–2 commands and most valuable on step 4, where all store crates interact.

## Sources

- `.planning/codebase/ARCHITECTURE.md` — workspace layering, data flows, constraints (HIGH — generated from repo at commit a157715, verified against `git-command/src/lib.rs` dispatch table listing ~45 ported builtins)
- `.planning/codebase/STRUCTURE.md` + `INTEGRATIONS.md` — crate layout, wiring checklist, oracle/shim/CI integration (HIGH — same provenance)
- `docs/plan/README.md` — phase map Phases 0–9, dependency order, Phase 10+ out-of-scope note (HIGH — repo-local canonical plan)
- gitoxide `AGENTS.md` / `crate-status.md` via Context7 (`/gitoxidelabs/gitoxide`) — plumbing-vs-porcelain split, crate organization, `gix` hub role (MEDIUM — upstream docs, cross-checked against workspace shape)
- libgit2 `merge.c` / `graph.c` / `blame.h` / `revwalk.h` / `threading.md` / `diff-internals.md` via Context7 (`/libgit2/libgit2`) — odb/revwalk/merge layering, walk-based merge-base/ahead-behind (MEDIUM — upstream source docs, consistent with both C git and this workspace)

---
*Architecture research for: pure-Rust git reimplementation (git.rs)*
*Researched: 2026-09-25*
