# Requirements: git.rs

**Defined:** 2026-09-26
**Core Value:** Byte-identical behavior with C git — same stdout/stderr/exit codes and crosswise-readable on-disk formats — verified by the `t/` oracle suite and crosswise tests.

## v1 Requirements

### Refs & Config Foundation

- [ ] **REFS-01**: User gets reflog safety net — `git reflog` reads `logs/<ref>` and every mutating command logs (gate: `t/t1410`)
- [ ] **REFS-02**: Concurrent ref updates stay safe via lock files + atomic rename + multi-ref transactions, including packed-refs write and `update-ref --stdin` (gate: `t/t1400`, `t/t3210`)
- [ ] **CONF-01**: User manages setup via `git config` command with system/global/local/worktree scopes, includes, and `--list/--get/--unset` (gate: `t/t1300`)

### Scriptability

- [ ] **SCRIPT-01**: Scripts rely on full `rev-parse` (`@{...}`, `A..B`/`A...B`, `--all`, `--is-bare-repository`, `--sq`) (gate: `t/t1500`–`t/t1503`)
- [ ] **SCRIPT-02**: Merge-adjacent plumbing works — `merge-base --octopus/--independent`, `merge-tree`, `cherry` (gate: `t/t6010`, `t/t6602`)
- [ ] **SCRIPT-03**: Small independent plumbing batch lands (`pack-refs`, `mktag`, `check-ref-format`, `stripspace`, `var`, `patch-id`, `check-mailmap`, `interpret-trailers`, `show-index`, `diff-files`, `diff-index`)

### History Inspection

- [ ] **INSP-01**: User inspects history daily via completed `show` plus `describe`/`name-rev`/`shortlog` (gate: `t/t4000`, `t/t4201`, `t/t6120`)
- [ ] **INSP-02**: User searches and blames with parity — `grep` over worktree/index/history and `blame --porcelain` output stability (gate: `t/t7810` family)

### File Lifecycle

- [ ] **LIFE-01**: User manages file lifecycle via completed `rm`/`mv`/`clean` (gate: `t/t3600`, `t/t7001`, `t/t7300`)
- [ ] **LIFE-02**: Patch workflows complete — `apply --3way/--index/--reject`, binary patches, whitespace handling (gate: `t/t4103`–`t/t4137`)
- [ ] **LIFE-03**: Three-way `read-tree -m/-u/--prefix` plus `write-tree` edge cases work as the engine under checkout/merge/stash (gate: `t/t1000`, `t/t2000`)
- [ ] **LIFE-04**: Repos written by C git 2.38+ open correctly — index v3/v4 read/write plus REUC extension (gate: `t/t0060`, `t/t3007`)

### Merge Core

- [ ] **MERGE-01**: User completes branch workflows via `git merge` on the merge-ort backend with rename detection and conflict clustering parity (gate: `t/t6402`–`t/t6430`)

### Sequencer Workflows

- [ ] **SEQ-01**: User edits history via `cherry-pick`/`revert` on a shared sequencer engine with `--continue/--abort` resume (gate: `t/t3501`–`t/t3510`)
- [ ] **SEQ-02**: User rebases via `git rebase` (am + merge backends, non-interactive; interactive deferred) (gate: `t/t3400` family)
- [ ] **SEQ-03**: User stashes interrupt-driven work via `stash` push/pop/apply/list/drop over `refs/stash` (gate: `t/t7500` family)
- [ ] **SEQ-04**: User works in multiple checkouts via completed `worktree` add/list/lock/repair with `commondir` layout

### Store Maintenance

- [ ] **STORE-01**: Repos the port writes stay acceleratable — commit-graph write plus chains and bloom queries (gate: `t/t5318`, `t/t5324`)
- [ ] **STORE-02**: Large repos stay readable — MIDX completion (`RIDX`/`BTMP`/`BASE`, incremental, `--preferred-pack`) (gate: `t/t5319`, `t/t5334`)
- [ ] **STORE-03**: Fetched/cloned packs ingest via `index-pack --stdin` with thin-pack base resolution (gate: `t/t5302`)
- [ ] **STORE-04**: Repos stay bounded via `gc`/`repack`/`prune` basics (gate: `t/t7700`–`t/t7704`, `t/t5304`)

### Transport Head

- [ ] **TRAN-01**: User clones and inspects locally via file/local transport (`clone`/`fetch`/`push`/`pull` file-local plus `ls-remote`, no sockets)

### Integrity Full

- [ ] **INTG-01**: User trusts repo integrity via full `fsck` (`--strict`, `--connectivity-only`, `--no-dangling`, `--full`, `--lost-found`, message catalog) (gate: `t/t1450`)

