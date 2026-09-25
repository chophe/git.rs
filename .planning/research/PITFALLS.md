# Pitfalls Research: Pure-Rust Git Reimplementation (git.rs)

**Domain:** Byte-compatible pure-Rust port of C git (remaining builtins milestone)
**Researched:** 2026-09-25
**Confidence:** HIGH (project context + plan docs read; ecosystem cross-checked against libgit2/JGit/gitoxide divergence history and git pack/index/ref format specs)

## Critical Pitfalls

### Pitfall 1: Exit-code and stderr-text drift (the byte-parity killer)

**What goes wrong:**
A ported builtin prints the right answer but with the wrong exit code or a
paraphrased error. C git distinguishes 0 / 1 / 128 / 129 / 141 precisely
(129 = usage, 128 = fatal/die, 1 = "found nothing"/unknown command, 141 =
SIGPIPE), and `t/*.sh` asserts on both code and exact `stderr` wording
including `fatal: `, `error: `, `usage: ` prefixes, quoting style, and even
trailing newlines. A Rust port that uses `anyhow!` / idiomatic messages passes
unit tests and fails the scoreboard.

**Why it happens:**
Rust ergonomics pull toward `Result<T, anyhow::Error>` with free-form
messages. Each command module invents its own wording instead of porting
`usage.c` / `die()` strings verbatim. SIGPIPE (141) is missed entirely because
Rust ignores `SIGPIPE` by default. Ambiguity diagnostics (`rev-parse`,
`commit-tree` arg resolution) are the worst offenders — C has dozens of
near-identical strings that differ by one word.

**How to avoid:**
- Centralize exit codes in one enum (`CommandError` with 0/1/128/129/141
  mapping) and forbid `process::exit(n)` literals in command modules; add a
  `grep` lint for raw exits.
- Port `die`/`usage` strings by copying from C source, not by paraphrase.
  Differential tests must assert `(stdout, stderr, code)` triples, not just
  stdout — already the harness contract (`cargo xtask differential`), keep it
  for every new builtin.
- Handle SIGPIPE explicitly (restore default disposition / map `EPIPE` to 141)
  in `git-cli`; test with `cmd | head -c0`.
- For each new builtin, lift the exact `t/` stderr expectations into the
  crosswise test first (TDD from the oracle), then implement.

**Warning signs:**
- Crosswise test asserts stdout only, or normalizes stderr ("contains"
  instead of "equals").
- New `eprintln!` strings with no C source citation comment.
- Scoreboard delta shows "1 failure, output looks right" — it is this pitfall.

**Phase to address:**
Every builtin-porting phase from now on (Phase B remainder, B8–B10, then merge/
transport/stash/blame builtins). Gate: `t/` scripts for that builtin pass 100%
through `scripts/shim-git`, zero stderr diffs.

---

### Pitfall 2: Racy-clean index — content-compare status that lies under speed

**What goes wrong:**
`status` / `add` / `commit` / `write-tree` disagree with C on whether a file is
dirty. The port compares content hashes every time (correct but slow) or trusts
`stat` blindly (fast but wrong within the racy window: file mtime == index
mtime with changed size/content). C's racy-clean logic (smudge `ce_stat_data`,
`racy` timestamp, empty-file special cases, `core.trustctime`,
`core.checkStat`) is subtle; getting it 90% right yields flaky crosswise
failures that only reproduce on fast filesystems or in CI.

**Why it happens:**
`FOLLOWUPS.md` already notes it: "status content-compares today" (Phase 6
remainder). Developers defer racy handling as "optimization" when it is
correctness — `add_to_index` cache-tree invalidation, `write-tree` freshness,
and `commit` nothing-to-commit detection all depend on it.

**How to avoid:**
- Port `ie_match_stat` / `ce_modified_check_fs` semantics faithfully into
  `git-index`, including nanosecond mtime comparison, ino/dev checks gated by
  config, and the "smudge and re-hash on next read" path — not a simplified
  `mtime != cached` check.
- Reuse one `is_racy_clean(entry, stat)` helper across `status`, `add`,
  `commit`, `update-index --refresh`; never reimplement per command.
- Crosswise tests must include a racy-window generator (write file, `add`, rewrite
  within same mtime tick, then `status`/`commit`) plus `update-index --refresh`
  round-trips validated by C `git status` on the Rust-written index.

**Warning signs:**
- `status` is "always right but slow" (hashes everything) — means racy logic
  was skipped; will break `write-tree` cache-tree writeback and `commit`
  performance gates later.
- Flaky crosswise failures that pass on rerun.
- Separate dirty-check implementations in `status.rs` vs `add.rs`.

**Phase to address:**
Index/worktree phase (Phase 6 remainder: B2 cache-tree, B4 `read-tree -m/-u`,
B7 `unpack-trees`). Must land before `commit`/`checkout`/`stash` scale work.

---

### Pitfall 3: Date/ident/timezone parity failures

