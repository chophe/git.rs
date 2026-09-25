# Feature Research

**Domain:** Byte-compatible pure-Rust reimplementation of C git (git.rs port, upstream `v2.55.0-540`)
**Researched:** 2026-09-25
**Confidence:** HIGH (repo state read directly from `crates/git-command/src/lib.rs` dispatch table, `scripts/shim-git`, `docs/plan/conversion-plan.md` + `REMAINING_TASKS.md`; competitor behaviour from gitoxide/libgit2/Dulwich docs and community sources — MEDIUM where noted)

## Feature Landscape

> Framing note: for a byte-compatible port, "features" are not green-field product ideas — they are **C git builtins + on-disk capabilities**, and the acceptance bar is **byte-identical stdout/stderr/exit codes** with crosswise-readable repos. Table stakes are therefore defined by (a) the daily-workflow porcelain every git user touches, (b) the plumbing that scripts, editors, and hosting tools shell out to, and (c) the subset every serious reimplementation (gitoxide, libgit2, JGit, Dulwich, go-git) converges on. Anything missing from (a)/(b) forces users back to C git via the shim — i.e. users "leave".

### Table Stakes (Users Expect These)

Already-ported (~45 commands in `dispatch_with` + `shim-git`) are the foundation and are NOT re-listed as work — they are the assumed base: `init add commit status checkout/switch/restore reset rm mv clean hash-object commit-tree verify-pack unpack-objects pack-objects count-objects multi-pack-index commit-graph cat-file ls-tree mktree write-tree read-tree rev-list log diff-tree diff ls-files update-index rev-parse show show-ref for-each-ref update-ref symbolic-ref branch tag merge-base merge-file fsck apply check-ignore check-attr index-pack`.

