# Phase 1: Refs & Config Foundation - Research

**Researched:** 2026-09-28
**Domain:** Git refs storage (files backend: loose locks, transactions, packed-refs, reflog) + git config subsystem (scopes, includes, CLI matrix)
**Confidence:** HIGH

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Phase Boundary:** Users get a reflog safety net, crash-safe ref updates, and working git config management. In scope: REFS-01 (reflog read + every mutating command logs), REFS-02 (lock files + atomic rename + multi-ref transactions, packed-refs write, `update-ref --stdin`), CONF-01 (`git config` with system/global/local/worktree scopes, includes, `--list/--get/--unset`). Out of scope: everything in Phases 2–12 (rev-parse completion, merge, sequencer, store, transport, fsck depth, network, mail, submodules, interactive, filters, long tail).

**Reflog surface**
- **D-01:** Full reflog surface in Phase 1, including `expire` (`--expire=now/--all`) and `delete` — not show+log only.
- **D-02:** Every ref-mutating command logs from day one, including branch/tag/update-ref/clone paths — not just checkout/commit/reset.
- **D-03:** C-exact reflog gating: HEAD always logged; branch ref logged only when `logallrefupdates` allows (existing `commit.rs` split is the pattern to extend).
- **D-04:** Honor C `gc.reflogExpire` / `gc.reflogExpireUnreachable` defaults (90/30-day family) via config; `--expire=now/--all` supported for tests.

