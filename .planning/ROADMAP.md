# Roadmap: git.rs — Pure-Rust Rewrite of Git

## Overview

From a partially-ported workspace (~45 builtins green) to the complete conversion of all of C git: reflog and ref-transaction safety first, then the cheap scriptability and daily-use wins, then the file-lifecycle engine, then the merge hub with its sequencer consumers, store maintenance sealed with a bounded file-local transport head, then the integrity-plus-depth parity pass, full network transports, patch exchange with signing trust, nested repos with interactive porcelain, checkout conversion at monorepo scale, and finally the long tail of every remaining builtin plus the legacy one-way importers — every step gated byte-identical against C git.

## Phases

**Phase Numbering:**

- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Refs & Config Foundation** - Reflog safety net, atomic ref transactions, and the config command
- [ ] **Phase 2: Scriptability Layer** - rev-parse completion, merge-adjacent plumbing, and the small plumbing batch
- [ ] **Phase 3: History Inspection** - show/describe/name-rev/shortlog plus grep and blame parity
- [ ] **Phase 4: File Lifecycle** - rm/mv/clean, apply, read-tree/write-tree engine, and modern index formats
- [ ] **Phase 5: Merge & Sequencer Workflows** - merge-ort plus cherry-pick/revert, rebase, stash, and worktree
- [ ] **Phase 6: Store Maintenance & Local Transport** - commit-graph/MIDX/index-pack/gc plus file-local clone/fetch/push/pull
- [ ] **Phase 7: Integrity & Option Depth** - Full fsck plus option-for-option parity depth across routed commands
- [ ] **Phase 8: Network Transport** - ssh/http transports, daemon, protocol v2, and credentials
- [ ] **Phase 9: Patch Exchange & Signing** - am/format-patch/mailinfo, send-email/imap-send, verify plus commit/tag signing
- [ ] **Phase 10: Nested Repos & Interactive Porcelain** - Full submodule family plus interactive add/rebase/stash with pager/color
- [ ] **Phase 11: Checkout Conversion & Scale** - Smudge/clean filters, autocrlf matrix, sparse-checkout/sparse-index
- [ ] **Phase 12: Long Tail & Legacy Importers** - Every remaining C builtin plus one-way legacy importers

## Phase Details

### Phase 1: Refs & Config Foundation

**Goal**: Users get a reflog safety net, crash-safe ref updates, and working git config management
**Depends on**: Nothing (first phase)
**Requirements**: REFS-01, REFS-02, CONF-01
**Success Criteria** (what must be TRUE):

  1. User can inspect reflog history after every mutating command and recover a moved ref from it
  2. Concurrent ref updates never tear packed-refs and `update-ref --stdin` applies multi-ref transactions atomically
  3. User can get, set, unset, and list config values across system/global/local/worktree scopes with includes honored
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: 4 plans

Plans:
**Wave 1**

- [ ] 01-01-PLAN.md — Storage tracer: lock helper, reflog writer, packed write, transaction engine
- [ ] 01-02-PLAN.md — Config storage: scopes, includeIf, file editor, typed ops

**Wave 2** *(blocked on Wave 1 completion)*

- [ ] 01-03-PLAN.md — Refs surface: full reflog matrix, update-ref --stdin, writer convergence

**Wave 3** *(blocked on Wave 2 completion)*

- [ ] 01-04-PLAN.md — Config surface plus gates: command matrix, shim, crosswise, scoreboard

### Phase 2: Scriptability Layer

**Goal**: Scripts can rely on full rev-parse resolution and merge-adjacent plumbing
**Depends on**: Phase 1
**Requirements**: SCRIPT-01, SCRIPT-02, SCRIPT-03
**Success Criteria** (what must be TRUE):

  1. User scripts can resolve `@{...}`, `A..B`/`A...B` ranges, `--all`, `--is-bare-repository`, and `--sq` quoting through rev-parse
  2. User can compute octopus/independent merge bases and compare trees via merge-tree and cherry
  3. User can run the small plumbing batch (pack-refs, mktag, check-ref-format, stripspace, var, patch-id, check-mailmap, interpret-trailers, show-index, diff-files, diff-index) with C-identical output
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 3: History Inspection

**Goal**: Users can inspect, search, and annotate history with C-identical output
**Depends on**: Phase 2
**Requirements**: INSP-01, INSP-02
**Success Criteria** (what must be TRUE):

  1. User can inspect history daily via completed show plus describe, name-rev, and shortlog
  2. User can search worktree/index/history via grep and annotate lines via blame --porcelain with stable output
  3. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 4: File Lifecycle

**Goal**: Users can manage tracked-file lifecycle end to end, from rm/mv through patches to tree plumbing
**Depends on**: Phase 3
**Requirements**: LIFE-01, LIFE-02, LIFE-03, LIFE-04
**Success Criteria** (what must be TRUE):

  1. User can remove, rename, and clean untracked files via completed rm/mv/clean
  2. User can apply patches including --3way/--index/--reject, binary patches, and whitespace variants
  3. Three-way read-tree (-m/-u/--prefix) and write-tree edge cases behave as the engine under checkout/merge/stash
  4. Repos written by C git 2.38+ open correctly, including index v3/v4 read/write and the REUC extension
  5. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 5: Merge & Sequencer Workflows