**What goes wrong:**
`log --format`, `rev-list --since/--until`, reflog timestamps, and commit
objects diverge: `+0000` vs `-0000` (unknown zone), local-zone vs UTC ident
offsets, DST fold handling, `@{yesterday}` / `2.weeks.ago` calendar math,
`--date=relative` thresholds ("2 hours ago" vs "120 minutes ago"), and raw
`author <a@b> 1234567890 +0200` serialization. C's `date.c` approxidate is a
large state machine; UTC-only shortcuts (a logged FOLLOWUPS deviation, now
marked DONE for A6/A12 — but every *new* date-touching builtin re-opens it)
produce off-by-hours commits C `fsck` still accepts but `t/` rejects.

**Why it happens:**
Rust `chrono`/`time` defaults differ from C `localtime`/`mktime` edge behavior
(negative timestamps, far-future years, `--date=iso-strict`, leap seconds).
Developers test in one TZ (usually UTC CI) and miss local-zone paths.

**How to avoid:**
- Keep the `git-date` crate as the single choke point; forbid direct
  `chrono::Local::now()` in command modules. All idents flow through it.
- Differential matrix for every date feature: `TZ=UTC`, `TZ=America/New_York`
  (DST), `TZ=Pacific/Kiritimati` (+14), plus `TZ=UTC0` vs unset; negative and
  pre-1970 timestamps; `-0000` inputs.
- Byte-compare generated commit objects (`cat-file -p` round-trip through C),
  not just pretty output.

**Warning signs:**
- Date tests only run in CI's default TZ.
- Hand-rolled "format timestamp" helpers outside `git-date`.
- `log` output matches but `fsck` or `verify-pack` on Rust-written commits
  differs in `author`/`committer` lines.

**Phase to address:**
Any phase touching commits/tags/reflogs/log (Phase 4 remainder: pretty,
`--format`, trailers, mailmap; Phase B5 `commit` extensions; future
`stash`/`am`/`format-patch`).

---

### Pitfall 4: Repo-discovery and config-precedence regressions

**What goes wrong:**
A new builtin works in a plain repo but breaks with `--git-dir`, `--work-tree`,
`-C <path>`, `-c key=val`, `GIT_DIR`/`GIT_WORK_TREE`/`GIT_CONFIG_*` env,
bare vs non-bare, `init.defaultBranch`, `--separate-git-dir`, or linked
worktrees. Config layering (system → global → local → worktree → `-c` → env)
picks the wrong value, so behavior silently differs (e.g. `core.ignorecase`,
`core.filemode`, `core.autocrlf`, `core.quotepath` flip diff/status output).

**Why it happens:**
Each command re-derives discovery instead of using the shared `RepoContext`.
FOLLOWUPS A2 fixed this once (`set_current_dir`-free design); new contributors
reintroduce `env::current_dir` / `std::env::set_current_dir` or read config
files directly. C's `setup.c` + `config.c` precedence is ~2000 lines of edge
cases (conditional includes, `includeIf hasconfig:remote.*.url`, worktree
overrides).

**How to avoid:**
- Hard rule: every builtin takes `RepoContext` as its only repo/config entry;
  ban `set_current_dir`, bare `config::load()` calls, and per-command
  `--git-dir` parsing. Enforce with `cargo xtask depcheck`-adjacent grep gate
  or code-review checklist.
- Per-builtin crosswise matrix (already established for B1 `init`): plain,
  `--bare`, `-C subdir`, `--git-dir X --work-tree Y`, `-c` override, `GIT_DIR`
  env. Copy the B1 matrix as the template for all future builtins.
- Port conditional-include evaluation once in `git-config`; never approximate
  with "read .git/config only".

**Warning signs:**
- Command works from repo root, fails from subdir (`did not match any file` /
  `not a git repository` diffs vs C).
- Tests never set `GIT_DIR` / `-C` / `-c`.
- Two commands disagree on the same config key.

**Phase to address:**
Ongoing — every new builtin phase. Cheapest to enforce at the composition-root
(`git-command::dispatch` + `RepoContext`) before the next milestone starts.

---

### Pitfall 5: Pathspec / ignore / attributes semantics approximated

**What goes wrong:**
`add`, `status`, `checkout`, `reset`, `ls-files`, `diff` diverge on pathspecs:
glob `*.rs` vs `**.rs`, trailing-slash dir semantics, `:(exclude)`, `:(icase)`,
`:(top,glob)`, `--` disambiguation, unborn-HEAD paths, subdir-relative display,
`core.quotepath` octal quoting of non-ASCII, and `.gitignore` negation +
directory-pruning interactions. The port "mostly works" on ASCII repos and
fails `t2200-t2299` (pathspec), `t0008` (ignore), `t0027` (attr) families.

**Why it happens:**
Developers reach for the `glob` crate or regex instead of porting
`wildmatch` + `pathspec.c` + `dir.c` semantics. Ignore handling gets
reimplemented per command (FOLLOWUPS explicitly warns `status --ignored` and
`ls-files --others` must reuse the `git-attributes` `IgnoreEngine`, not fork
it). Case-insensitivity (`core.ignorecase`) and symlink-vs-dir conflicts are
deferred as "Windows/macOS only".