What follows is the **remaining** table stakes — the gap between "core object layer exists" and "usable repo". Ordered roughly by user pain if missing.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| `git merge` (merge-ort backend) | THE daily collaboration command; without it no branch workflow completes | HIGH | `git-merge` major expansion: rename detection, dir/file conflicts, recursive criss-cross bases, index merge, conflict clustering parity. Gate: `t/t6402`–`t/t6430`. Conversion-plan C1. This is the single largest remaining porcelain item. |
| `reflog` read/write + `git reflog` | Users expect safety net (`reset --hard @{1}`, recovery); branch/delete/update must log | LOW-MEDIUM | `logs/<ref>` format + reflog-aware update/branch/delete in `git-refs`. Gate: `t/t1410`. Conversion-plan C5. Prerequisite for `stash`, safe `reset`, `checkout -`. |
| Ref locking / transactions, `packed-refs` write, `update-ref --stdin`, symref writes | Concurrent/parallel safety + every script that atomically updates refs (`push`, `fetch`, `stash`, `rebase` all depend on it) | MEDIUM | `git-refs` expansion: lock files, `*_lock` protocol, transaction semantics, worktree-specific refs. Gate: `t/t1400`, `t/t3210`. C6. Without this, later phases build on sand. |
| `git config` command (read/write, scopes, includes) | Every setup flow starts here (`user.name`, `init.defaultBranch`, includes, `--list/--get/--unset`); tools shell out to it constantly | MEDIUM | `git-config` + `git-command/config.rs`: system/global/local/worktree scopes, includes, env vars, `--edit`. Gate: `t/t1300`. C8. Currently the config *library* exists but the *command* does not — high embarrassment factor. |
| `cherry-pick` / `revert` (sequencer subset) | Core history-editing workflow; expected alongside `merge` | HIGH | New `git-sequencer` crate (sequencer state machine, `MERGE_MSG`/`CHERRY_PICK_HEAD`, conflict-resume `--continue/--abort`). Gate: `t/t3501`–`t/t3510`. C3. Depends on merge machinery + reflog. |
| `git rebase` (am + merge backends) | As common as merge in many shops; missing = port unusable for them | HIGH | Same `git-sequencer` crate; am-backend first, merge-backend, interactive deferred. Gate: `t/t3400` family. C4. Depends on cherry-pick/sequencer. |
| `git show` completion / `describe` / `name-rev` / `shortlog` / `whatchanged` | Read-only history inspection users run dozens of times a day; `show` is partially ported but shallow | LOW-MEDIUM | `git-command` small modules; mostly composition over existing revwalk+pretty+diff. Gate: `t/t4000`, `t/t4201`, `t/t6120`. B9. High value / low risk — good early wins. |
| `git stash` (push/pop/apply/list/drop) | Ubiquitous interrupt-driven workflow (`stash` → switch branch → pop) | MEDIUM-HIGH | Composes commit + reflog (`refs/stash`) + merge + checkout. Order per plan: `stash` first among C9. Respective suite `t/t7500` family. Requires reflog (C5) + merge (C1) first. |
| `git worktree` completion (add/list/lock/repair) | Multi-checkout flow is mainstream now; partial `worktree.rs` exists | MEDIUM | Needs `git-worktree` + ref locking + `unpack-trees` + worktree-specific refs (`refs/worktree/*`, `commondir`). Respective suite. Order: after `stash`. |
| `git rm` / `mv` / `clean` completion | File-lifecycle porcelain; stubs exist but B8 is NOT DONE | LOW-MEDIUM | `t/t3600`, `t/t7001`, `t/t7300`. B8. Depends on index + ignore engine (both done — A11). Straightforward; should precede merge work. |
| `git apply` completion (`--3way`, `--index`, `--reject`, binary, whitespace) | `apply` routes but B10 is NOT DONE; patch workflows (non-GitHub flows, `rebase`, `am`) need it | MEDIUM | `git-command/apply.rs`. Gate: `t/t4103`–`t/t4137`. B10. `--3way` depends on merge machinery. |
| `rev-parse` completion (`@{...}`, `A..B`/`A...B`, `--all`, `--is-bare-repository`, `--sq`) | Scripts' universal argument parser; incompleteness silently breaks every downstream tool | MEDIUM | `rev_parse.rs`. Gate: `t/t1500`–`t/t1503`. Phase-A leftover (A5 in conversion-plan numbering). Unblocks log ranges, commit-tree, for-each-ref filters. |
| `merge-base --octopus/--independent`, `merge-tree`, `cherry` | Merge-adjacent plumbing that `merge`, `rebase`, and hosting UIs assume | MEDIUM | `git-merge`. Gate: `t/t6010`, `t/t6602`. C2. `--is-ancestor` already DONE — these are the remaining flags. |
| Small independent plumbing batch (`pack-refs`, `mktag`, `check-ref-format`, `stripspace`, `var`, `patch-id`, `check-mailmap`, `interpret-trailers`, `show-index`, `unpack-file`, `diff-files`, `diff-index`, `for-each-repo`, `url-parse`) | Each is tiny but each has scripts depending on it; collectively they close dozens of `t/` scripts | LOW (each) | `git-command` small modules, mutually independent — ideal parallel-track / contributor wins. C7. `check-ignore`/`check-attr` already DONE; the rest are open. |
| `gc` / `repack` / `prune` / `prune-packed` / `maintenance` | Repos grow without bound otherwise; hosting and long-lived clones require it | MEDIUM-HIGH | `git-command` over pack write + MIDX + bitmaps. Gate: `t/t7700`–`t/t7704`, `t/t5304`. D6. Depends on commit-graph write (D3) + MIDX (D4) for full parity, but a basic repack/prune lands earlier. |
| Commit-graph **write** + chains + bloom query | C git writes graphs on `gc`/fetch; repos the port writes must be acceleratable the same way or C-side tooling degrades | MEDIUM | `git-commitgraph` (read exists, write missing). Gate: `t/t5318`, `t/t5324`. D3. Read/write asymmetry is a crosswise hazard — prioritize. |
| MIDX completion (`RIDX`/`BTMP`/`BASE`, incremental, `--preferred-pack`) | Large repos (the ones that matter) use MIDX; partial MIDX = corrupt-looking repos to C git | MEDIUM-HIGH | `git-odb/midx`. Gate: `t/t5319`, `t/t5334`. D4. |
| `index-pack --stdin` + thin-pack base resolution | Required the moment any fetch/clone path produces packs (and for `t/t5302` parity) | MEDIUM | `index_pack.rs` exists with `--verify`; stdin + thin-pack missing. D5. Do before any transport work. |
| Index v3/v4 read/write + REUC extension | Forward-compat: C git 2.38+ can write v4; refusing to read = hard failure on real-world repos | MEDIUM | `git-index` (cache-tree `TREE` done; REUC/v3/v4 pending — B2 PARTIAL). Gate: `t/t0060`, `t/t3007`. Silent corruption risk if written loosely — needs exactness. |
| `read-tree -m/-u/--prefix` + `write-tree` edge cases | Three-way read-tree is the engine under `checkout -m`, `merge`, `stash`; one-way only (current) blocks them | MEDIUM | `git-command` (B4 PARTIAL). Gate: `t/t1000`, `t/t2000`. Depends on unpack-trees completion (B7 core done, `-m` pending). |
| `fsck` completion (`--strict`, `--connectivity-only`, `--no-dangling`, `--full`, `--lost-found`, message catalog) | The integrity story is the port's headline promise; partial fsck undermines it | MEDIUM | `fsck.rs`. Gate: `t/t1450` full pass. C10. Message-catalog parity is tedious but high-trust-value. |
| `clone` / `fetch` / `push` / `pull` (local/file transport first) | Users literally cannot get code without clone; the #1 "is this real git?" test | HIGH | `git-transport` + pkt-line + protocol v2 + negotiation (E1–E4, E8). Local/file transport first (`ls-remote`, no network) — real value with bounded scope. Full network (ssh/http/daemon) deferred to v2. |
| `grep` | Daily code-search porcelain; expected in any "complete" claim | MEDIUM | Threaded search over worktree+index+history; respects ignore/attributes (both done). Respective suite `t/t7810` family. Order per plan: after bisect, before blame. |
| `blame` / `annotate` | Code-archeology staple; editors shell out to `git blame --porcelain` constantly | MEDIUM-HIGH | Line-history over revwalk + diff; `--porcelain/-p` output stability is the contract. Respective suite. Porcelain-output parity matters more than speed here. |