**Transaction safety**
- **D-05:** All-or-nothing multi-ref atomicity with C-exact lock/transaction error text — no best-effort partial apply.
- **D-06:** Full `update-ref --stdin` batch syntax (create/update/delete/verify lines) byte-exact in Phase 1 — not single-ref only.
- **D-07:** Packed-refs write included in Phase 1 alongside loose-ref locking (no torn packed-refs; success criterion #2).
- **D-08:** Lock-contention failures render byte-exact C stderr with correct exit codes; concurrent updates never tear state.

**Config command scope**
- **D-09:** Full C option matrix in Phase 1: get/set/unset/list plus `--add/--replace-all/--remove-section/--show-origin/--type` — not basics only.
- **D-10:** All four scopes honored with C precedence (system → global → local → worktree); CLI `-c` / `GIT_CONFIG_COUNT` overlays beat files.
- **D-11:** C-exact multivar handling (`--null/--fixed-value/--get-urlmatch` behaviors) for script use.
- **D-12:** Invalid files/sections fail with C-exact fatal text (exit 128), byte-compared in crosswise.

**Includes & scopes**
- **D-13:** Full conditional `includeIf` (`gitdir`/`onbranch`/`hasconfig`) plus `include.path` honored like C — not plain includes only.
- **D-14:** C-exact include path resolution: relative paths resolve from the including file; `~`/`~/` expansion like C.
- **D-15:** Locked precedence: CLI `-c` and `GIT_CONFIG_COUNT/K/V` beat all files; files apply system → global → local → worktree.
- **D-16:** C-exact include cycle guard with depth cap (no infinite loops); cycles fail C-exactly.

### the agent's Discretion

None — user made concrete selections on all 16 questions.

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REFS-01 | User gets reflog safety net — `git reflog` reads `logs/<ref>` and every mutating command logs (gate: `t/t1410`) | Reflog line format, gating rules, `builtin/reflog.c` subcommand matrix, shared append helper |
| REFS-02 | Concurrent ref updates stay safe via lock files + atomic rename + multi-ref transactions, including packed-refs write and `update-ref --stdin` (gate: `t/t1400`, packed-refs gate) | C lock/transaction protocol, `--stdin` batch grammar, packed-refs file format, contention error strings |
| CONF-01 | User manages setup via `git config` command with system/global/local/worktree scopes, includes, and `--list/--get/--unset` (gate: `t/t1300`) | C scope sequence, includeIf conditions, full action/option matrix, existing `ConfigSet` reuse |

## Project Constraints (from AGENTS.md)

- **Oracle, not dependency:** C tree (`refs.c`, `refs/files-backend.c`, `builtin/`, `config.c`, `t/`) is read-only reference; **no FFI** — never link C [VERIFIED: AGENTS.md:7-8].
- **Run cargo from `crates/`** — root `Cargo.toml` is a stale leftover (`gitcore` staticlib); real workspace is `crates/Cargo.toml` [VERIFIED: AGENTS.md:14].
- **Never hand-edit `crates/scoreboard.json`** — regenerate via `cargo xtask scoreboard`; it fails on regression [VERIFIED: AGENTS.md:15].
- **New command checklist:** add to `scripts/shim-git` `case` list + wire in `git-command::dispatch` + add crosswise suite [VERIFIED: AGENTS.md:35].
- **Strict downward layering** enforced by `cargo xtask depcheck`; `git-command` is the sole composition-root exception [VERIFIED: crates/xtask/src/depcheck.rs:1-14].
- **No new deps** (no `clap`/`serde`/`tokio`, no new crypto) without plan amendment [VERIFIED: .planning/PROJECT.md:55].
- **C-exact behavior:** exit codes 129 usage / 128 fatal / 1 error / 141 SIGPIPE; **`t/` wins ties** over C source [VERIFIED: AGENTS.md:69].
- **Known deviations in `docs/plan/FOLLOWUPS.md`** (non-collision-detecting SHA-1, non-deltified packs, UTC-only dates) — do not silently "fix" [VERIFIED: AGENTS.md:70].
- **Rust edition 2021, MSRV 1.74; no async runtime; `unsafe` near-zero** gated by `cargo xtask safety` [VERIFIED: .planning/PROJECT.md:53].
- **Do not run `watch-and-commit.sh`** [VERIFIED: AGENTS.md:17].

## Summary

Phase 1 ports three tightly-coupled C subsystems into the existing Rust workspace: (a) the **files-backend ref transaction engine** (`refs/files-backend.c`, ~4100 lines) — lock files, two-phase prepare/commit, packed-refs rewrite, reflog append gating; (b) the **`reflog` + `update-ref --stdin` CLI surface** (`builtin/reflog.c` 492 lines, `builtin/update-ref.c` 905 lines); (c) the **`config` command + scope/include resolution** (`builtin/config.c` 1658 lines on top of `config.c` 3666 lines). The good news: the workspace already owns the hardest parsing pieces — `ConfigSet` (sections, quoting, continuations, `include.path`, cycle guard), `RefStore` (loose+packed read, single-ref atomic update), and two reflog append helpers with the HEAD-vs-branch gating pattern. The phase is therefore **extension, not greenfield**: centralize one reflog writer, grow `RefStore` into lock/transaction/packed-refs-write, and build two thin `Command` modules on top.

The two highest-risk items are byte-exactness surface area (`update-ref --stdin` has ~10 line verbs with C-quoted args; `git config` has ~16 actions × file/display/type options with mutual-exclusion errors) and the **`t/t3210-pack-refs.sh` gate named in CONTEXT.md does not exist** in this tree — the real packed-refs oracles are `t/t0601-reffiles-pack-refs.sh` + `t/pack-refs-tests.sh` (see Open Questions). A second flag: `reflog.h` in this tree initializes `default_expire_total = now - 30d` / `default_expire_unreachable = now - 90d`, which reads swapped versus the documented 90/30-day family — the built C binary and `t/t1410` decide (t/ wins ties).

**Primary recommendation:** Grow `git-refs` into a C-faithful transaction engine (lock → prepare → commit/abort, packed-refs atomic rewrite) with one shared reflog writer, then add thin `reflog` and `config` command modules in `git-command` reusing `ConfigSet`; gate every step on `t/t1410`, `t/t1400` (+`t/t1404`), `t/t0601`, `t/t1300` (+`t/t1305`, `t/t1308`) through the shim with no scoreboard regression.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Ref lock/transaction/packed-refs write + reflog append gating | Storage library (`git-refs`) | — | On-disk format owner; must stay callable without CLI; depcheck forbids upward edges |
| Config file parse/layer/scope/include resolution | Mid library (`git-config`) | — | Pure function over files+env; `Repository` already embeds `ConfigSet` |
| `reflog` / `config` / `update-ref --stdin` CLI parsing + output rendering | Surface (`git-command` modules) | — | One-module-per-builtin pattern; only `git-command` may compose libs |
| `-c` / `GIT_CONFIG_COUNT` overlays + `-C/--git-dir` resolution | Surface (`RepoContext` in `git-command`) | — | Already implemented in `repository()`; new commands inherit automatically |
| `t/` oracle execution + scoreboard guard | Automation (`xtask`, `scripts/shim-git`) | — | Existing harness; new commands registered, not reinvented |

## Standard Stack

### Core (all workspace-internal — no new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `git-refs` | workspace 0.1.0 | Ref storage: extend `RefStore` with reflog read, C-exact locking, transaction, packed-refs write | Owns the on-disk ref format; read path already exists and is tested [VERIFIED: crates/git-refs/src/lib.rs:1-6] |
| `git-config` | workspace 0.1.0 | Config parse/layer/get/set; extend with scopes, includeIf, multivar ops, file rewrite | Parser + `set_cli` + cycle guard already exist and are proptested [VERIFIED: crates/git-config/src/lib.rs:1-6] |
| `git-command` | workspace 0.1.0 | New `reflog` + `config` modules; extend `update_ref.rs`; wire `dispatch` | One-module-per-builtin + `Command` trait pattern; `dispatch_with` takes caller context [VERIFIED: crates/git-command/src/lib.rs:333-343] |
| `git-core` | workspace 0.1.0 | `Repository` (discovery, `config`, `resolve_head`), `RepoEnv` | Every command receives `RepoContext` and calls `ctx.repository()` [VERIFIED: crates/git-command/src/lib.rs:271-299] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `git-hash` | workspace 0.1.0 | `Oid::from_hex`, algo hex widths, null oid | Ref target parse/format; reflog old/new rendering |
| `git-date` | workspace 0.1.0 | Date parsing for `--expire=<time>` / `--expire-unreachable=<time>` and reflog timestamps | `reflog expire` expiry parsing; ident timestamp formatting |
| `git-revision` | workspace 0.1.0 | `@{n}` / `@{date}` specifier resolution for `reflog delete <ref>@{...}` and `show` | Reflog-entry selector parsing |
| `git-index` | workspace 0.1.0 | Unchanged | Only if `config --blob` or worktree paths need it — otherwise untouched |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Extend `ConfigSet` | New config parser crate | Rejected: existing parser already handles quoting/continuations/includes with proptests; a rewrite risks byte-divergence |
| Hand-ported lock/transaction in `git-refs` | External lockfile crate | Rejected: no-new-deps rule; C `lockfile.c` semantics (`.lock` suffix, stale detection, `commit_lock_file` rename) are git-specific anyway |
| Thin `Command` modules | `clap`-derived CLI | Rejected: locked no-`clap` decision; existing manual parsers already match C usage-text style |

**Installation:** None — all crates are workspace members. No `cargo add` steps.

**Version verification:** No external packages; toolchain verified: `cargo 1.97.1`, `rustc 1.97.1` (MSRV 1.74 satisfied) [CITED: toolchain probe this session].

## Package Legitimacy Audit

No external packages are installed or recommended in this phase. All work extends workspace-internal crates (`git-refs`, `git-config`, `git-command`) using `std` only, per the no-new-deps constraint.

| Package | Registry | Verdict | Disposition |
|---------|----------|---------|-------------|
| *(none)* | — | — | No installs; audit not applicable |

**Packages removed due to SLOP verdict:** none.
**Packages flagged as suspicious (SUS):** none.

## Architecture Patterns

### System Architecture Diagram

```
                    ┌───────────── git-command (surface) ─────────────┐
                    │  reflog.rs      config.rs       update_ref.rs   │
                    │  (parse argv,  (16 actions,     (single +       │
                    │   render to     file/display/   --stdin batch   │
                    │   out: &mut     type opts →      parser →       │
                    │   dyn Write)    ConfigSet ops)  transaction)    │
                    └──────┬──────────────┬───────────────┬───────────┘
                           │              │               │  prepare: validate all,
                           │              │               │  lock every ref (.lock)
                           ▼              ▼               ▼  commit: rename all or abort
 .git/            ┌─────────────────┐ ┌──────────┐ ┌──────────────────┐
 refs/heads/* ───►│ RefStore        │ │ConfigSet │ │  transaction     │
 packed-refs ────►│ (lock→write→    │ │system→   │ │  (all-or-nothing,│
 logs/<ref> ◄────►│  rename; packed │ │global→   │ │  D/F + old-oid   │
 (append-only)    │  rewrite atomic)│ │local→    │ │  + symref checks)│
                  │  + reflog gate  │ │worktree→│ └──────────────────┘
                  └─────────────────┘ │cmdline  │
 config(.worktree)◄── include.path ──►└──────────┘── includeIf(gitdir/onbranch/
        ▲            + cycle guard(M=10)              hasconfig/worktree) ──► files
        │                                                            │
 RepoContext::repository() ── -C/--git-dir/-c/GIT_CONFIG_COUNT ──────┘
```

Data-flow notes: `--stdin` lines parse → per-line transaction ops queued → **all** locks acquired and validations run (old-oid, D/F conflicts, symref rules) → commit renames every lock or aborts all [CITED: builtin/update-ref.c usage lines 18-23; t/t1404-update-ref-errors.sh transaction tests]. Reflog append is a post-commit side effect gated per-ref (HEAD always; others only when `logallrefupdates` allows) [CITED: refs/files-backend.c `log_ref_setup`/`should_autocreate_reflog`]. Config reads flow files→entries once; writes rewrite the selected scope file preserving surrounding content.

### Recommended Project Structure

```
crates/git-refs/src/
├── lib.rs            # RefStore: + lock(), transaction(), reflog read/append/expire/delete
├── lock.rs           # (new) C lockfile.c port: <ref>.lock create, stale detect, commit(rename)/rollback
├── reflog.rs         # (new) log format/parse, gating (LOG_REFS_*), expire policy, delete
└── packed.rs         # (new) packed-refs read (extend) + atomic sorted write with ^peeled lines
crates/git-config/src/
├── lib.rs            # ConfigSet: + scopes loader, includeIf, multivar ops, typed get
└── file.rs           # (new) scope-file read-modify-write preserving comments/layout
crates/git-command/src/
├── reflog.rs         # (new) show/list/exists/write/delete/drop/expire subcommands
├── config_cmd.rs     # (new) get/set/unset/list + subcommand spellings + display opts
└── update_ref.rs     # (extend) --stdin batch grammar (-z, no-deref, start/prepare/commit/abort)
```

(Exact file split is the planner's call; the constraint is: storage logic in `git-refs`/`git-config`, composition only in `git-command` — depcheck layer map: `git-config`=mid(1), `git-refs`=store(2), `git-command`=surface(5), edges downward only [VERIFIED: crates/xtask/src/depcheck.rs:27-46].)

### Pattern 1: Thin CLI + `Command` trait, output to injected writer

**What:** Each builtin is a unit struct implementing `Command::run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write)`, returning `Option<Result<(), CommandError>>` via `dispatch_with`; never `println!`, never process-exit inside logic [VERIFIED: crates/git-command/src/lib.rs:333-343,371-428].
**When to use:** Both new commands (`reflog`, `config`) and the `update-ref --stdin` extension.
**Example:**
```rust
// Source: crates/git-command/src/lib.rs:371-428 (dispatch_with match arms)
"update-ref" => &update_ref::UpdateRef,
"symbolic-ref" => &update_ref::SymbolicRef,
// planner adds: "reflog" => &reflog::Reflog, "config" => &config_cmd::Config,
```

### Pattern 2: Centralized reflog writer with HEAD-vs-branch gating

**What:** One helper owns the line format `<old> <new> <name> <email> <ts> <tz>\t<msg>\n` (committer split at `>`), parent-dir creation, and append-open; callers only decide *whether* to log per the `core.logallrefupdates` tri-state (`always` / bool / unset→non-bare default) and the ref-prefix rule (`refs/heads/`, `refs/remotes/`, `refs/notes/`, `HEAD`) [CITED: refs/files-backend.c `log_ref_write_fd`, `log_ref_setup`, `should_autocreate_reflog`; `refs.c:1059` bool parse].
**When to use:** Every mutating command (D-02) — replace the three existing ad-hoc writers (`checkout_core.rs:259`, `commit.rs:659`) with the single helper.
**Example:**
```rust
// Existing pattern to extend — crates/git-command/src/commit.rs:287-303:
// "HEAD is always logged; the branch ref's reflog is only written
//  when its value actually changes (C skips a no-op ref update)."
```

### Pattern 3: Config layering — files in scope order, overlays last

**What:** Load order system → XDG → user(global) → local(`$COMMONDIR/config`) → worktree(`$GIT_DIR/config.worktree`, only when `extensions.worktreeConfig` set) → `GIT_CONFIG_COUNT/K/V` → `-c` [CITED: config.c `do_git_config_sequence`]. Last occurrence wins on lookup; `ConfigSet::append` already implements this [VERIFIED: crates/git-config/src/lib.rs:237-239].
**When to use:** Repository config load (extend `Repository::discover_from`, which today reads only `$COMMONDIR/config` [VERIFIED: crates/git-core/src/lib.rs:144-150]) and every `git config [--scope]` read/write path.

### Anti-Patterns to Avoid

- **`.lock.<pid>` temp names:** current `RefStore::update` and `write_file_atomic` use `path.with_extension(format!("lock.{}", pid))` [VERIFIED: crates/git-refs/src/lib.rs:110; crates/git-command/src/checkout_core.rs:225]. C lock files are `<ref>.lock`; contention/stale-lock tests (`t/t1404`) observe lock failure text, and a foreign suffix breaks C interop (C won't see our lock). Port `lockfile.c` naming exactly.
- **Best-effort partial apply in `--stdin`:** `t/t1404`'s `test_update_rejected` asserts the ref set is byte-unchanged after a failed batch — validate everything before renaming anything.
- **Re-implementing config value parsing per flag:** `--type=bool/int/path/expiry-date/bool-or-int` share one canonicalizer (`builtin/config.c:option_parse_type` + `TYPE_*`); don't branch per subcommand.
- **Bypassing `RepoContext`:** new commands must take `ctx` and call `ctx.repository()` so `-C/--git-dir/-c/GIT_CONFIG_COUNT` keep working [VERIFIED: crates/git-command/src/lib.rs:271-299] — the `GIT_CONFIG_COUNT` missing-key fatal already exists [VERIFIED: crates/git-command/src/lib.rs:312-331].

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Config file parsing (quotes, continuations, comments, subsections) | New parser | Extend `ConfigSet::parse/parse_into` | Already handles sections, `key value`/`key=value`, inline comments, `\`-continuations, C escapes with proptests [VERIFIED: crates/git-config/src/lib.rs:120-199,568-589] |
| `--expire=<time>` timestamps | New date parser | `git-date::parse` | C uses `approxidate`/`parse_expiry_date`; the crate exists for exactly this |
| Refname validation | New regex | Extend `validate_refname` | Subset exists (dots, `@{`, `.lock`, voids, controls) [VERIFIED: crates/git-refs/src/lib.rs:183-207]; C adds one-level, `@`, `*`, reflog-suffix rules — port, don't invent |
| `--stdin` C-quoted args | New unquoter | Port of `parse_arg`/`unquote_c_style` semantics | `-z` vs whitespace+C-quote dual mode is subtle; byte-parity requires the same state machine [CITED: builtin/update-ref.c `parse_arg`] |
| Reflog selector `@{n}`/`@{date}` | New resolver | `git-revision` resolver | Shared ambiguity/shortening logic already used by `short_oid` [VERIFIED: crates/git-command/src/checkout_core.rs:288-294] |
| Atomic file replace | `write` + `rename` inline copies | One `write_file_atomic`-style helper with `<path>.lock` naming | Five current call sites each invent temp names; C crash-safety (`fsync` + rename + rollback) must live in one place |

**Key insight:** Every behavior in this phase already has a C implementation whose edge cases are pinned by `t/` tests — the winning move is faithful porting behind the existing crate seams, never novel design.

## Common Pitfalls

### Pitfall 1: Lock filename and stale-lock semantics diverge from C
**What goes wrong:** Concurrent `update-ref` corrupts or deadlocks; `t/t1404` contention cases fail on stderr text.
**Why it happens:** Current Rust code writes temp files as `<ref>.lock.<pid>` [VERIFIED: crates/git-refs/src/lib.rs:110] while C uses `<ref>.lock` and dies `unable to create lock file %s.lock` on contention [CITED: refs/files-backend.c:831]. C's lock also fsyncs before rename (`write_ref_to_lockfile`) and rolls back on error.
**How to avoid:** Port `lockfile.c` naming/lifecycle (`create → write+fsync → commit_lock_file(rename) / rollback`) into `git-refs`; make all writers (refs, HEAD, ORIG_HEAD, packed-refs, config) use it.
**Warning signs:** Any `with_extension("lock...")` outside the single lock helper; tests that pass solo but flake under `t/t1404`-style concurrency.

### Pitfall 2: Reflog gating over- or under-logs
**What goes wrong:** `t/t1410` fails: branch reflogs appear when `core.logallrefupdates=false`, or HEAD reflog missing in bare repos.
**Why it happens:** C rule is tri-state + prefix-based: `LOG_REFS_ALWAYS` logs everything; `NORMAL` (default non-bare; bare default is NONE) logs only `refs/heads/*`, `refs/remotes/*`, `refs/notes/*`, `HEAD`; `HEAD` is additionally force-logged on updates via the commit path [CITED: refs/files-backend.c `should_autocreate_reflog`, `log_ref_setup`]. Current Rust `should_log_refs` collapses this to a bool [VERIFIED: crates/git-command/src/commit.rs:654-656].
**How to avoid:** Port the `LOG_REFS_{UNSET,NONE,NORMAL,ALWAYS}` enum (`refs_parse_log_all_ref_updates_config`, incl. `"always"` string) and gate per-refname in the single writer.
**Warning signs:** `logallrefupdates` read anywhere outside the reflog helper.

### Pitfall 3: Non-atomic packed-refs rewrite tears the file
**What goes wrong:** Crash or concurrent update leaves half-written `packed-refs`; C git then fails to read the repo (crosswise break).
**Why it happens:** `packed-refs` must be rewritten whole (sorted, with `# pack-refs with: peeled fully-peeled sorted` header and `^<peeled>` continuation lines for tags), locked via the packed-refs lock, then renamed; loose refs are unlinked only after [CITED: refs/files-backend.c:1450-1590 `should_pack_refs`/pack transaction]. Current Rust code only *reads* packed-refs [VERIFIED: crates/git-refs/src/lib.rs:138-156].
**How to avoid:** Implement packed-refs write as lock→write-temp→fsync→rename; hold the packed-refs lock across the whole ref transaction (C takes it at transaction commit).
**Warning signs:** Any direct `write(packed-refs)` without the lock helper; unsorted output; missing `^` peeled lines.

### Pitfall 4: `--stdin` treated as independent single updates
**What goes wrong:** `t/t1400` batch cases and all of `t/t1404` (D/F conflicts, `start/prepare/commit/abort`, old-oid verification, symref verbs, `-z` NUL mode) fail.
**Why it happens:** The batch grammar has ~10 verbs (`update/create/delete/verify/symref-*/start/prepare/commit/abort/option`) with per-verb arity and die-strings (`"create %s: missing <new-oid>"`, `"delete %s: extra input: %s"`…) [CITED: builtin/update-ref.c `parse_cmd_*`], plus `ref_transaction_*` semantic checks (old value, existence, D/F collisions) that must all pass before any rename.
**How to avoid:** Two-phase implementation: parse all lines into ops (byte-exact die on syntax), then run C's transaction checks, then commit. Current `UpdateRef::run` handles only single-ref `-d`/set and must be kept as the non-`--stdin` path [VERIFIED: crates/git-command/src/update_ref.rs:11-54].
**Warning signs:** Renames happening inside the parse loop; missing `start`/`prepare`/`commit`/`abort` verb handling.

### Pitfall 5: Config scope precedence and worktree-config gating wrong
**What goes wrong:** `t/t1300` scope tests fail; `config.worktree` leaks into repos without `extensions.worktreeConfig`.
**Why it happens:** C order is system → XDG → user → local → worktree(cmdline last), and the worktree file applies **only** when `repository_format_worktree_config` is set [CITED: config.c `do_git_config_sequence`]. Current Rust discovery reads only the local file [VERIFIED: crates/git-core/src/lib.rs:144-150]; `-c`/`GIT_CONFIG_COUNT` overlays exist only in `RepoContext::repository()` [VERIFIED: crates/git-command/src/lib.rs:290-297], not in bare `Repository::discover`.
**How to avoid:** Build one scope-loading function used by both discovery and the `config` command's `--system/--global/--local/--worktree/-f/--blob` selectors; honor `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` env overrides [CITED: config.c `git_system_config`, `git_global_config_paths`, `git_config_system`].
**Warning signs:** Scope order implemented in more than one place; worktree file read without checking the extensions flag.

### Pitfall 6: includeIf condition set incomplete
**What goes wrong:** `t/t1305-config-include.sh` conditional cases fail.
**Why it happens:** C supports **six** prefixes — `gitdir:`, `gitdir/i:`, `onbranch:`, `hasconfig:remote.*.url:`, plus `worktree:` and `worktree/i:` — unknown conditionals are silently false [CITED: config.c `include_condition_is_true`]; current Rust code handles only unconditional `include.path` [VERIFIED: crates/git-config/src/lib.rs:189-198]. `MAX_INCLUDE_DEPTH 10` with a die (not silent skip) on overflow [CITED: config.c:135-185].
**How to avoid:** Port the condition matcher fully (case-insensitive `gitdir/i`, trailing-slash gitdir matching, `**` patterns via wildmatch semantics); keep cycle-vs-depth as distinct errors.
**Warning signs:** `includeIf` handled as plain `include`; missing `onbranch` HEAD-branch lookup.

### Pitfall 7: Config write clobbers file layout
**What goes wrong:** `git config set/unset/remove-section` rewrites the whole file, destroying comments/ordering; `t/t1300` + `t/t1308-config-set.sh` compare exact file content.
**Why it happens:** `ConfigSet::set` only appends in memory [VERIFIED: crates/git-config/src/lib.rs:241-261] — there is no file read-modify-write yet. C edits the scope file in place, preserving layout.
**How to avoid:** New `file.rs` edit layer: locate the target section/key lines, splice values, append sections as needed; multivar ops (`--add/--replace-all/--unset-all`) must handle repeated keys with optional `--value`/`--fixed-value` matching.
**Warning signs:** `set` implemented as file rewrite from `entries()`.

## Code Examples

Verified patterns from in-repo sources (C oracle + existing Rust):

### Reflog line format (C writer — the byte contract)

```c
// Source: refs/files-backend.c log_ref_write_fd (read this session)
strbuf_addf(&sb, "%s %s %s", oid_to_hex(old_oid), oid_to_hex(new_oid), committer);
if (msg && *msg) { strbuf_addch(&sb, '\t'); strbuf_addstr(&sb, msg); }
strbuf_addch(&sb, '\n');
```

i.e. `<old-hex> <new-hex> <Name> <email> <ts> <tz>\t<message>\n` — committer from `git_committer_info`, message may be empty (no tab then). The existing Rust helpers already split ident at `>` [VERIFIED: crates/git-command/src/checkout_core.rs:267-271] — reuse that split verbatim.

### Reflog expire defaults (C struct init — read this session, discrepancy flagged)

```c
// Source: reflog.h:25-28 (verbatim)
#define REFLOG_EXPIRE_OPTIONS_INIT(now) { \
	.default_expire_total = now - 30 * 24 * 3600, \
	.default_expire_unreachable = now - 90 * 24 * 3600, \
}
```

Mapping `gc.reflogExpire` → `default_expire_total`, `gc.reflogExpireUnreachable` → `default_expire_unreachable` [CITED: reflog.c `reflog_expire_config`]; per-ref `gc.<pattern>.reflogExpire*` overrides; `refs/stash` never expires when unconfigured [CITED: reflog.c `reflog_expire_options_set_refname`]. **The 30/90 magnitudes read swapped vs the documented 90/30 family — verify against the built C binary; `t/` wins** (Open Question O1).

### update-ref usage surface (must all parse)

```c
// Source: builtin/update-ref.c git_update_ref_usage (verbatim)
N_("git update-ref [<options>] -d <refname> [<old-oid>]"),
N_("git update-ref [<options>]    <refname> <new-oid> [<old-oid>]"),
N_("git update-ref [<options>] --stdin [-z] [--batch-updates]"),
```

Batch verbs: `update|create|delete|verify|symref-update|symref-create|symref-delete|symref-verify|start|prepare|commit|abort|option` [CITED: builtin/update-ref.c `parse_cmd_*`]. Note current Rust `UpdateRef` rejects `-m` silently-ok but dies on other flags [VERIFIED: crates/git-command/src/update_ref.rs:19-28] — C's `-m <msg>` sets the reflog message and `--create-reflog`/`--no-deref` alter transaction flags; port them.

### Config action matrix (must all exist)

Actions: `get|get-all|get-regexp|get-urlmatch|replace-all|add|unset|unset-all|rename-section|remove-section|list|edit|get-color|get-colorbool` + legacy flag spellings (`--get/--get-all/--replace-all/--unset/--unset-all/-l`, run in both `legacy` and `subcommands` modes per `t/t1300`) [CITED: builtin/config.c `cmd_config_actions`]; file selectors `--system/--global/--local/--worktree/-f/--blob`; display `--show-origin/--null/--name-only/--type/--bool/--int/--path/--expiry-date/--fixed-value/--default/--includes` [CITED: builtin/config.c `CONFIG_LOCATION_OPTIONS`/`CONFIG_DISPLAY_OPTIONS`/`CONFIG_TYPE_OPTIONS`]; mutual-exclusion errors exit 129 (e.g. `--show-origin` only with get/list) [CITED: builtin/config.c action checks].

### Error-code mapping (existing, reuse)

```rust
// Source: crates/git-command/src/lib.rs:75-96 (verbatim semantics)
CommandError::usage(..)  // code 129
CommandError::fatal(..)  // code 128
CommandError::error(..)  // code 1
CommandError::silent(..) // code as given, no message
```

C lock/transaction failures are `die()` → exit 128 with `fatal:` prefix [ASSUMED — die convention; verify each string against built C binary during planning].

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `git config --get/--list` legacy flags only | Dual spelling: legacy flags AND `get/set/unset/list` subcommands, both tested | C 2.x (t1300 loops `legacy`+`subcommands` modes) [CITED: t/t1300-config.sh mode loop] | Phase 1 must implement both spellings |
| Single-ref `update-ref` | `--stdin` batch transactions with `start/prepare/commit/abort` | Long-standing C surface; `t/t1404` pins error matrix | Full grammar required (D-06), not single-ref only |
| Loose-refs-only writes | Files-backend transaction also rewrites `packed-refs` under lock | C files-backend design | D-07: packed-refs write is in-scope even though the `pack-refs` *command* is Phase 2 (SCRIPT-03) |
| Ad-hoc reflog appends per command | Single `files_log_ref_write` with `log_ref_setup` gating | C files-backend design | Consolidate the 3 Rust call sites into one helper |

**Deprecated/outdated:**
- `t/t3210-pack-refs.sh` (named in CONTEXT.md Canonical References): **does not exist in this tree** — use `t/t0601-reffiles-pack-refs.sh` + `t/pack-refs-tests.sh` instead (Open Question O2).
- `RefStore::update` temp-name scheme (`.lock.<pid>`): superseded by C-exact `<ref>.lock` design (Pitfall 1).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | C `die()` in ref/config paths always exits 128 with `fatal: ` prefix | Code Examples | Medium — each message still needs binary-verification; planner should add a verify-each-string task |
| A2 | System C git for crosswise is `/usr/bin/git` 2.50.1 (Apple Git) while tree oracle is v2.55.0-540 — behavior deltas between versions could cause false crosswise failures | Environment Availability | Medium — planner should pin `SYSTEM_GIT` to a freshly built tree binary for phase gates |
| A3 | `reflog expire` reachability walk can reuse `git-revision` rev-walk instead of porting C's mark-list (`mark_limit`, `UE_*` kinds) | Architecture Patterns | Medium — expire-correctness depends on identical reachable sets; needs a `t/t1410`-driven check |
| A4 | No new test harness needed — `cargo test` + `cargo xtask differential/scoreboard` cover unit/crosswise/`t/` gates | Validation Architecture | Low — harness exists and is documented in AGENTS.md |

## Open Questions (RESOLVED — dispositions to owning plan tasks)

1. **Reflog expire default magnitudes → RESOLVED → 01-01 Task 3**
   - What we know: `reflog.h:25-28` initializes total=30d/unreachable=90d; `reflog_expire_config` maps `gc.reflogExpire`→total, `gc.reflogExpireUnreachable`→unreachable [VERIFIED: reflog.h:25-28; CITED: reflog.c].
   - What's unclear: whether this tree intentionally differs from the documented 90/30 family, and what the built binary actually enforces.
   - Recommendation: planner adds a task to probe the built C binary (`reflog expire --dry-run --verbose` on dated entries) and follow `t/t1410` expectations; `t/` wins ties.

2. **Packed-refs gate identity (CONTEXT references non-existent `t/t3210`) → RESOLVED → 01-01 Task 3 + 01-04**
   - What we know: `t/t3210-pack-refs.sh` absent; packed-refs behavior is pinned by `t/t0601-reffiles-pack-refs.sh` (22 lines, sources `t/pack-refs-tests.sh`) covering `pack-refs --all/--prune/--no-prune`, show-ref interplay, D/F conflicts [CITED: t/t0601-reffiles-pack-refs.sh; t/pack-refs-tests.sh].
   - What's unclear: whether Phase 1 must also port the `pack-refs` *command* (currently Phase 2 SCRIPT-03) or only the transaction-layer packed-refs rewrite.
   - Recommendation: planner resolves with user; researcher recommends transaction-layer write + read paths in Phase 1, `pack-refs` command stays Phase 2 unless success-criterion #2's `t/` gate demands it.

3. **`reflog delete` vs `drop` vs `expire` subcommand split → RESOLVED → 01-03**
   - What we know: `builtin/reflog.c` implements `show|list|exists|write|delete|drop|expire` with distinct usages (`delete [--rewrite] [--updateref]`, `drop [--all ...]`, `expire [--stale-fix] [--dry-run]...`) [CITED: builtin/reflog.c usage block].
   - What's unclear: exact per-subcommand test weight in `t/t1410` (696 lines) vs `t/t1411,t1412,t1413,t1414,t1417` (show/loop/detach/walk/updateref — possibly out-of-scope neighbors).
   - Recommendation: planner scopes to `t/t1410` + `t/t1417` explicitly; lists sibling scripts as out-of-scope.

4. **Config `--blob` / `--file` editing scope → RESOLVED → 01-04 Task 1**
   - What we know: C supports `--blob` reads and `-f` file writes; `t/t1307-config-blob.sh`, `t/t1308-config-set.sh` exist.
   - What's unclear: whether `--blob` write or `--edit` (editor spawn) fall in Phase 1's "full matrix" (D-09 lists get/set/unset/list + add/replace-all/remove-section/show-origin/type only).
   - Recommendation: planner treats `--edit` as out (interactive editor, like commit's deferred editor path); `--blob` read in, `--blob` write out unless a gate demands it.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` / `rustc` | Build + test (run from `crates/`) | ✓ | 1.97.1 (MSRV 1.74 OK) | — |
| System C git (`/usr/bin/git`) | Crosswise comparator (default) | ✓ | 2.50.1 (Apple Git-155) | Build tree oracle via `make` at root; set `SYSTEM_GIT` to it (see A2) |
| C oracle tree + `t/` harness | Behavior spec, gate scripts | ✓ | v2.55.0-540 tree; gates `t/t1410` (696 lines), `t/t1400` (2488), `t/t1300` (3022), `t/t1404`, `t/t0601`, `t/t1305`, `t/t1308` | — |
| `proptest` | Parser/serializer property tests | ✓ | Declared in `git-config` (also used cross-crate) | — |
| `cargo llvm-cov` (≥90% gate) | Phase done-gate coverage | ? | Unverified this session | Planner verifies; fallback `cargo tarpaulin` [ASSUMED] |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** tree-built C git binary (fallback: system 2.50.1 with version-skew risk A2).

## Validation Architecture

| Property | Value |
|----------|-------|
| Framework | `cargo test` (unit + integration) + `cargo xtask differential` (crosswise) + `cargo xtask scoreboard` (`t/` regression gate) |
| Config file | `crates/Cargo.toml` workspace; suites registered in `crates/xtask/src/main.rs::suites()` |
| Quick run command | `cd crates && cargo test -p git-refs -p git-config -p git-command` |
| Full suite command | `cd crates && cargo test --workspace && cargo xtask differential` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REFS-01 | reflog read/show/expire/delete; every mutating cmd logs | unit + `t/` gate | `cargo test -p git-refs` + `cd t && ./t1410-reflog.sh` via shim | ❌ Wave 0 (no `crates/git-refs/tests/`; unit tests inline only) |
| REFS-02 | lock+rename, `--stdin` atomic batches, packed-refs write, contention text | unit + crosswise + `t/` gate | `cargo test -p git-command` + `t1400`/`t1404`/`t0601` via shim | ❌ Wave 0 (extend `update_ref` tests; new crosswise suite file) |
| CONF-01 | scopes/includes/matrix/multivar, C-exact errors | unit + proptest + crosswise + `t/` gate | `cargo test -p git-config` + `t1300`/`t1305`/`t1308` via shim | Partial (unit+props exist; scope/includeIf/file-edit tests missing) |
| Gate 4 | No scoreboard regression | harness | `cargo xtask scoreboard` | ✅ exists (`crates/scoreboard.json`, 2 top-level keys) |

### Sampling Rate
- **Per task commit:** `cd crates && cargo test -p git-refs -p git-config -p git-command`
- **Per wave merge:** `cd crates && cargo test --workspace`
- **Phase gate:** Full suite green + `cargo xtask differential` + `cargo xtask scoreboard` clean before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/git-refs/tests/` — lock/transaction/reflog/packed-refs-write integration tests (covers REFS-01/02)
- [ ] `crates/git-command/tests/phase1_crosswise.rs` — reflog/config/update-ref `--stdin` byte-parity suites + registration in `xtask::suites()` (covers all three reqs)
- [ ] `crates/git-config/tests/` (or inline) — scope precedence, includeIf matrix, file-edit preservation tests (covers CONF-01)
- [ ] `scripts/shim-git` — add `reflog|config` to the `case` list [VERIFIED: scripts/shim-git:15]
- [ ] Framework/config check: confirm `cargo llvm-cov` availability for the ≥90% gate

## Security Domain

`security_enforcement: true`, ASVS level 1 [VERIFIED: .planning/config.json:48-50].

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (no auth in this phase) |
| V3 Session Management | no | — |
| V4 Access Control | partial | Symlink/`commondir` redirection only inside repo; `safe.bareRepository`-style discovery guards inherited from `git-core` |
| V5 Input Validation | **yes** | Refname validation (extend `validate_refname`); `--stdin` arity/quoting dies; config key/section parse with `BadLine{line,file}` fatal [VERIFIED: crates/git-config/src/lib.rs:37-46] |
| V6 Cryptography | no | Oids treated as opaque bytes; no new crypto (locked decision) |
| V14 Configuration | **yes** | Scope precedence fixed; `includeIf.hasconfig:remote.*.url` must forbid remote-URL smuggling (C `forbid_remote_url` path [CITED: config.c]) |

### Known Threat Patterns for files-backend + config

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Symlink escape in ref/lock paths (`refs/heads/x` → outside `.git`) | Tampering / Elevation | Canonicalize + reject escapes; D/F-conflict checks in transaction (C `ref_update` checks) |
| Malicious `include.path` (absolute / `~` / recursive) | Tampering | Depth cap 10 + cycle guard; relative-to-including-file resolution only [CITED: config.c:135-185] |
| Lock-file squatting (pre-created `.lock` → DoS) | Denial of service | Stale-lock detection with C-exact error; never silently unlink foreign locks |
| Config injection via `-c`/`GIT_CONFIG_COUNT` into privileged keys | Elevation | Same as C: overlays allowed by design; `hasconfig` include path forbids remote URLs |

## Sources

> Note on method: the `gsd_run query research-plan` seam was unavailable in this environment (`gsd_run` not on PATH) and `.planning/config.json` disables all web providers (`brave/exa/tavily/ref/perplexity/jina/firecrawl: false`). No external fetch was needed: this phase's authority is the in-repo C oracle tree + `t/` gates + existing Rust crates, all read directly. No package names are recommended (workspace-internal only), so no registry verification was required.

### Primary (HIGH confidence — Read this session, verbatim quotes inline)
- `reflog.h:1-77` — expire options struct + `REFLOG_EXPIRE_OPTIONS_INIT` defaults
- `crates/git-refs/src/lib.rs:1-288` — `RefStore`, `RefError`, `validate_refname`, packed read
- `crates/git-config/src/lib.rs:1-589` — `ConfigSet`, `ConfigError`, include/cycle handling, proptests
- `crates/git-command/src/lib.rs:1-457` — `Command` trait, `CommandError` codes, `RepoContext`, `dispatch_with`
- `crates/git-command/src/update_ref.rs:1-92` — current single-ref implementation (extension point)
- `crates/git-command/src/commit.rs:270-303,654-673` — HEAD-vs-branch reflog gating pattern
- `crates/git-command/src/checkout_core.rs:211-280,909-915` — atomic-write, `reflog_append`, `reflog_action`, `log_all_ref_updates`
- `crates/xtask/src/depcheck.rs:1-50` — layer map + downward-edge rule
- `scripts/shim-git:1-21` — dispatcher `case` list (needs `reflog|config`)
- `AGENTS.md`, `.planning/{PROJECT,REQUIREMENTS,ROADMAP,STATE}.md`, `01-CONTEXT.md`, `crates/Cargo.toml`

### Secondary (MEDIUM confidence — observed via shell grep/sed this session against the C oracle)
- `builtin/reflog.c` — 7-subcommand usage matrix, expire callbacks, `reflog_expire_config` wiring
- `builtin/update-ref.c` — 3 usage lines, `parse_cmd_*` verb set + die strings
- `builtin/config.c` — 16-action matrix, location/display/type options, mutual-exclusion errors
- `config.c` — `do_git_config_sequence` scope order, 6 includeIf conditions, `MAX_INCLUDE_DEPTH 10`, env overrides
- `reflog.c` — `reflog_expire_config` key→slot mapping, per-ref override, `refs/stash` exemption
- `refs/files-backend.c` — `log_ref_setup`/`should_autocreate_reflog`, lock error strings, packed-refs transaction
- `refs.c:1059` + `refs/files-backend.c:131-163` — `core.logallrefupdates` tri-state parse
- `t/t1410-reflog.sh`, `t/t1400-update-ref.sh`, `t/t1404-update-ref-errors.sh`, `t/t1300-config.sh`, `t/t0601-reffiles-pack-refs.sh`, `t/pack-refs-tests.sh` — gate scripts (heads read; full bodies for planner/executor)

### Tertiary (LOW confidence — marked [ASSUMED], needs binary verification)
- Exact stderr strings/exit codes per failure path (die-convention assumed; each string must be confirmed against the built C binary)
- `cargo llvm-cov` availability for the coverage gate

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates exist, read, and tested; no external deps involved
- Architecture: HIGH — C oracle files located and structurally mapped; layering rules verified in code
- Pitfalls: HIGH — each pitfall anchored to a concrete C/Rust code divergence observed this session
- Expiry-default magnitudes + per-string stderr text: LOW — flagged as Open Questions / Wave-0 verify tasks, never stated as fact

**Research date:** 2026-09-28
**Valid until:** 2026-10-28 (stable domain: C git files-backend semantics change slowly; re-verify if tree rebases past v2.55.0-540)