**How to avoid:**
- Single `git-pathspec` + `git-attributes/IgnoreEngine` used by all commands;
  forbid ad-hoc glob matching (lint for `glob::` / manual `fnmatch` outside
  those crates).
- Gate each pathspec-touching builtin on its `t/` family through the shim
  (pathspec: `t6130`, `tblame` not needed; ignore: `t0008`; attr: `t0027`).
- Include non-ASCII filenames, quoted-path output (`core.quotepath=true/false`),
  and case-collision fixtures in every crosswise suite.

**Warning signs:**
- New `fn matches(path)` helper inside a command module.
- Ignore tested only with `*.log`, never with `!keep.log`, `dir/`, `a/**/b`.
- Tests run only on Linux with ASCII paths.

**Phase to address:**
Pathspec/ignore phase (Phase 6 remainder + Phase B3 `add` extensions:
`-p/-i`, `--chmod`, pathspec magic; B6 `status` pathspec limiting; future
`checkout`/`reset` pathspec work).

---

### Pitfall 6: Packs C git rejects — thin packs, delta chains, idx fanout

**What goes wrong:**
Rust-written packs verify with the Rust reader but fail C
`verify-pack` / `index-pack --verify` / `fsck` / `clone`: thin packs shipped
without bases, OFS_DELTA offsets off by the header length, delta base-size
headers mismatching, non-oid-sorted idx entries breaking the v2 fanout table,
missing trailing pack checksum, or delta chains deeper than C's default that
explode on clone. FOLLOWUPS A13 (deltified `pack-objects`) is DONE, but every
new pack-writing path (`repack`, `gc`, `commit-graph write` touching
reachability, future `fetch`/`push`) can reintroduce it.

**Why it happens:**
Pack format docs are terse; the killer details live in C code, not the spec:
OFS_DELTA offset is measured from the *type byte* of the delta entry (not the
data start) with the `2^7+2^14+…` bias; idx v2 requires oid-sorted entries for
fanout correctness while the pack itself stays in delta-friendly order; thin
packs must be fixed (`--fix-thin`) before storing; `verify-pack -v` ordering
assumptions break silently.

**How to avoid:**
- Never write a pack path without the crosswise seal: C `verify-pack`,
  C `index-pack --verify`, C `fsck`, and C `clone`/`fetch` from the Rust pack
  (the A13 `phaseA13_crosswise.rs` pattern). Bidirectional: Rust must also read
  C packs with deep chains, REF_DELTA, and v3 packs.
- Keep one `write_pack_opts` choke point (window/depth/compression, OFS vs REF
  default matching C `repack`/`gc`); no per-command pack writers.
- Proptest pack round-trips (parse → serialize → parse) plus fuzz targets for
  pack/idx/midx parsers (still NOT DONE per FOLLOWUPS B4 — schedule before
  `repack`/`gc`).

**Warning signs:**
- Pack tests assert "Rust can read its own output" without C verification.
- New flag changes delta defaults (`--no-reuse-delta` semantics guessed).
- idx entries emitted in pack order instead of oid-sorted order.

**Phase to address:**
Object-store phases (Phase 2/3 remainder: `repack`/`gc`, MIDX `RIDX`/`BTMP`/
`BASE`, bitmaps, cruft packs; Phase 9 `index-pack --stdin`). Fuzz targets
(FOLLOWUPS B4) are a prerequisite, not a follow-up.

---

### Pitfall 7: Refs without transactions — lost updates, torn packed-refs, missing reflogs

**What goes wrong:**
`branch`/`tag`/`update-ref`/`commit`/`reset`/`stash` lose concurrent updates,
corrupt `packed-refs` (torn write visible to readers), skip `logs/<ref>`
reflog entries C would write, mishandle symrefs (`HEAD` detached vs symbolic),
or accept invalid refnames C rejects (`HEAD..x`, `refs/heads/.lock`,
double-dots, unicode). Works single-threaded; fails `t1400-t1430` (update-ref,
transactions, symrefs) and real-world concurrent writers.

**Why it happens:**
C's `files-backend.c` locking (`lock_ref` → `.lock` file → atomic rename,
multi-ref transactions with `--stdin`, packed-refs snapshot isolation) is
dismissed as "just file IO". The port writes refs directly. Reftable is
currently config-only stub (B1 gap) so the files-backend path must be exactly
right first; reflog options (`--no-deref`, `--log`, `--stdin -z`) are deferred
and then forgotten.

**How to avoid:**
- Port the lock-file + transaction protocol once in `git-refs` (acquire `.lock`,
  verify old-oid under lock, write, fsync, atomic rename; multi-ref all-or-none
  with proper `--stdin` dialect). All commands go through it — no direct
  `fs::write` of ref files anywhere (grep-gate this).
- Crosswise: concurrent `update-ref` + `commit` interleavings; torn-read check
  (reader during `pack-refs` rewrite must see old or new, never partial);
  reflog byte-compare (`git log -g` through C on Rust-written reflogs).