### Depth Parity

- [ ] **DEPTH-01**: Ported commands match C option-for-option — `--word-diff`, `--histogram/--patience`, `-S/-G` pickaxe, `--dirstat`, whitespace family (`-w/-b`), `--relative`

### Network Transport

- [ ] **TRAN-02**: User clones/fetches/pushes over the network — ssh/http transports plus daemon, protocol v2 negotiation, and credentials (conversion-plan Phase E order; sync-first stack, plan amendment during planning if a dependency is needed)

### Email & Patch Exchange

- [ ] **MAIL-01**: User exchanges patches via `am`, `format-patch`, `mailinfo`/`mailsplit`
- [ ] **MAIL-02**: User sends patches via `send-email` plus `imap-send`

### Submodules

- [ ] **SUBM-01**: User works with nested repos via the full `submodule` family (add/update/init/foreach/sync)

### Signing & Verification

- [ ] **SIGN-01**: Supply-chain trust via `verify-commit`/`verify-tag` plus signing in `commit`/`tag` (`gpgsig` headers already preserved byte-exactly for hash correctness)

### Interactive

- [ ] **INTER-01**: User works interactively via `add -p`, `rebase -i`, `stash -p` plus pager/color handling (tested with a pty oracle since the differential harness cannot cover terminal interaction)

### Filters & Platform

- [ ] **FILT-01**: Checkouts convert correctly via smudge/clean filter drivers plus the full `core.autocrlf` matrix
- [ ] **FILT-02**: Monorepos scale via sparse-checkout/sparse-index/split-index

### Remaining Odds & Ends

- [ ] **MISC-01**: No builtin left behind — `bisect`, `notes`/`replace`, `daemon`, `shell`, `filter-branch`, `scalar`, `diagnose`, `bugreport`, `hook`, `backfill` and every other remaining C builtin with C-identical behavior
- [ ] **MISC-02**: Migrations stay possible via the one-way legacy importers (`cvs*`/`svn`/`p4`/`quiltimport`/`archimport`)

## v2 Requirements

None — the goal is full conversion, so nothing is deferred. Phasing lives in ROADMAP.md.

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| New/different UX on top of git semantics | Breaks the byte-identical core value; differential gate would fail by construction |
| `gitweb`/`instaweb`, `gui`/`citool` | Separate Perl/Tcl applications, not part of the C implementation |
| Adding `clap`/`serde`/`tokio`/async or new crypto deps casually | Locked architecture decision; plan amendment required for any new dep |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| REFS-01 | Phase 1 | Pending |
| REFS-02 | Phase 1 | Pending |
| CONF-01 | Phase 1 | Pending |
| SCRIPT-01 | Phase 2 | Pending |
| SCRIPT-02 | Phase 2 | Pending |
| SCRIPT-03 | Phase 2 | Pending |
| INSP-01 | Phase 3 | Pending |
| INSP-02 | Phase 3 | Pending |
| LIFE-01 | Phase 4 | Pending |
| LIFE-02 | Phase 4 | Pending |
| LIFE-03 | Phase 4 | Pending |
| LIFE-04 | Phase 4 | Pending |
| MERGE-01 | Phase 5 | Pending |
| SEQ-01 | Phase 5 | Pending |
| SEQ-02 | Phase 5 | Pending |
| SEQ-03 | Phase 5 | Pending |
| SEQ-04 | Phase 5 | Pending |
| STORE-01 | Phase 6 | Pending |
| STORE-02 | Phase 6 | Pending |
| STORE-03 | Phase 6 | Pending |
| STORE-04 | Phase 6 | Pending |
| TRAN-01 | Phase 6 | Pending |
| INTG-01 | Phase 7 | Pending |
| DEPTH-01 | Phase 7 | Pending |
| TRAN-02 | Phase 8 | Pending |
| MAIL-01 | Phase 9 | Pending |
| MAIL-02 | Phase 9 | Pending |
| SUBM-01 | Phase 10 | Pending |
| SIGN-01 | Phase 9 | Pending |
| INTER-01 | Phase 10 | Pending |
| FILT-01 | Phase 11 | Pending |
| FILT-02 | Phase 11 | Pending |
| MISC-01 | Phase 12 | Pending |
| MISC-02 | Phase 12 | Pending |

**Coverage:**
- v1 requirements: 34 total
- Mapped to phases: 34
- Unmapped: 0

---
*Requirements defined: 2026-09-26*
*Last updated: 2026-09-26 after initial definition*