### Differentiators (Competitive Advantage)

For a byte-compatible port, differentiation does NOT mean new UX (that would break parity — see Anti-Features). It means *being a better git than C git while behaving identically*. This aligns directly with the PROJECT.md Core Value (byte-identical behavior + crosswise interop).

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Full option-parity depth on already-routed commands | Every competitor stays shallow (gitoxide deliberately diverges CLI; libgit2 exposes a subset API; Dulwich covers common paths). Depth — `diff`'s ~100 options, `log --format` full, `rev-list` ordering flags — is the moat. The conversion-plan "depth before breadth" principle already bets on this. | MEDIUM-HIGH (cumulative) | Deferred A8 items are the backlog: `--word-diff`, `--color`, `--patience/--histogram`, `--dirstat`, whitespace family (`-w/-b`), `--relative`, `-S/-G` pickaxe, stat-width 80-col. Each is small; together they are what "byte-identical" means. |
| Memory safety (no CVEs of the C class) | C git has a steady stream of memory-safety CVEs (out-of-bounds in parsers, zlib handling). A pure-Rust port with near-zero `unsafe` (gated by `cargo xtask safety`) eliminates the class. Strong adoption argument for security-sensitive hosting. | LOW (ongoing discipline, not a feature build) | Keep the `safety` gate green; advertise per-phase. No extra code — it falls out of the stack choice. |
| Performance: parallel pack ops, fast status/diff | gitoxide demonstrates 2–10× wins on traversal/pack ops from fearless parallelism + clean data structures. A fast `status`/`fetch`-negotiation/`pack-objects` is felt daily on monorepos. | MEDIUM | No async runtime per constraints (MSRV 1.74, no tokio) — use scoped threads/`rayon`-style data parallelism only where C is serial. Measure against C, never at parity's expense. |
| Library-first crates as embeddable dependencies | gitoxide's real success is `gix-*` as libraries; libgit2 won by being linkable. The 24-crate workspace (odb/refs/revision/diff/merge as independent crates) is already shaped for this — crates.io releases turn the port into infrastructure, not just a binary. | LOW-MEDIUM (packaging + semver + docs, not new algorithms) | Requires API-stability pass per crate; keep `depcheck` layering so downstream crates stay acyclic. |
| The verification harness itself (differential + crosswise + `t/` scoreboard) | No competitor runs C's own `t/` suite as a merge gate. `cargo xtask differential/scoreboard`, `scripts/shim-git`, committed `scoreboard.json`, proptest + fuzz gates are a trust artifact: "every claim is machine-checked against the oracle". Publish the scoreboard. | LOW (exists — productize it: CI badges, docs) | `crates/xtask` + `scripts/shim-git` already exist. Differentiator is making it visible and keeping it green. |
| Safe concurrency primitives for hosting (parallel fetch, background gc/maintenance) | Gitaly-scale hosts want library-level parallelism without forking C git. Rust ownership makes parallel pack-index writes and background maintenance sound where C needs processes + lockfiles. | HIGH | Long-term; builds on ref-transactions (C6) + repack/gc (D6). Do not start before those. |
| First-class Windows/macOS behavior parity | C git's platform shims are a perennial bug source (CRLF, file modes, case-insensitivity). Rust `std` + explicit platform modules can match behavior with fewer `#ifdef`s. Dulwich explicitly sells "works anywhere Python does" on the same logic. | MEDIUM (cumulative) | Needs CI on all three OSes early; autocrlf/smudge filters (currently Phase-F deferred) become table stakes on Windows — revisit if Windows is a target. |