**Goal**: Users can complete branch workflows end to end — merge, cherry-pick/revert, rebase, stash, and multiple checkouts
**Depends on**: Phase 4
**Requirements**: MERGE-01, SEQ-01, SEQ-02, SEQ-03, SEQ-04
**Success Criteria** (what must be TRUE):

  1. User can merge branches on the merge-ort backend with rename detection and conflict clustering matching C git
  2. User can cherry-pick and revert with --continue/--abort resume on the shared sequencer engine
  3. User can rebase non-interactively (am + merge backends) and stash push/pop/apply/list/drop over refs/stash
  4. User can manage multiple checkouts via worktree add/list/lock/repair with the commondir layout
  5. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 6: Store Maintenance & Local Transport

**Goal**: Repos stay fast, readable, and bounded, and users can clone/fetch/push/pull over file-local transport
**Depends on**: Phase 5
**Requirements**: STORE-01, STORE-02, STORE-03, STORE-04, TRAN-01
**Success Criteria** (what must be TRUE):

  1. Repos the port writes stay acceleratable — commit-graph write plus chains and bloom queries readable by C git
  2. Large repos stay readable — MIDX completion (RIDX/BTMP/BASE, incremental, --preferred-pack) verified crosswise
  3. Fetched/cloned packs ingest via index-pack --stdin with thin-pack base resolution
  4. Repos stay bounded via gc/repack/prune basics, and user can clone/fetch/push/pull plus ls-remote over file-local transport with no sockets
  5. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 7: Integrity & Option Depth

**Goal**: Users can trust repo integrity via full fsck and get option-for-option parity depth on every routed command
**Depends on**: Phase 6
**Requirements**: INTG-01, DEPTH-01
**Success Criteria** (what must be TRUE):

  1. User can verify repo integrity via full fsck (--strict, --connectivity-only, --no-dangling, --full, --lost-found) with the C-identical message catalog
  2. User gets C-identical output for the option-depth family across routed commands (--word-diff, --histogram/--patience, -S/-G pickaxe, --dirstat, whitespace family, --relative)
  3. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 8: Network Transport

**Goal**: Users can clone, fetch, and push over the network with protocol v2 negotiation
**Depends on**: Phase 7
**Requirements**: TRAN-02
**Success Criteria** (what must be TRUE):

  1. User can clone/fetch/push over ssh and http transports plus the git daemon, reusing the Phase 6 file-local negotiation core
  2. Protocol v2 negotiation works with fallback, and credentials are handled per platform on the sync-first stack (no async runtime)
  3. Repos fetched by the port verify under C git (fsck/verify-pack) and vice versa — the crosswise transport contract
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 9: Patch Exchange & Signing

**Goal**: Users can exchange patches by email and trust commits/tags through verification and signing
**Depends on**: Phase 8
**Requirements**: MAIL-01, MAIL-02, SIGN-01
**Success Criteria** (what must be TRUE):

  1. User can produce and consume mailbox patch series via format-patch/am with mailinfo/mailsplit parsing matching C git
  2. User can send patches via send-email plus imap-send over the Phase 8 network stack
  3. User can verify commits/tags and sign via commit/tag with gpgsig headers preserved byte-exactly for hash correctness
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 10: Nested Repos & Interactive Porcelain

**Goal**: Users can compose nested repos via submodules and drive staged workflows interactively
**Depends on**: Phase 9
**Requirements**: SUBM-01, INTER-01
**Success Criteria** (what must be TRUE):

  1. User can manage nested repos via the full submodule family (add/update/init/foreach/sync) cloning over the available transports
  2. User can stage hunks via add -p, edit todo lists via rebase -i, and split stash hunks via stash -p with pager/color handling honored
  3. Interactive behavior matches C git under the pty oracle (the differential harness cannot cover terminal interaction)
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 11: Checkout Conversion & Scale

**Goal**: Checkouts convert content correctly through filters and scale to monorepos via sparse layouts
**Depends on**: Phase 10
**Requirements**: FILT-01, FILT-02
**Success Criteria** (what must be TRUE):

  1. User checkouts convert correctly via smudge/clean filter drivers plus the full core.autocrlf matrix matching C git
  2. User monorepos scale via sparse-checkout/sparse-index/split-index with C-identical sparse behavior
  3. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

### Phase 12: Long Tail & Legacy Importers

**Goal**: No C builtin is left behind and legacy repos can still migrate in
**Depends on**: Phase 11
**Requirements**: MISC-01, MISC-02
**Success Criteria** (what must be TRUE):

  1. User can run every remaining C builtin (bisect, notes/replace, daemon, shell, filter-branch, scalar, diagnose, bugreport, hook, backfill, and all others) with C-identical behavior
  2. User can migrate legacy repos via the one-way importers (cvs*/svn/p4/quiltimport/archimport)
  3. The full `t/` suite passes through the shim with no scoreboard regression — the whole-suite seal on the complete conversion
  4. Ported commands produce byte-identical stdout/stderr/exit codes vs C git on the phase's `t/` gates with no scoreboard regression

**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Refs & Config Foundation | 0/TBD | Not started | - |
| 2. Scriptability Layer | 0/TBD | Not started | - |
| 3. History Inspection | 0/TBD | Not started | - |
| 4. File Lifecycle | 0/TBD | Not started | - |
| 5. Merge & Sequencer Workflows | 0/TBD | Not started | - |
| 6. Store Maintenance & Local Transport | 0/TBD | Not started | - |
| 7. Integrity & Option Depth | 0/TBD | Not started | - |
| 8. Network Transport | 0/TBD | Not started | - |
| 9. Patch Exchange & Signing | 0/TBD | Not started | - |
| 10. Nested Repos & Interactive Porcelain | 0/TBD | Not started | - |
| 11. Checkout Conversion & Scale | 0/TBD | Not started | - |
| 12. Long Tail & Legacy Importers | 0/TBD | Not started | - |