- Refname validation via ported `check_refname_component` rules + proptest
  against C `check-ref-format`.

**Warning signs:**
- `fs::write(ref_path)` outside `git-refs`.
- Tests never exercise `update-ref --stdin`, `-d` with non-zero old value, or
  deleting the checked-out branch (already fixed once for `branch -d` — keep
  the regression test).
- Reflog assertions missing ("refs updated but `log -g` empty").

**Phase to address:**
Refs phase (Phase 7 remainder: reflog, packed-refs writing, transactions/
locking, symref writes, `update-ref --stdin`, reftable backend). Blocks `stash`,
`worktree`, and any concurrent-safe `commit`/`reset`.

---

### Pitfall 8: Revision-walk ordering and reachability shortcuts

**What goes wrong:**
`log`, `rev-list`, `merge-base`, `fetch` negotiation, `gc` reachability, and
`fsck` connectivity checks return the right *set* of commits in the wrong
*order*, or miss/wrongly-include boundary commits: `--topo-order` vs
`--date-order` vs default, `--first-parent`, `--all`/`--branches`/`--tags`/
`--reflog`, grafts/replace objects, shallow boundaries, `--objects`/`--count`/
`-n` limits, `A..B`/`A...B`/`A^!` syntax. `merge-base --is-ancestor` direction
was already fixed once (A8) — the same direction bug recurs in every new walk.

**Why it happens:**
Naive BFS/DFS "looks sorted" on linear history; C's `revision.c` priority-queue
with date-order tiebreaks, topo-order generation numbers (commit-graph!), and
`UNINTERESTING` boundary propagation only diverge on merges, clock skew, and
grafts. Commit-graph-driven walks and bloom queries are still unimplemented
(Phase 3/4 remainder), so performance pressure tempts walk duplication per
command.

**How to avoid:**
- One `RevWalk` in `git-revision` with explicit ordering modes and boundary
  flags; commands pass options, never hand-roll traversal. Add commit-graph
  generation-number support before optimizing (correctness first, then speed).
- Differential fixtures must include: criss-cross merges, clock-skewed parents
  (child older than parent), grafted/replaced commits, shallow repos,
  `--first-parent` on octopus merges.
- `fsck --connectivity-only` and `count-objects` reachability must share the
  same walk — divergence here corrupts `gc`.

**Warning signs:**
- `sort_by(commit_time)` in a command module instead of using `RevWalk` flags.
- Tests only cover linear history.
- `--topo-order`/`--date-order` accepted as no-op flags.

**Phase to address:**
Revision phase (Phase 4 remainder: ordering, path limiting, grafts/replace,
`--all`, commit-graph walks; Phase 8 remainder: octopus/independent ancestor
checks). Required before `gc`/`fetch`/`push` (Phase 10+).

---

### Pitfall 9: Merge/diff rename-detection divergence (silent wrong results)

**What goes wrong:**
`merge`, `cherry-pick`, `revert`, `stash`, `status -M`, and `diff -M` produce
*plausible but wrong* results: renames missed or over-detected, criss-cross
histories merged with the wrong base set, adjacent-change conflict clustering
differing from `xdl_merge`, `merge-tree` output mismatching C on directory/
file (D/F) conflicts. FOLLOWUPS logs the seed: "exact rename scoring is a
line-similarity approximation vs C's diffcore-rename". Every merge built on
that approximation inherits the error, and exit codes (`--exit-code`/`--quiet`
already flagged in Phase 5 remainder) mask it.

**Why it happens:**
C's `diffcore-rename` (similarity index with basename引导, break detection,
copy detection) + `merge-ort` (rename-aware, D/F conflict rules, recursive
criss-cross via `merge-base --all`) is a multi-thousand-line subsystem.
Ports implement "similarity = jaccard(lines)" and declare victory when small
fixtures pass. `unpack-trees` D/F rules (needed for B4 `read-tree -m`, B7
checkout/reset `-m`) are documented as the dependency and then skipped.

**How to avoid:**
- Treat rename scoring as spec, not heuristic: port `diffcore-rename`
  thresholds (`-M50%` default, `-C`, `--find-copies-harder`), exact-blob fast
  path, and break/rewrite detection — behind the `git-diff` crate, shared by
  `diff`, `status`, and `merge`.
- `merge-ort` port order: `unpack-trees` D/F + sparse rules first (unblocks B4/B7
  deferred items), then base selection (`merge-base --all` recursive), then
  content merge with C-identical conflict markers (`--marker-size`, `--diff3`,
  `-L` deferred but must not change default output).