### Anti-Features (Commonly Requested, Often Problematic)

Explicitly record scope discipline. Sources: `docs/plan/conversion-plan.md` Phase F ("schedule last, defer freely" + "explicitly out of scope"), PROJECT.md Out of Scope (network/transport beyond file-local, FFI, C-tree changes).

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| New/different UX on top of git semantics (jujutsu-style CLI, new porcelain) | "While we're rewriting, let's fix git's UI" | Destroys the Core Value: byte-identical behavior is the entire product. Any divergence fails the differential gate by construction and forks the `t/` oracle. gitoxide deliberately chose divergence — that is a different product, not this one. | Keep CLI output byte-identical; innovate only in library APIs and performance, never in porcelain text. |
| Network transports (ssh/http/daemon, `push` over network, `send-email`, `imap-send`) in this milestone | "Clone over ssh is table stakes for real git" | True for *full* git, but PROJECT.md scopes transport out of the core-object-layer milestone, and conversion-plan orders it Phase E *after* repack/gc (D6) + bitmaps (D1). Starting early builds negotiation on missing pack infrastructure. | File/local transport + `ls-remote` first (bounded, no sockets); full E1–E10 as the next milestone, in plan order. |
| `submodule` family | Monorepo users ask for it | Historically the buggiest, most shell-scripted corner of C git (`submodule--helper`, dozens of edge cases); huge surface for modest milestone value. Plan defers to Phase F. | Defer to v2+; document as known-gap with C-git fallback via shim. |
| `filter-branch`, `send-email`/`mailinfo`/`mailsplit`, `imap-send`, `mergetool`/`difftool`, `daemon`, `scalar`, `backfill`, `diagnose`, `bugreport`, `hook`, `instaweb`/`gitweb`, `gui`/`citool`, `shell` | "Complete = all 150+ builtins" | Low daily use, some are separate products (email stack, web UI, GUI). Each consumes a full parity cycle for near-zero milestone validation. | Phase F stretch pool; pick only if a `t/`-gate milestone demands them. |
| `cvs*` / `svn` / `p4` / `quiltimport` / `archimport` bridges | "Migration completeness" | Explicitly "not planned" in conversion-plan Phase F; legacy one-way importers with their own format quirks. | Never build; point at C git. Record as "not planned" in roadmap. |
| GPG/SSH signature verification stack (`verify-commit`/`verify-tag`, signing in `commit`/`tag`) | "Signed commits are table stakes for supply chain" | Requires crypto verification stack + keyring integration; large, platform-entangled, and orthogonal to object-layer parity. Plan parks it in Phase F. | Parse-and-preserve `gpgsig` headers byte-exactly now (needed for hash correctness); verify later. |
| Interactive UI (`add -p`, `rebase -i`, `stash -p`, pager, `--color` everywhere) | Users love it | Terminal-interactive code is untestable in the differential harness (needs a pty oracle) and C's Perl/UI layers are the least spec-able. | Defer interactivity; keep `--no-pager` semantics and plain output byte-identical first. |
| Adding `clap`/`serde`/`tokio`/async or new crypto deps casually | "Faster development" | Locked architecture decision: `flate2` only via `git-compress`, no `clap`/`serde`/`tokio`; async runtime banned; `unsafe` gated. Violations break MSRV 1.74, layering (`depcheck`), and the safety story. | Hand-rolled arg parsing per command (matches C option quirks better anyway); scoped threads for parallelism; plan amendment required for any new dep. |
| Smudge/clean filters + `core.autocrlf` full matrix (early) | Windows correctness | Filter drivers fork every worktree path (checkout, add, diff, status) — multiplies the test matrix of the current milestone's core flows. Plan defers to Phase F. | Byte-transparent handling now; revisit as table stakes only if Windows becomes a milestone target. |
| Sparse-checkout / sparse-index / split-index (early) | Monorepo scale story | Layers锥 on top of unpack-trees + index extensions still in flight (B2 PARTIAL, B4 PARTIAL); premature optimization surface with its own `t/` families. | Land after checkout/index completion; then promote from P3 to P2. |

