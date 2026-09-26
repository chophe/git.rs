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

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Integrity & Depth

- **INTG-01**: Full `fsck` completion (`--strict`, `--connectivity-only`, `--no-dangling`, `--full`, `--lost-found`, message catalog) (gate: `t/t1450`)
- **DEPTH-01**: Option-parity depth on routed commands (`--word-diff`, `--histogram/--patience`, `-S/-G` pickaxe, `--dirstat`, whitespace family `-w/-b`, `--relative`)

### Full Transport

- **TRAN-02**: Full network transports (ssh/http/daemon, network push) as the next milestone in plan order

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| New/different UX on top of git semantics | Breaks the byte-identical core value; differential gate would fail by construction |
| `submodule` family | Historically buggiest corner of C git; plan defers to Phase F |
| Email stack (`am`, `format-patch`, `send-email`) | Separate product surface with its own format quirks |
| GPG/SSH signature verification | Platform-entangled crypto stack; parse-and-preserve `gpgsig` headers only |
| Interactive UI (`add -p`, `rebase -i`, pager, `--color`) | Terminal-interactive code untestable in the differential harness |
| Smudge/clean filters + full autocrlf matrix | Multiplies the test matrix of core flows; revisit only if Windows becomes a target |
| Sparse-checkout / sparse-index / split-index | Layers on top of in-flight index work (B2/B4 partial); premature surface |
| Adding `clap`/`serde`/`tokio`/async or new crypto deps | Locked architecture decision; requires plan amendment |
| `filter-branch`, `scalar`, `gitweb`/GUI, `cvs*`/`svn` bridges | Low daily use or explicitly not planned in conversion-plan Phase F |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| REFS-01 | TBD | Pending |
| REFS-02 | TBD | Pending |
| CONF-01 | TBD | Pending |
| SCRIPT-01 | TBD | Pending |
| SCRIPT-02 | TBD | Pending |
| SCRIPT-03 | TBD | Pending |
| INSP-01 | TBD | Pending |
| INSP-02 | TBD | Pending |
| LIFE-01 | TBD | Pending |
| LIFE-02 | TBD | Pending |
| LIFE-03 | TBD | Pending |
| LIFE-04 | TBD | Pending |
| MERGE-01 | TBD | Pending |
| SEQ-01 | TBD | Pending |
| SEQ-02 | TBD | Pending |
| SEQ-03 | TBD | Pending |
| SEQ-04 | TBD | Pending |
| STORE-01 | TBD | Pending |
| STORE-02 | TBD | Pending |
| STORE-03 | TBD | Pending |
| STORE-04 | TBD | Pending |
| TRAN-01 | TBD | Pending |

**Coverage:**
- v1 requirements: 22 total
- Mapped to phases: 0
- Unmapped: 22

---
*Requirements defined: 2026-09-26*
*Last updated: 2026-09-26 after initial definition*