- Crosswise on conflict-heavy corpora (git's own `t6400-t6499` merge fixtures),
  asserting conflicted file bytes + index stages (1/2/3) + exit code.

**Warning signs:**
- "Similarity" computed differently in `diff.rs` vs `status.rs` vs `merge.rs`.
- Merge tests with no renames, no D/F conflicts, no criss-cross.
- `diff-tree --exit-code` still always 0 (known Phase 5 gap — fix before merge).

**Phase to address:**
Diff/merge phases (Phase 5 remainder: rename/copy, break/order; Phase 8
remainder: `merge-ort`, `merge-tree`, `cherry-pick`/`revert`; B7 `-m` merge
checkout). Do `unpack-trees` before any new merge builtin.

---

### Pitfall 10: Object-format and backend stubs that become permanent (`sha256`, reftable, index v3/v4, split/sparse)

**What goes wrong:**
`--object-format=sha256` and `--ref-format=reftable` are accepted as
config-only no-ops (B1 known gaps); index v3/v4, split index, sparse index,
and REUC extensions are rejected or silently misread; `compatObjectFormat`
(`sha1↔sha256` conversion, `gpgsig↔gpgsig-sha256` rewriting, LMAP) never lands
(Phase 9 gate `t1016`). The port works on maintainer laptops (SHA-1, files
backend, index v2) and fails on repos C creates by default in other
configurations — a compatibility time bomb.

**Why it happens:**
Each gap is individually reasonable ("SHA-1 first", "reftable later"), but
without a forcing function they fossilize. Readers that `bail!` on unknown
extensions/chunks (commit-graph chains, MIDX `RIDX`/`BTMP`/`BASE`, index
extensions) turn future C repos unreadable instead of forward-compatible.

**How to avoid:**
- Policy: readers must be liberal (skip-and-preserve unknown chunks/extensions
  they don't need, per C's forward-compat rule), writers must be conservative
  (emit exactly what the configured format demands). Reject unknown *required*
  chunks explicitly (as now for chains) — but log them in FOLLOWUPS, don't
  silently ignore.
- Round-trip rule: any index/graph/midx the Rust port reads, it must rewrite
  byte-identically (or documented-equivalent) so C accepts it — test both
  directions for v2/v3/v4, split, sparse, REUC once implemented.
- Schedule `t1016-compatObjectFormat` and reftable `t1400+t1460` families as
  explicit milestone gates; keep the B1 "config only" stubs visibly marked
  until replaced.

**Warning signs:**
- `bail!("unsupported extension")` on read paths C tolerates.
- New writer emits v2 unconditionally while claiming sha256/reftable support.
- "Config only" comments older than one milestone with no tracking test.

**Phase to address:**
Format-parity track cutting across Phase 3 (commit-graph chains, MIDX optional
chunks, bitmaps), Phase 6 (index v3/v4, split/sparse, REUC), Phase 7
(reftable), Phase 9 (`compatObjectFormat`, LMAP). Assign one phase per format;
don't batch them.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Paraphrased error strings ("looks close enough") | Faster porting per builtin | Permanent `t/` failures; every message becomes a Hunt-the-diff debugging session | Never — copy C strings verbatim |
| stdout-only differential tests | Green CI quickly | Exit-code/stderr drift ships silently; scoreboard catches it late at high triage cost | Never for builtins; unit helpers only |
| Content-hash-everything `status` (skip racy logic) | Correct on clean checkouts | 10–100× slowdown on large repos; breaks `write-tree`/`commit` perf gates; racy bugs surface later | Only as a stepping stone with a tracking test that fails on perf gate |
| Per-command pathspec/glob helpers | No need to finish `git-pathspec` | N divergent semantics; `t6130`/`t0008` fail per command instead of once | Never — finish the shared crate first |
| Line-similarity rename approximation | `diff -M` demos pass | All merges built on it are subtly wrong (Pitfall 9); re-tuning later changes historical outputs | Only behind an explicit `--experimental` flag with FOLLOWUPS entry |
| Non-deltified / REF-only pack writer | Simple writer, passes self-read | Bloated packs; C `gc`/`clone` interop surprises; re-tuning deltas later invalidates golden fixtures | Acceptable for first `pack-objects` cut (done); never for `repack`/`gc` |
| Direct `fs::write` of refs/index instead of lock+rename | Less code | Torn reads, lost concurrent updates, corrupted `packed-refs` | Never |
| `bail!` on unknown index/graph/midx extensions | Avoids forward-compat work | Unreadable future repos; crosswise breakage on C upgrades | Only for truly *required* chunks (chains today), with tracking issue |
| `chrono`/`glob`/`clap`/`serde` convenience deps | Familiar APIs | Violates locked dependency constraints (`flate2` via `git-compress` only; no clap/serde/tokio); MSRV/unsafe drift | Never without plan amendment |
| Skipping fuzz/proptest/coverage gates "for velocity" | Faster merges | Parser panics on hostile input; acceptance criteria (README §done-gates) become fiction | Never — gates are the parity contract |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| C `t/` suite via `scripts/shim-git` | Forgetting to add the new builtin to the shim `case` list + `dispatch`, so `t/` silently tests C instead of Rust | Checklist per builtin: shim entry → dispatch arm → crosswise suite registration in `xtask::suites()` → `t/` family run |
| `crates/scoreboard.json` baseline | Hand-editing the baseline or regenerating to "make red green" | `cargo xtask scoreboard` regenerates and *fails on regression*; commit baseline updates only with intentional behavior-change justification |
| System C git as oracle (`/usr/bin/git`) | Assuming the local git version matches `v2.55.0-540` behaviors under test | Pin oracle version in CI; note version-skew in crosswise failures; re-verify `gen-fixtures` checksums after toolchain bumps |
| `cargo xtask gen-fixtures` golden repos | Regenerating fixtures casually, masking writer drift | Treat `crates/tests/fixtures/.checksums` as golden; regeneration requires review like the scoreboard |
| `git-test` helper crate (Rust `test-tool` replacement) | Reimplementing `test-tool` subcommands ad hoc per test | Finish the planned `git-test` subcommand list (FOLLOWUPS B3) once; `t/` scripts depend on exact `test-tool` output too |
| Editor/pager/hook invocation (`commit`, future `am`/`rebase`) | Shelling out with inherited env, ignoring `GIT_EDITOR`/`core.editor` precedence and hook exit codes | Port `editor.c`/`run-command.c` precedence once; hooks opt-in per command with C-identical `--no-verify` semantics; deferred hooks must be documented gaps, not silent skips |
| GPG/SSH signing (`commit -S`, future `tag -s`) | Skipping signature plumbing ("out of scope") while emitting commit objects C would sign differently | Explicitly defer with `unknown-option`/documented gap; never emit fake `gpgsig` headers (breaks `fsck` + `compatObjectFormat`) |
| Submodules / worktrees / sparse-checkout | Treating them as "later" while `add`/`checkout`/`status` recurse into them with wrong defaults | Gate recursion behavior per command now (match C defaults: e.g. `status` submodule summary, `checkout` submodule non-recursion) even before full submodule support |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Full-content `status` (no racy/stat cache, no untracked-cache, no fsmonitor) | `status` 10–60s on linux.git-scale repos; `t/` timeouts | Port racy-clean + cache-tree + untracked-cache incrementally; share `stat` across index/worktree/diff passes | ~10k files / monorepo scale; CI timeout |
| Whole-pack inflate / full delta-chain replay per object | `cat-file`/clone latency spikes; memory blowup on deep chains | Delta-base cache with size cap (port C's `delta_base_cache` limit + `retain_data` refcount semantics); streaming inflate; verify depth cap matches C | DeepИменно chains (C default depth 50); repos with large blobs |
| Loading entire idx/midx/bitmap/commit-graph into `Vec` eagerly | RSS spikes on repos with 100+ packs / million-commit graphs | `mmap`-style lazy chunk access (or bounded read windows honoring the no-async-runtime constraint); parse chunk table first, payloads on demand | Chromium / kernel / git.git itself at scale |
| Rev-walk without commit-graph generation numbers | `log --topo-order`, `merge-base --all`, `gc` take minutes | Implement generation numbers + bloom reachability (Phase 3/4) before optimizing walks; gate with `t6600` perf-adjacent suites | ~100k commits |
| `pack-objects` window search O(n·w) without threading/memory caps | `pack-objects --window=250` OOMs or takes 10× C time | Port `--window/--depth/--window-memory` caps faithfully; reuse-delta fast path (`--no-reuse-delta` only when asked); single-threaded but bounded (no tokio per constraints) | Large binary repos |
| Re-hashing every file on `add -A` (no `stat` short-circuit, no parallel walk) | `add` slower than C by 5–20× on warm tree | `stat`-valid fast path + racy smudge discipline; directory walk with early ignore-pruning (don't `stat` ignored dirs) | ~50k files |
| String-per-path allocations in diff/patch/status hot loops | Excess allocs show in profiles before any algorithmic issue | `StringBuf`-style reuse / borrowed paths in `git-diff` renderers; profile with a 100k-file fixture before optimizing algorithms | Large-tree `diff --stat` / `status --porcelain` |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Path traversal in `checkout` / `reset --hard` / `read-tree -u` / `apply` (malicious `../`, absolute paths, symlink races) | Arbitrary file overwrite outside worktree (classic git CVE class) | Port `verify_path` / `unpack-trees` path checks verbatim; lstat-then-create with `O_NOFOLLOW` discipline; crosswise-test with hostile tree fixtures (include `..`, `.git/`, symlinks, case-collisions) |
| Symlink-following `stat` in index/worktree code | TOCTOU: status/add sees through attacker-controlled links | `symlink_metadata` everywhere on worktree walks; never `follow` during ignore/pathspec evaluation |
| Lenient `fsck` (accept what C `--strict` rejects) | Corrupt/malicious objects enter the store; later `clone`/`fetch` propagates them | `fsck` message-catalog parity (Phase 9 remainder: `--strict`, `--full`, `--lost-found`); fuzz pack/loose/commit parsers; `index-pack --verify` on untrusted input before use |
| zlib bombs / delta bombs (tiny pack expands to GBs) | OOM / disk exhaustion on `index-pack`, `clone`, `fetch` | Enforce C's size caps (`core.bigFileThreshold`-adjacent limits, delta output bounded by declared result size as in `patch_delta`'s `size` accounting); stream with caps, never `xmalloc(untrusted_len)` without bound |
| Hook execution without opt-in parity (`commit` hooks, future `am`/`rebase`) | Skip hooks user expects (policy bypass) or run hooks C wouldn't (RCE surprise) | Match C hook gating exactly (`.git/hooks/*.sample` not executable by default; `--no-verify` skips the same set); never auto-execute new hook points |
| Credential/config injection via `-c` / `GIT_CONFIG_*` / includes | Repo-local config executing `core.fsmonitor` / `core.sshCommand` / `insteadOf` unexpectedly in tests | Tests must scrub env (`GIT_CONFIG_NOSYSTEM`, isolated `HOME`) like `t/` does; document which config keys the port honors vs ignores |

## UX Pitfalls (CLI-parity edition — this port's UX *is* C-compatibility)

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| "Helpful" reworded errors / colored output by default | Scripts grepping `fatal:` break; `t/` fails; piped output differs | Byte-identical pager/color default (no color on non-tty, 80-col stat like C non-tty); copy message catalogs |
| Abbrev hashes of different length (`--short` defaults) | Copy-pasted oids don't match C; `t0019`-style tests fail | Port `core.abbrev` auto-scaling (7-char on small repos only as C computes); same uniqueness-expansion loop |
| `--quiet`/`--exit-code`/`--porcelain` machine flags treated as cosmetic | Automation misreads results (`diff --exit-code` always 0 masks dirty trees) | Machine flags first-class: exit-code parity tested explicitly per command |
| Progress/verbose output on stderr in scripts | `add -v`, `commit --dry-run --porcelain` (deferred) diverge when finally added | Port `--dry-run`/`--porcelain`/`-z` NUL-delimited variants with byte tests from day one of each builtin |
| Locale/encoding assumptions (assume UTF-8, ignore `core.quotepath`, `i18n.*`) | Non-ASCII paths/log messages garble vs C's octal quoting | Respect `core.quotepath` + octal quoting; test with non-UTF8-tolerant byte fixtures |

## "Looks Done But Isn't" Checklist

- [ ] **New builtin ported:** Often missing exact `stderr` + exit-code parity — verify `(stdout, stderr, code)` triple vs system git on success *and* all error paths, plus `usage` (129) paths.
- [ ] **`add`/`status` green on clean tree:** Often missing racy-window + `core.ignorecase/filemode` cases — verify rewrite-in-same-tick, mode-only change, and `'A' vs 'a'` collision fixtures.
- [ ] **Pack writer "works":** Often missing C-side acceptance — verify C `verify-pack`, C `index-pack --verify`, C `fsck`, and C `clone` of Rust output (not just Rust self-read).
- [ ] **Refs updated:** Often missing reflog entries + `packed-refs` atomicity — verify `git log -g` through C and concurrent-update interleavings.
- [ ] **`log`/`rev-list` "ordered":** Often missing `--topo/--date-order`, `--first-parent`, grafts/replace, shallow — verify on merge-heavy + clock-skewed fixtures, not linear history.
- [ ] **Merge/diff "handles renames":** Often missing C's rename scoring + D/F conflicts — verify against `t6400-t6499` conflict fixtures including stages 1/2/3.
- [ ] **Index written:** Often missing C acceptance both directions — verify C `read` of Rust index *and* Rust `read` of C index for v2 (+v3/v4/split/sparse once claimed), with extensions preserved.
- [ ] **Date output matches:** Often missing TZ matrix — verify under at least 3 `TZ` settings including a DST zone and `+14`.
- [ ] **Works from subdir with `-C`/`--git-dir`:** Often missing discovery precedence — verify the B1-style matrix (bare, `-C`, `--git-dir/--work-tree`, `-c`, `GIT_DIR` env) per builtin.
- [ ] **Quoting/encoding:** Often missing `core.quotepath` + non-ASCII — verify octal-quoted vs literal path output both ways.
- [ ] **Shim + dispatch wired:** Often missing registration — verify `scripts/shim-git` case entry, `git-command::dispatch` arm, `xtask::suites()` entry, and the `t/` family actually exercising the Rust binary (not silently falling through to system git).
- [ ] **Gates green:** Often missing the non-obvious gates — verify proptests, fuzz budget (once B4 lands), `cargo llvm-cov ≥90%` on touched crates, and zero `scoreboard.json` drift.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Exit-code/stderr drift shipped | LOW (per builtin) | Diff `t/` output, copy C strings verbatim into a message-catalog module, extend crosswise triple-assertions; no architecture change |
| Racy-clean wrongness | MEDIUM | Centralize `is_racy_clean` in `git-index`, add racy-window fixtures, re-verify `add/commit/write-tree/status` together (they share the helper) |
| Date/TZ divergence | LOW–MEDIUM | Funnel through `git-date`, add TZ matrix to CI, regenerate affected golden commits |
| Config/discovery bypass | MEDIUM | Refactor commands to `RepoContext`-only; add B1-matrix tests per builtin; grep-ban direct env/dir/config access |
| Pathspec/ignore fork | MEDIUM | Delete per-command matchers, migrate to `git-pathspec`/`IgnoreEngine`, re-run `t6130/t0008/t0027` families |
| Bad packs in the wild | HIGH | Never migrate bad packs — rewrite with fixed writer (`repack -a -d` equivalent), re-verify with C trio (`verify-pack`/`fsck`/`clone`); invalidate affected golden fixtures explicitly |
| Torn refs / lost updates | HIGH | Audit for direct ref writes, port lock+transaction protocol, add concurrency tests; repair repos with C `pack-refs` + reflog replay |
| Walk-ordering bugs | MEDIUM | Replace hand-rolled sorts with `RevWalk` flags, add merge/skew/graft fixtures; no data migration needed (read-path only) |
| Rename/merge approximation baked in | HIGH | Re-port `diffcore-rename` scoring + `unpack-trees` D/F rules; expect historical merge outputs to change — gate behind fixture regeneration review |
| Format-stub fossilization (sha256/reftable/v3/v4) | HIGH | One format per phase with `t1016/t14xx` gates; readers-liberal/writers-conservative retrofit; may require index/graph rewrite tooling |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1 Exit-code/stderr drift | Every builtin phase (B8–B10 onward); enforce in dispatch review | `t/` family 100% via shim; crosswise triple-assert (stdout/stderr/code) |
| 2 Racy-clean index | Phase 6 remainder (B2/B4/B7: cache-tree, unpack-trees, refresh) | Racy-window fixtures; C `status` on Rust index; perf gate (no full-hash fallback) |
| 3 Date/ident/TZ | Phase 4 remainder (pretty/format/trailers) + commit/tag phases | TZ matrix (UTC/DST/+14); commit-object byte-compare via C `cat-file` |
| 4 Discovery/config precedence | Composition-root hardening now; every builtin phase | B1-style matrix per builtin; grep-ban on direct env/config access |
| 5 Pathspec/ignore/attr | Phase 6 remainder + B3/B6 extensions | `t6130`/`t0008`/`t0027` via shim; non-ASCII + quotepath fixtures |
| 6 Pack acceptability | Phase 2/3 remainder (`repack`/`gc`, MIDX chunks, bitmaps) + fuzz prereq (FOLLOWUPS B4) | C `verify-pack` + `index-pack --verify` + `fsck` + `clone` on Rust packs, both directions |
| 7 Ref transactions/locking/reflog | Phase 7 remainder (reflog, packed-refs write, locking, reftable) | Concurrent-update tests; C `log -g` on Rust reflogs; `t1400-t1430` |
| 8 Walk ordering/reachability | Phase 4 remainder + Phase 8 (`--all`, grafts/replace, shallow) | Merge/skew/graft/shallow fixtures; `t6000-t6600` families |
| 9 Rename/merge divergence | Phase 5 remainder (rename/copy) → Phase 8 (`merge-ort`, unpack-trees first) | `t6400-t6499` conflict corpora; stages 1/2/3 + exit-code asserts |
| 10 Format stubs (sha256/reftable/v3/v4/split/sparse/LMAP) | Dedicated track: Ph 3 (graph/MIDX chunks) / Ph 6 (index variants) / Ph 7 (reftable) / Ph 9 (`compatObjectFormat`+`t1016`) | Bidirectional round-trip suites; readers-liberal/writers-conservative audit |

## Sources

- Repo oracle: `docs/plan/README.md` (shared done-gates), `docs/plan/FOLLOWUPS.md`
  (§A–E: A13 deltified packs, racy/cache-tree gaps, reftable/sha256 stubs, missing
  fuzz/coverage/`git-test` infra), `docs/plan/phase-3/4/5/6/7/8/9-summary.md` remainders.
- Git formats: `Documentation/technical/pack-format.txt` (OFS_DELTA offset bias,
  idx fanout ordering, thin-pack self-containment rule), `patch-delta.c`
  (`cp_size==0 → 0x10000`, opcode-0 rejection, size accounting).
- Reimplementation divergence history: libgit2 `docs/differences-from-git.md` +
  changelog (worktree refs, junction-vs-symlink, `.gitattributes` size caps,
  trailing-slash path reporting, "git change" lag tracker); JGit/Eclipse and
  gitoxide issue trackers (rename scoring, date/TZ, shallow/replace coverage
  gaps — same classes recurring across all three projects).
- C behavior sources consulted via repo tree: `builtin/index-pack.c` (delta-base
  cache + `retain_data`), `diff-delta.c`/`patch-delta.c`, `date.c` approxidate,
  `dir.c`/`pathspec.c`/`wildmatch`, `files-backend.c` locking, `revision.c`
  ordering, `merge-ort`/`unpack-trees`, `fsck` message catalog.

---
*Pitfalls research for: pure-Rust git reimplementation (git.rs), remaining-builtins milestone*
*Researched: 2026-09-25*