## Feature Dependencies

```
init ──requires──> config-layer (+ later: `config` command scopes)
  └──requires──> repo-discovery (RepoContext: -C/--git-dir/--work-tree/-c)  [DONE A2]

add ──requires──> index read/write ──requires──> ignore/attributes engine [DONE A11]
  └──requires──> pathspec matcher ──enhances──> status/ls-files/clean/rm

commit ──requires──> write-tree ──requires──> index ──requires──> ident (author/committer + tz) [DONE]

status ──requires──> index + ignore/attributes + diff + revision (HEAD)
  └──requires──> racy-clean stat scan ──enhances──> checkout/reset decisions

checkout / switch / restore / reset ──requires──> unpack-trees ──requires──> index + worktree helpers
  └──requires──> ref resolution (HEAD/symref) ──requires──> reflog (for `-`, `--orphan`, safety)
  └──pending──> `-m` (merge-carrying) ──requires──> merge machinery

merge (merge-ort) ──requires──> rename detection (diff) + index merge + unpack-trees
  └──requires──> merge-base (multi-base, criss-cross) ──requires──> revwalk/reachability
  └──requires──> ref transactions + reflog (`MERGE_HEAD`, `ORIG_HEAD` logging)

cherry-pick / revert ──requires──> merge machinery + sequencer state
rebase ──requires──> cherry-pick/sequencer ──requires──> reflog + ref transactions
stash ──requires──> commit + reflog (refs/stash) + merge + checkout
worktree (full) ──requires──> unpack-trees + ref locking + commondir layout

rev-parse (full) ──requires──> revision resolver (@{...}, ranges, --all) [PARTIAL]
  └──enhances──> everything scriptable (log ranges, for-each-ref, show, diff)

show / describe / name-rev / shortlog ──requires──> revwalk + pretty engine + diff [all exist]
grep ──requires──> ignore/attributes + (worktree | index | revwalk blobs)
blame ──requires──> revwalk + diff ──enhances──> editors (porcelain output contract)

gc / repack / prune ──requires──> pack-write (+delta DONE) + MIDX + commit-graph write + bitmaps
commit-graph write ──requires──> revwalk ──enhances──> rev-list/log/merge-base speed
MIDX full ──requires──> pack idx ──enhances──> gc/repack/large-repo reads

clone / fetch / push / pull ──requires──> pkt-line + protocol v2 + negotiation
  └──requires──> index-pack --stdin + thin-pack (D5) ──requires──> repack/gc (D6) + bitmaps (D1)
  └──requires──> ref transactions (C6) + config command scopes (C8) + credentials

fsck (full) ──requires──> odb + refs + index + message catalog ──validates──> everything above
```

### Dependency Notes

