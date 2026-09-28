# Phase 1: Refs & Config Foundation - Context

**Gathered:** 2026-09-28
**Status:** Ready for planning

## Phase Boundary

Users get a reflog safety net, crash-safe ref updates, and working git config management. In scope: REFS-01 (reflog read + every mutating command logs), REFS-02 (lock files + atomic rename + multi-ref transactions, packed-refs write, `update-ref --stdin`), CONF-01 (`git config` with system/global/local/worktree scopes, includes, `--list/--get/--unset`). Out of scope: everything in Phases 2–12 (rev-parse completion, merge, sequencer, store, transport, fsck depth, network, mail, submodules, interactive, filters, long tail).

## Implementation Decisions

### Reflog surface
- **D-01:** Full reflog surface in Phase 1, including `expire` (`--expire=now/--all`) and `delete` — not show+log only.
- **D-02:** Every ref-mutating command logs from day one, including branch/tag/update-ref/clone paths — not just checkout/commit/reset.
- **D-03:** C-exact reflog gating: HEAD always logged; branch ref logged only when `logallrefupdates` allows (existing `commit.rs` split is the pattern to extend).
- **D-04:** Honor C `gc.reflogExpire` / `gc.reflogExpireUnreachable` defaults (90/30-day family) via config; `--expire=now/--all` supported for tests.

### Transaction safety
- **D-05:** All-or-nothing multi-ref atomicity with C-exact lock/transaction error text — no best-effort partial apply.
- **D-06:** Full `update-ref --stdin` batch syntax (create/update/delete/verify lines) byte-exact in Phase 1 — not single-ref only.
- **D-07:** Packed-refs write included in Phase 1 alongside loose-ref locking (no torn packed-refs; success criterion #2).
- **D-08:** Lock-contention failures render byte-exact C stderr with correct exit codes; concurrent updates never tear state.

### Config command scope
- **D-09:** Full C option matrix in Phase 1: get/set/unset/list plus `--add/--replace-all/--remove-section/--show-origin/--type` — not basics only.
- **D-10:** All four scopes honored with C precedence (system → global → local → worktree); CLI `-c` / `GIT_CONFIG_COUNT` overlays beat files.
- **D-11:** C-exact multivar handling (`--null/--fixed-value/--get-urlmatch` behaviors) for script use.
- **D-12:** Invalid files/sections fail with C-exact fatal text (exit 128), byte-compared in crosswise.

### Includes & scopes
- **D-13:** Full conditional `includeIf` (`gitdir`/`onbranch`/`hasconfig`) plus `include.path` honored like C — not plain includes only.
- **D-14:** C-exact include path resolution: relative paths resolve from the including file; `~`/`~/` expansion like C.
- **D-15:** Locked precedence: CLI `-c` and `GIT_CONFIG_COUNT/K/V` beat all files; files apply system → global → local → worktree.
- **D-16:** C-exact include cycle guard with depth cap (no infinite loops); cycles fail C-exactly.

### the agent's Discretion
None — user made concrete selections on all 16 questions.

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Planning (scope + gates)
- `.planning/ROADMAP.md` Phase 1 section — goal, success criteria (4 items), `t/` gates
- `.planning/REQUIREMENTS.md` Refs & Config Foundation section — REFS-01, REFS-02, CONF-01 text
- `.planning/PROJECT.md` Constraints + Key Decisions — byte-identical contract, exit codes, layering, no-new-deps rule

### C oracle gates (behavior spec; `t/` wins ties)
- `t/t1410-reflog.sh` — reflog read/write/expire expectations
- `t/t1400-update-ref.sh` — update-ref single + `--stdin` transaction expectations
- `t/t3210-pack-refs.sh` — packed-refs read/write expectations
- `t/t1300-config.sh` — config command scopes/ops/includes expectations

### C source (reference only, never link)
- `refs.c` / `refs/` — lock + transaction + packed-refs reference
- `builtin/reflog.c` — reflog subcommand reference
- `builtin/update-ref.c` — `--stdin` batch syntax reference
- `builtin/config.c` — config option matrix + scope precedence reference
- `config.c` — include/includeIf resolution + cycle guard reference

### Existing Rust code (reuse / extend)
- `crates/git-refs/src/lib.rs` — `RefStore` (loose + packed-refs read, single-ref `update`, `validate_refname`); needs reflog read, locking, transactions, packed-refs write
- `crates/git-config/src/lib.rs` — `ConfigSet` (parse/layer/get/set/`set_cli`); needs CLI command + scopes + includes wired
- `crates/git-command/src/update_ref.rs` — `UpdateRef`/`SymbolicRef` (91 lines, no `--stdin`); extend here
- `crates/git-command/src/checkout_core.rs` — `reflog_append` + `reflog_action` (`GIT_REFLOG_ACTION`) helpers to reuse
- `crates/git-command/src/commit.rs` — `append_reflog` + HEAD-vs-branch gating pattern to extend
- `crates/git-command/src/lib.rs` — `RepoContext`/`dispatch`/`Command` trait; wire `reflog` + `config` commands here
- `scripts/shim-git` — add `reflog` + `config` to the `case` list when ported

## Existing Code Insights

### Reusable Assets
- `reflog_append` (`crates/git-command/src/checkout_core.rs:259`): append-one-line helper used by checkout/reset — reuse for all mutating commands.
- `reflog_action` (`crates/git-command/src/checkout_core.rs:910`): honors `GIT_REFLOG_ACTION` — reuse verbatim.
- `append_reflog` (`crates/git-command/src/commit.rs:659`): HEAD+branch writer with `logallrefupdates` split — extend to all writers.
- `ConfigSet` (`crates/git-config/src/lib.rs`): parse/get/set/`set_cli` layering already exists — build the CLI on top, don't re-parse.
- `RefStore::resolve/list/head_symbolic_target` (`crates/git-refs/src/lib.rs`): read path to reuse for reflog/show and config-adjacent ref work.

### Established Patterns
- Thin CLI + `Command` trait per builtin (`crates/git-command/src/lib.rs:333`): new `reflog`/`config` commands follow the one-module-per-builtin pattern, write to injected `out: &mut dyn Write`, never `println!`.
- `CommandError` exit codes (129 usage / 128 fatal / 1 error / 141 SIGPIPE): all new error paths map through these; stderr text byte-exact vs C.
- Strict downward layering (`cargo xtask depcheck`): ref/config logic lives in `git-refs`/`git-config`; composition only in `git-command`.
- Crosswise contract: Rust vs `/usr/bin/git` byte-identical stdout/stderr/exit; C must accept Rust-written refs/config artifacts and vice versa.

### Integration Points
- `git_command::dispatch` + `RepoContext::repository()` (`crates/git-command/src/lib.rs`): entry for both new commands; honors `-C/--git-dir/--work-tree/-c` automatically.
- `scripts/shim-git` `case` list: add `reflog|config` so the `t/` suite exercises the Rust port.
- `cargo xtask differential` suites + `crates/scoreboard.json`: register/guard the phase's `t/` gates; no scoreboard regression.

## Specific Ideas

User wants strict C parity everywhere in this phase: full surfaces (not minimal slices), C-exact messages/exit codes, C defaults honored (reflog expire, include resolution, scope precedence). No simplified/deferred variants accepted — researcher should resolve ambiguities from C source + `t/` gates, not by narrowing scope.

## Deferred Ideas

None — discussion stayed within phase scope.

---

*Phase: 1-Refs & Config Foundation*
*Context gathered: 2026-09-28*
