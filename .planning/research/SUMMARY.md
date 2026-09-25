# Project Research Summary

**Project:** git.rs — pure-Rust byte-compatible reimplementation of C git (upstream v2.55.0-540)
**Domain:** Developer tooling / VCS reimplementation (parity port, not greenfield product)
**Researched:** 2026-09-25
**Confidence:** HIGH

## Executive Summary

git.rs is a standalone pure-Rust rewrite of git whose entire product value is **byte-identical CLI behavior plus crosswise on-disk interop** with C git: identical stdout/stderr/exit codes, repos readable and writable in both directions. ~45 builtins are already ported behind a thin CLI surface and composition-root dispatch; the remaining work is the "usable repo" milestone — merge, sequencer workflows (cherry-pick/revert/rebase/stash), ref infrastructure, worktree engine completion, and store maintenance (repack/gc/MIDX/commit-graph-write). Every mature reimplementation (gitoxide, libgit2, C git's own `builtin/` over `lib/`) converges on the same layering this workspace already has, so the recommendation is to keep the architecture fixed and order remaining work by integration surface, not by builtin number.

The stack is deliberately locked and must stay that way: stable Rust (edition 2021, MSRV 1.74), sync `std::io` only (no async runtime), vendored SHA-1/sha1dc + SHA-256 in `git-hash`, `flate2` 1.1.10 exclusively behind the `git-compress` facade, hand-rolled CLI parsing and typed errors per crate (for C-exact exit codes 129/128/1/141), hand-rolled config/index/object parsers (no serde), and `proptest` plus the `differential`/`scoreboard`/`shim-git` harness as the parity proof. The main risks are all parity-shaped: exit-code/stderr-text drift, racy-clean index semantics, date/TZ divergence, ref writes without lock+transaction protocol, packs C git rejects, and merge rename-scoring approximation. Each is prevented the same way — copy C strings/semantics verbatim, assert `(stdout, stderr, code)` triples crosswise against system git, and gate every builtin on its `t/` family through the shim with zero scoreboard regression.

## Key Findings

### Recommended Stack

The workspace pins are load-bearing decisions, not defaults: byte parity needs zero-cost byte control, C-exact error text, and MSRV 1.74 discipline, which rules out `clap`, `serde`, `tokio`, external SHA crates, and second compression deps without a plan amendment. Anything adding those fails review. See `STACK.md` for the full version table and rejection rationale.

**Core technologies:**
- Rust stable, edition 2021, MSRV 1.74 — entire port; `depcheck`/`safety`/`msrv verify` gates enforce layering and near-zero `unsafe`
- Sync `std::io`/`std::fs`/`std::process`, no async runtime — matches C's single-invocation stateless CLI model
- Vendored `git-hash` (sha1 + sha1dc + sha256) — C-exact SHAttered collision semantics no external crate provides
- `flate2` **1.1.10** (default `miniz_oxide` backend) via `git-compress` facade only — sole zlib provider; bump lock 1.1.9 → 1.1.10
- Hand-rolled CLI parsing + `RepoContext` — C's bespoke `usage:`/`fatal:` text and `-C`/`-c`/`--git-dir` threading are not expressible in a derive framework
- Hand-rolled typed errors → `CommandError` — preserves the 129/128/1/141 exit-code contract `anyhow` would erase
- Hand-rolled config/index/object parsers, no serde — git formats are not serde-shaped
- `proptest` 1.11.0 (dev-only) + `cargo xtask` harness (`differential`, `scoreboard`, `gates`) + system C git oracle via `scripts/shim-git` — the parity proof

### Expected Features

For a parity port, "features" are C builtins plus on-disk capabilities; the bar is byte-identical output with crosswise-readable repos. ~45 commands are DONE and form the assumed base. See `FEATURES.md` for the full matrix with gates and conversion-plan IDs.

**Must have (table stakes — this "usable repo" milestone):**
- `merge` via merge-ort (C1) — the workflow hub and single largest remaining item; gate `t/t6402`–`t/t6430`
- Reflog read/write + `git reflog` (C5) — safety net every mutating command logs to; schedule early, it unlocks stash/rebase/recovery
- Ref locking/transactions + packed-refs write + `update-ref --stdin` (C6) — foundation everything stateful builds on; schedule early
- `config` command with scope resolution (C8) — library exists, command missing; high embarrassment factor
- `cherry-pick`/`revert` sequencer subset (C3) + `rebase` am+merge backends non-interactive (C4) — depend on merge machinery + reflog
- `rm`/`mv`/`clean` completion (B8), `apply` completion (B10), `show`/`describe`/`name-rev`/`shortlog` (B9) — cheap, visible, daily-use
- `rev-parse` completion + `merge-base` remaining flags + `merge-tree`/`cherry` (C2) — the scriptability layer
- Small plumbing batch C7 (`pack-refs`, `mktag`, `check-ref-format`, `diff-index`/`diff-files`, `show-index`, …) — parallel-track wins
- Commit-graph write (D3) + MIDX completion (D4) + `index-pack --stdin`/thin-pack (D5) + `gc`/`repack`/`prune` basics (D6) — finish the on-disk story
- `fsck` completion (C10) — the integrity headline promise
- Depth pass on routed commands (deferred-A8: word-diff, histogram/patience, pickaxe, dirstat, whitespace family) — parity is the moat, runs alongside

**Should have (competitive — differentiators, not new UX):**
- Full option-parity depth on routed commands — no competitor does depth; this is the moat
- Memory safety (near-zero `unsafe`, `safety` gate green) — eliminates C's CVE class; advertise per phase
- Performance via scoped-thread parallelism where C is serial (status, pack ops) — never at parity's expense, no async runtime
- Library-first crates as embeddable dependencies + publishing the verification harness (differential + crosswise + scoreboard) as a trust artifact

**Defer (v2+):**
- Network transports (ssh/http/daemon, network push) — file/local transport + `ls-remote` first; sockets need a plan amendment
- `submodule` family, email stack (`am`, `format-patch`, `send-email`), `verify-commit`/`verify-tag` + signing, interactive (`-p`/`-i`/pager/color), sparse-checkout/index, smudge/clean filters + full autocrlf matrix, `filter-branch`/`scalar`/`gitweb`/GUI/bridges — see Anti-Features table in `FEATURES.md`
- New UX on top of git semantics — fork-grade divergence that fails the differential gate by construction; innovate only in library APIs and speed

### Architecture Approach

Keep the current layering — it is what gitoxide, libgit2, and C git itself all converge on: thin `git-cli` surface (flags + exit codes only) → `git-command` composition root (one module per builtin, the sole upward-dependency exception) → language/query layer (`git-revision` RevWalk with loader callbacks, `git-pathspec`) → pure domain operations (`git-object`, `git-diff`, `git-merge`, `git-pretty`) → stores (`git-odb`, `git-refs`, `git-index`, the crosswise contract) → zero-dep foundation leaves (`git-hash`, `git-core`, `git-config`, `git-date`, `git-compress`). Data flows strictly downward; composition lives only in `git-command` (enforced by `depcheck`). New builtins follow the mechanical wiring checklist (`pub mod` + dispatch arm + `shim-git` case + crosswise suite + `suites()` registration); command families share engines (`checkout_core.rs` pattern → sequencer engine, ref-mutation engine). See `ARCHITECTURE.md` for the system diagram, data flows, and build-order rationale.

**Major components:**
1. `git-cli` (surface) — process entry, global flags, exit-code mapping 129/128/1/141; DONE, do not extend
2. `git-command` (composition root) — `Command` trait, `CommandError`, `RepoContext`, per-builtin modules; nearly all remaining work lands here
3. `git-revision` + `git-pathspec` (language) — rev strings → oid sets via storage-agnostic loader callbacks; single `RevWalk` with explicit ordering modes, never hand-rolled traversal
4. `git-object` / `git-diff` / `git-merge` (operations) — pure algorithms over bytes; rename scoring and merge3 live here behind shared crates
5. `git-odb` / `git-refs` / `git-index` (stores) — the crosswise contract; lock+transaction protocol, racy-clean helper, and pack-write choke point live here
6. `xtask` + `shim-git` + system C git (automation/oracle, exempt from layering) — differential byte-compare, `t/` suite runner, `scoreboard.json` regression gate

### Critical Pitfalls

Top risks distilled from all 10 critical pitfalls in `PITFALLS.md` (each with phase mapping and recovery cost there):

1. **Exit-code and stderr-text drift** — right answer, wrong code/paraphrased `fatal:` text; fails `t/` while unit tests pass. Avoid: centralize codes in `CommandError`, copy C `die`/`usage` strings verbatim, assert `(stdout, stderr, code)` triples for every builtin, handle SIGPIPE→141, TDD from `t/` stderr expectations. Applies to every builtin-porting phase.
2. **Racy-clean index shortcuts** — content-hash-everything (slow) or blind `stat` trust (wrong in the racy window); flaky crosswise failures. Avoid: port `ie_match_stat` semantics faithfully into one shared `is_racy_clean` helper in `git-index`, add racy-window fixtures. Must land before commit/checkout/stash scale work.
3. **Refs without transactions** — direct `fs::write` of ref files: torn `packed-refs`, lost concurrent updates, missing reflogs, lax refname validation. Avoid: port the `.lock` + atomic-rename + multi-ref transaction protocol once in `git-refs`, grep-ban direct ref writes, test concurrency + `log -g` through C. Blocks stash/worktree/concurrent commit.
4. **Packs C git rejects** — OFS_DELTA offset bias, unsorted idx fanout, thin packs stored un-fixed, missing checksums. Avoid: C-side seal on every pack-writing path (`verify-pack` + `index-pack --verify` + `fsck` + `clone` of Rust output, both directions), one `write_pack_opts` choke point, fuzz targets before `repack`/`gc`.
5. **Merge/diff rename-detection divergence** — plausible-but-wrong merges from line-similarity approximations; exit-code masking. Avoid: port `diffcore-rename` scoring + `unpack-trees` D/F rules first (unblocks B4/B7), then base selection, then content merge; crosswise on `t6400-t6499` conflict corpora with stages 1/2/3 + exit codes.

## Implications for Roadmap

Based on research, suggested phase structure (ordered by integration surface per `ARCHITECTURE.md` §Suggested Build Order, honoring `FEATURES.md` dependency chains with silent prerequisites first):

### Phase 1: Ref + Config Foundation
**Rationale:** Reflog (C5) + ref locking/transactions (C6) are silent prerequisites of nearly everything stateful (stash, rebase, reset recovery, worktree, fetch/push); the `config` command (C8) de-risks transport scoping. Cheapest work that unblocks the most features.
**Delivers:** Reflog read/append + `git reflog`; lock-file + multi-ref transaction protocol in `git-refs` with packed-refs write, symref writes, `update-ref --stdin`, refname validation; `config` command with full scope resolution (system/global/local/worktree/includes/env/`-c`); `RevWalk` ordering hardening if not already green.
**Addresses:** C5, C6, C8; rev-parse completion (A5 leftover) rides here as the scriptability unlock.
**Avoids:** Pitfalls 7 (refs without transactions), 4 (discovery/config precedence), 1 (exit-code drift — establish the triple-assert + shim-wiring discipline every later phase copies).

### Phase 2: File Lifecycle + Inspection Depth
**Rationale:** Cheap, visible, parallelizable wins on top of the Phase 1 foundation; closes dozens of `t/` scripts while the hard merge engine is still cooking. Embodies "depth before breadth."
**Delivers:** `rm`/`mv`/`clean` completion (B8), `apply` completion minus `--3way` (B10 partial), `show`/`describe`/`name-rev`/`shortlog` completion (B9), small plumbing batch C7 in parallel tracks, deferred-A8 depth items (`--word-diff`, `--histogram`/`--patience`, pickaxe `-S`/`-G`, `--dirstat`, whitespace family) running alongside.
**Uses:** Existing `git-diff`, `git-revision`, `git-pretty` composition; no new store code.
**Implements:** Read-only multi-store consumer pattern; shared-engine extraction where families emerge.
**Avoids:** Pitfalls 1 (machine flags `--quiet`/`--exit-code`/`--porcelain` first-class), 5 (single `git-pathspec`/`IgnoreEngine`, non-ASCII + quotepath fixtures).

### Phase 3: Index/Worktree Engine Completion
**Rationale:** The three-way tree-merge-into-worktree engine (unpack-trees + index merge) is the single choke point `checkout -m`, `merge`, `stash`, and `read-tree -m` all land on. B7 core is DONE but `-m`/merge-carrying remains — finishing it before merge-ort or merge has nowhere to land its result.
**Delivers:** Racy-clean port (`is_racy_clean` shared helper) + cache-tree writeback (B2), `read-tree -m/-u/--prefix` + `write-tree` edge cases (B4), unpack-trees `-m` + D/F + sparse rules (B7 remainder), index v3/v4 read + REUC groundwork (B2 partial), `diff --exit-code` correctness fix (merge gate prerequisite).
**Addresses:** B2, B4, B7; Phase 6 remainder.
**Avoids:** Pitfalls 2 (racy-clean), 5 (pathspec/ignore/attr finalization), 10-partial (readers-liberal policy for index extensions).

### Phase 4: Merge Core
**Rationale:** merge-ort (C1) is the hub — cherry-pick, rebase, stash, `apply --3way`, `read-tree -m` all consume it. Prerequisites (rename detection, merge-base completeness, ref transactions, unpack-trees D/F) are green after Phases 1–3, so this is the earliest safe slot for the single largest remaining item.
**Delivers:** `merge-ort` port (base selection via `merge-base --all` recursive, content merge with C-identical conflict markers, D/F handling), `merge-base --octopus/--independent` + `merge-tree` + `cherry` (C2), `apply --3way` completion, `fsck` completion (C10) as the integrity seal on everything merged.
**Addresses:** C1, C2, B10-remainder, C10.
**Avoids:** Pitfalls 9 (rename scoring as spec, conflict corpora), 8 (walk ordering — no hand-rolled sorts, skew/graft fixtures), 6 (any pack-touching merge output gets the C-side seal).

### Phase 5: Sequencer Workflows (cherry-pick / rebase / stash / worktree)
**Rationale:** All four compose merge + reflog + ref transactions + checkout engine — every dependency lands in Phases 1–4. Build the sequencer engine once, then the wrappers (repeat of the `checkout_core` pattern).
**Delivers:** New/expended sequencer engine (`MERGE_MSG`/`CHERRY_PICK_HEAD`, `--continue`/`--abort`), `cherry-pick`/`revert` (C3), `rebase` am + merge backends non-interactive (C4), `stash` push/pop/apply/list/drop, `worktree` add/list/lock/repair completion with `commondir` + worktree refs.
**Addresses:** C3, C4, C9-head (stash, worktree in plan order).
**Avoids:** Pitfalls 3 (date/ident funnel through `git-date`, TZ matrix on generated commits), 7-remainder (reflog byte-compare via C `log -g`), 1 (sequencer's many error paths need verbatim C strings).

### Phase 6: Store Maintenance + Local Transport Head
**Rationale:** Pack maintenance must exist before transport negotiates over it (negotiation advertises bitmaps; fetched packs are thin; maintenance runs post-fetch). Fuzz + coverage gates are prerequisites here, not follow-ups.
**Delivers:** Commit-graph write + chains + bloom (D3), MIDX completion (D4), `index-pack --stdin` + thin-pack fix-up (D5), `gc`/`repack`/`prune` basics (D6), pack fuzz targets (FOLLOWUPS B4) + `cargo llvm-cov ≥90%` wiring, then pkt-line + protocol v2 → `ls-remote` → file/local `clone`/`fetch` head (E1–E2, E8-subset) as the bounded transport start with zero sockets.
**Addresses:** D3, D4, D5, D6; E1–E2 transport head; C9-tail (`notes`, `bisect`, `grep`, `blame`) and bitmaps/cruft (D1/D2) queue behind as the next milestone.
**Avoids:** Pitfalls 6 (pack acceptability + bidirectional round-trips), 10 (one format per phase; `t1016`/`t14xx` gates tracked, stubs visibly marked), 8-remainder (reachability shared between `gc`/`fsck`/`count-objects`).

### Phase Ordering Rationale

- **Silent prerequisites first:** reflog + ref transactions + config command (Phase 1) unlock more downstream features than their own size suggests; scheduling them late would serialize everything behind them.
- **Choke-point engine before its consumers:** unpack-trees/index-merge (Phase 3) before merge-ort (Phase 4) before sequencer workflows (Phase 5) — any other order builds landing pads with no runway.
- **Integration surface, not builtin number:** single-store plumbing (Phases 1–2, cheapest gates) → shared engine (Phase 3) → full worktree porcelain touching every store crate (Phases 4–5, most valuable gates) → store-format changes + transport (Phase 6, must come with the writers they exercise).
- **Transport last within the milestone:** out-of-order transport integration-tests against stubs and multiplies matrix cost; file-local-only keeps the first transport value bounded with no socket/TLS/MSRV risk.
- **Depth alongside breadth:** the deferred-A8 option-parity backlog and C7 plumbing batch run as parallel tracks from Phase 2 onward so new commands never inherit untested flag interactions.

### Research Flags

Phases likely needing deeper research during planning (`/gsd-plan-phase --research-phase`):
- **Phase 4 (Merge Core):** `merge-ort` + `diffcore-rename` scoring + `unpack-trees` D/F rules are a multi-thousand-line subsystem where approximations silently produce wrong results — needs targeted C-source + gitoxide-reader research per plan.
- **Phase 5 (Sequencer):** sequencer state machine + `rebase` backend behaviors have the densest `t/` families (`t3400`, `t3501`–`t3510`); needs oracle-first (TDD from `t/` expectations) research.
- **Phase 6 (Store + Transport Head):** OFS_DELTA bias, idx fanout ordering, thin-pack fix-up, MIDX optional chunks, and pkt-line/protocol-v2 framing details live in C code, not specs — needs format-level research before writing.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Ref + Config):** lock-file + atomic-rename transactions and config layering are well-trodden; workspace already has the `git-refs`/`git-config` shape — follow existing crate patterns.
- **Phase 2 (File Lifecycle + Inspection):** per-builtin composition over existing diff/revwalk/pretty layers with the established wiring checklist; ~45 prior ports are the template.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Versions verified via crates.io registry + Context7 official docs on research date; workspace pins read directly from `Cargo.lock`/`Cargo.toml` |
| Features | HIGH | Remaining scope enumerated directly from dispatch table + `shim-git` + conversion-plan/REMAINING_TASKS DONE-PARTIAL-NOT-DONE ledger; competitor behavior MEDIUM where noted |
| Architecture | HIGH | Workspace layering verified against `.planning/codebase/` map at commit a157715; ecosystem shape cross-checked against gitoxide + libgit2 via Context7 |
| Pitfalls | HIGH | Plan/FOLLOWUPS gaps + phase-summary remainders read directly; pack/index/ref format details from C technical docs; divergence classes corroborated across libgit2/JGit/gitoxide histories |

**Overall confidence:** HIGH

### Gaps to Address

- **Network transport stack (sockets/TLS/SSH/HTTP, credentials per platform):** deliberately out of scope for this milestone; when Phase 10+ opens, require a plan amendment covering MSRV + static-binary + parity justification, with sync-I/O-first bias. Handle during roadmap scoping, not implementation.
- **Reftable backend + `compatObjectFormat` (sha1↔sha256, LMAP, `gpgsig-sha256`):** currently config-only stubs; schedule one format per phase with `t1016`/`t14xx` gates so they don't fossilize. Flag in roadmap as explicit follow-up phases.
- **Windows/macOS behavior parity (CRLF, modes, case-insensitivity):** CI currently Linux-shaped; if Windows becomes a milestone target, promote smudge/clean + autocrlf from P3 and add three-OS CI early. Validate target platforms during roadmap creation.
- **Pack-write performance envelope:** `pack-objects` writes non-deltified packs (logged deviation); `zlib-rs`-behind-facade is benchmark-contingent. Measure against C before claiming perf wins; never trade parity for speed.
- **Fuzz + coverage gate wiring:** fuzz targets (B4) and `cargo llvm-cov ≥90%` enforcement are still infra gaps — wire them in Phase 6 at the latest, before claiming done-gates green.

## Sources

### Primary (HIGH confidence)
- crates.io registry via `cargo search` (2026-09-25) — `flate2 1.1.10`, `proptest 1.11.0`, `clap 4.6.7`, `sha1`/`sha2`, `thiserror 2.0.21`, `tempfile`, `assert_cmd`, `gix 0.88.0`
- Context7 `/rust-lang/flate2-rs`, `/proptest-rs/proptest`, `/clap-rs/clap`, `/websites/rs_sha2`, `/gitoxidelabs/gitoxide`, `/libgit2/libgit2` — backend tables, API currency, layering cross-checks
- Repo state: `crates/git-command/src/lib.rs` dispatch (~45 commands), `scripts/shim-git`, `crates/Cargo.lock` + `Cargo.toml`, `.planning/codebase/` map, `.planning/PROJECT.md` constraints
- Plan docs: `docs/plan/conversion-plan.md` (Phases A–F), `REMAINING_TASKS.md`, `FOLLOWUPS.md`, `README.md` done-gates, phase-3–9 summaries
- C oracle: `builtin/` listing, `Documentation/technical/pack-format.txt`, `patch-delta.c`, `date.c`, `dir.c`/`pathspec.c`, `files-backend.c`, `revision.c`, `merge-ort`/`unpack-trees`, `fsck` catalog

### Secondary (MEDIUM confidence)
- gitoxide repo/docs (library-first shape, divergence costs, 2–10× perf reports), libgit2 project page + Gitaly #3314 (partial-parity dual-stack trap), Dulwich docs (`c-git-compatibility.txt`, portability claim), git-scm book plumbing/porcelain taxonomy
- Git man pages + sparse-checkout perf literature (P3 ranking), usage-frequency threads (daily-workflow command consensus)

### Tertiary (LOW confidence)
- None — no finding rests on a single source or pure inference; all rankings trace to the plan ledger or corroborated competitor histories.

---
*Research completed: 2026-09-25*
*Ready for roadmap: yes*