- **checkout/reset/merge/stash/rebase all require unpack-trees + index merge:** the three-way tree-merge-into-worktree engine is the single choke point of the "usable repo" milestone. B7 core is DONE; `-m`/merge-carrying and `--orphan`/`-p` remain — finish the engine before starting merge-ort, or merge has nowhere to land its result.
- **merge-ort (C1) is the hub:** cherry-pick, rebase, stash, `apply --3way`, and `read-tree -m` all consume it. It must land before any of them; its own prerequisites are rename detection (diff — DONE) + merge-base completeness (C2, partially DONE) + ref transactions (C6).
- **reflog (C5) + ref transactions (C6) are silent prerequisites of nearly everything stateful:** stash, rebase, `reset --hard` recovery, worktree refs, fetch/push ref updates. They look boring next to merge but block more features — schedule them early in the milestone.
- **`config` command (C8) enhances fetch/push/clone/credential scoping:** transport reads dozens of keys (`http.*`, `credential.*`, `remote.*`, includes); landing the command's scope resolution first de-risks Phase E.
- **transport requires D5 (index-pack stdin/thin-pack) + D6 (repack/gc) + D1 (bitmaps):** negotiation advertises bitmaps; fetched packs are thin; maintenance runs post-fetch. Out-of-order transport work integration-tests against stubs.
- **depth-before-breadth (locked plan principle):** routed-but-shallow commands (`diff` ~100 options, `log --format`, `rev-list` orderings) must reach option parity before new-command breadth — otherwise every new feature inherits untested flag interactions. The deferred-A8 list (word-diff, histogram/patience, pickaxe, dirstat, whitespace family) is the concrete backlog.
- **no conflicts in the strict sense** (git features compose), but two *ordering* anti-patterns: (1) starting network transport before pack maintenance exists; (2) starting interactive (`-p`/`-i`/pager/color) before plain-output parity — both multiply test-matrix cost without milestone payoff.

## MVP Definition

MVP here = **"usable repo" milestone**: a developer can clone (file-local), branch, merge, stash, rebase, inspect history, and gc — with the shim routing everything else to C git — and the `t/` suites for those flows pass through the shim with no scoreboard regression. Network clone/fetch/push is explicitly the *next* milestone (Phase E), not this one.

### Launch With (v1 — this milestone)

- [ ] `git merge` via merge-ort (C1) — the workflow hub; without it the port isn't usable
- [ ] Reflog read/write + `git reflog` (C5) — safety net every mutating command logs to
- [ ] Ref locking/transactions + packed-refs write + `update-ref --stdin` (C6) — sound foundation for all state changes
- [ ] `git config` command with scope resolution (C8) — setup + tool interop
- [ ] `cherry-pick` / `revert` sequencer subset (C3) — history editing alongside merge
- [ ] `git rebase` am + merge backends, non-interactive (C4) — the other half of branch workflows
- [ ] `rm`/`mv`/`clean` completion (B8) + `apply` completion (B10) — file lifecycle + patch path
- [ ] `show`/`describe`/`name-rev`/`shortlog` completion (B9) — daily inspection
- [ ] `rev-parse` completion + `merge-base` remaining flags + `merge-tree`/`cherry` (C2) — scriptability layer
- [ ] Small plumbing batch C7 (pack-refs, mktag, check-ref-format, diff-index/diff-files, show-index, …) — parallel-track wins that close `t/` scripts
- [ ] Commit-graph write (D3) + MIDX completion (D4) + index-pack stdin/thin-pack (D5) — on-disk story finished
- [ ] `gc`/`repack`/`prune` basics (D6) — bounded repos
- [ ] `fsck` completion (C10) — integrity promise kept
- [ ] Depth pass on routed commands (deferred-A8 backlog) running alongside — parity is the moat

### Add After Validation (v1.x — next milestone: transport)

- [ ] pkt-line + protocol v2 → local/file transport → `ls-remote` (E1–E2) — first network-adjacent value, no sockets
- [ ] `fetch`/`push` negotiation + file-local `clone`/`pull` (E3–E4, E8 subset) — trigger: D6 + D1 + D5 green
- [ ] `stash` (needs C1+C5 green) → `worktree` full → `notes` → `bisect` → `grep` → `blame` (C9 order) — trigger: merge + reflog green
- [ ] `git remote` + credential stack (E9) — trigger: fetch/push negotiation green
- [ ] Pack bitmaps (D1) + cruft packs (D2) — trigger: gc/repack green; needed before network negotiation claims performance
- [ ] `bundle`, `fast-export`/`fast-import` (E10) — trigger: transport core green; offline-transfer story

### Future Consideration (v2+)

- [ ] Full ssh/http/daemon transports (E5–E7) — why defer: socket/TLS stacks, credential helpers per platform, largest new-dependency risk; needs plan amendment
- [ ] `verify-commit`/`verify-tag` + signing — why defer: crypto verification stack orthogonal to object parity
- [ ] `submodule` family — why defer: buggiest C corner, huge surface, modest milestone value
- [ ] Interactive (`add -p`, `rebase -i`, pager/color) — why defer: needs pty oracle the differential harness doesn't have
- [ ] Sparse-checkout/sparse-index/split-index — why defer: layers on unfinished index/checkout work; promote to P2 once B2/B4/B7 fully green
- [ ] Email stack (`am`, `format-patch` completion, `send-email`, `mailinfo`), `filter-branch`, `mergetool`/`difftool`, `scalar`, `diagnose`/`bugreport` — why defer: separate products or superseded flows
- [ ] `cvs*`/`svn`/`p4`/GUI/`gitweb` — why defer: explicitly "not planned", ever

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| merge (merge-ort) | HIGH | HIGH | P1 |
| reflog + `git reflog` | HIGH | LOW-MEDIUM | P1 (early — unlocks others) |
| ref locking/transactions + packed-refs write | HIGH | MEDIUM | P1 (early — foundation) |
| `config` command | HIGH | MEDIUM | P1 |
| cherry-pick/revert (sequencer) | HIGH | HIGH | P1 |
| rebase (non-interactive) | HIGH | HIGH | P1 |
| rm/mv/clean completion | MEDIUM-HIGH | LOW-MEDIUM | P1 (cheap, visible) |
| show/describe/name-rev/shortlog | MEDIUM-HIGH | LOW-MEDIUM | P1 (cheap, visible) |
| rev-parse completion | HIGH (scripts) | MEDIUM | P1 |
| small plumbing batch C7 | MEDIUM (each; HIGH collectively) | LOW (each) | P1 (parallel track) |
| apply completion | MEDIUM-HIGH | MEDIUM | P1 |
| commit-graph write | MEDIUM-HIGH | MEDIUM | P1 |
| MIDX completion | MEDIUM | MEDIUM-HIGH | P1 |
| gc/repack/prune basics | HIGH | MEDIUM-HIGH | P1 |
| fsck completion | MEDIUM-HIGH | MEDIUM | P1 |
| routed-command depth (deferred-A8) | HIGH (parity moat) | MEDIUM (cumulative) | P1 (running alongside) |
| stash | HIGH | MEDIUM-HIGH | P2 (needs merge+reflog) |
| worktree full | MEDIUM-HIGH | MEDIUM | P2 |
| grep | MEDIUM-HIGH | MEDIUM | P2 |
| blame | MEDIUM | MEDIUM-HIGH | P2 |
| file-local clone/fetch + ls-remote | HIGH | HIGH | P2 (next milestone head) |
| notes/replace/bisect/range-diff/rerere/archive | LOW-MEDIUM | MEDIUM (each) | P2–P3 (C9 tail order) |
| pack bitmaps / cruft packs | MEDIUM (perf) | MEDIUM-HIGH | P2 (after gc) |
| bundle / fast-export/import | LOW-MEDIUM | MEDIUM | P2 |
| network transports (ssh/http/daemon) | HIGH (full git) | HIGH | P3 (v2+) |
| verify-commit/verify-tag + signing | MEDIUM | HIGH | P3 |
| submodules | MEDIUM | HIGH | P3 |
| interactive (`-p`/`-i`/pager/color) | MEDIUM | HIGH (untestable in harness) | P3 |
| sparse-checkout/index | MEDIUM (monorepos) | MEDIUM-HIGH | P3 (promote after B2/B4/B7) |
| email stack, filter-branch, scalar, gitweb/GUI, bridges | LOW | MEDIUM-HIGH | P3 / never (bridges: never) |

**Priority key:**
- P1: Must have for this ("usable repo") milestone
- P2: Should have, next (transport + C9) milestone
- P3: Nice to have, v2+ or never (see Anti-Features)

## Competitor Feature Analysis

| Feature | gitoxide (`gix`) | libgit2 / git2-rs | Dulwich (Python) | Our Approach |
|---------|------------------|-------------------|------------------|--------------|
| Goal | Library-first pure-Rust git; CLI (`gix`/`ein`) deliberately diverges from C UX | Portable C library for embedding; no CLI parity goal | Pure-Python formats + protocols; thin CLI | **Byte-identical CLI + crosswise on-disk**; library crates as a consequence, not the goal |
| Porcelain breadth | Narrow: `status` only recently started; no full merge/rebase/stash story | No porcelain (API only); consumers (Gitaly) still shell out to C git for missing features (e.g. SHA-256, partial clone per Gitaly #3314) | Common paths (add/commit/status/log); `restore`/`switch` recently added as porcelain functions; checkout historically patchy | All daily porcelain to full option parity, gated by C's own `t/` suite — breadth + depth together |
| Plumbing depth | Good object/db primitives, fast traversals | Good core (odb, refs, diff, merge-bases) but lags C features | Good wire + repo format compat focus | Depth-before-breadth: every routed command to full flag parity (the deferred-A8 list is the quantified gap) |
| Merge/rebase/sequencer | Partial / experimental | Basic merges; complex recursive/ort behaviors lag | Basic; conflict handling simpler than C | Full merge-ort port + sequencer, verified against `t/t6402`–`t/t6430` + `t/t3400`/`t/t3501` families |
| Transport/network | In progress, library-shaped | Partial; Gitaly keeps C git alongside for what libgit2 lacks | Full protocol support (its original use case: hg-git bridging) | Phase E after pack maintenance; file-local first, sockets last |
| Worktree/checkout engine | Partial | Reasonable checkout; edge cases lag | Historically the weakest area (checkout/status semantics) | Full unpack-trees port first (B7 engine), then merge/stash/rebase land on it |
| Verification story | Unit + integration tests; no C-oracle gate | Own test suite; behavior drift vs C accepted | Compat doc (`c-git-compatibility.txt`) listing gaps | Differential byte-identical + crosswise both-directions + committed `t/` scoreboard as merge gate — no competitor does this |
| Performance claim | 2–10× on several ops (parallelism, clean structures) | Fast for embedding; avoids fork/exec | Slow (Python); buys portability, not speed | Match C exactly first; then scoped-thread parallelism where C is serial — performance as differentiator, never at parity's expense |
| Safety story | Memory-safe Rust (shared motivation) | C — same vulnerability class as C git itself | Memory-safe Python (slow) | Pure Rust, near-zero `unsafe` gated by `cargo xtask safety`; MSRV 1.74, no async runtime |
| What they teach us | Library-shaped crates win adoption (`gix-*`); divergence is a choice with costs | Embedding demand is real but partial parity forces dual-stack (C git stays) — our byte-parity avoids the dual-stack trap | Format+protocol compat is achievable purely; checkout/worktree semantics are where ports die — invest in unpack-trees early | Be the port you can *replace* C git with (shim → 100%), not a library that lives beside it |

## Sources

- Repo state (HIGH): `crates/git-command/src/lib.rs` dispatch table (~45 commands), `scripts/shim-git` routed set, `crates/git-command/src/` module list, `crates/scoreboard.json` suite names, `.planning/codebase/{ARCHITECTURE,STRUCTURE}.md` (2026-09-25)
- Plan docs (HIGH): `docs/plan/conversion-plan.md` (Phases A–F + explicit out-of-scope list), `docs/plan/REMAINING_TASKS.md` (DONE/PARTIAL/NOT DONE per B/C/D/E item), `docs/plan/FOLLOWUPS.md` (deferred-A8 list, known deviations), `docs/plan/README.md` (phase map, done-gates), `.planning/PROJECT.md` (constraints: no FFI, no new deps, MSRV 1.74, exit-code parity)
- C oracle surface (HIGH): `builtin/` listing (~150+ builtins incl. helpers) vs 45 routed — the breadth gap is directly enumerable
- Competitors (MEDIUM): gitoxide repo/docs (`gitoxide` — pure-Rust, `gix` CLI, status only recently, 2–10× perf reports, GitMerge 2024 talk), libgit2 project page + Gitaly #3314 (SHA-256/partial-clone gaps), Dulwich docs (`c-git-compatibility.txt`, `dulwich.io` portability claim, thin CLI per discussion #940), git-scm book (plumbing vs porcelain taxonomy; Dulwich/Libgit2 appendix), StackOverflow/Reddit usage-frequency threads (daily-workflow command consensus: clone/status/add/commit/merge/rebase/stash/log/show)
- Git docs/man pages (MEDIUM): `git-read-tree` sparse-checkout note, sparse-checkout performance literature (monorepo motivation for P3 ranking)

---
*Feature research for: pure-Rust byte-compatible git reimplementation (git.rs)*
*Researched: 2026-09-25*
