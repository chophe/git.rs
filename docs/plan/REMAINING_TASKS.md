# Remaining Unimplemented Tasks — git.rs Port

Generated from `docs/plan/FOLLOWUPS.md` and `docs/plan/conversion-plan.md`.

---

## Phase A — Foundation Fixes (in progress)

| Item | Status | Description | Crate/Module | Test Gate |
|------|--------|-------------|--------------|-----------|
| **A1** | ✅ DONE | Collision-detecting SHA-1 (sha1dc) | `git-hash` | `t/t0013` |
| **A2** | ✅ DONE | `--git-dir`/`--work-tree` threading through all commands | `git-command` (RepoContext) | existing suites |
| **A3** | ✅ DONE | `cat-file --batch` / `--batch-check` / `%(format)` | `git-command/cat_file.rs` | `t/t1006` |
| **A4** | ✅ DONE (as A13) | Pack delta compression in `pack-objects` | `git-odb/pack` | `phaseA13_crosswise` + `git verify-pack` |
| **A5** | ✅ DONE | Abbreviation resolution (short OIDs, refs, `HEAD~n`) | `git-revision` + `git-command` | `t/t1514`, `t/t1400` |
| **A6** | ✅ DONE | Local-timezone / calendar parity in `git-date` | `git-date` | `t/t0006` |
| **A7** | ✅ DONE | Ident offset uses local timezone | `git-command/ident.rs` | `t/t0006` |
| **A8** | ✅ DONE | Diff/patch engine completion | `git-diff`, `git-command/diff.rs`, `diff_tree.rs` | `phaseA08_crosswise` |
| **A9** | ✅ DONE | Hunk-header function context (userdiff drivers) | `git-diff` | `t/t4018`, `phaseA09_crosswise` |
| **A10** | ✅ DONE | `count-objects -v` real sizes | `git-command/count_objects.rs` | `t/t1450` |
| **A11** | ✅ DONE | `.gitignore` + attributes engine | `git-attributes` (+ `check-ignore`, `check-attr`) | `t/t0008`, `t/t0003`, `phaseA11_crosswise` |
| **A12** | ✅ DONE | Local timezone for dates + idents (fixes UTC-only) | `git-date`, `git-command/ident.rs` | `t/t0006` |
| **A13** | ✅ DONE | Delta compression in `pack-objects` write path | `git-odb/pack` | `phaseA13_crosswise` + `git verify-pack` |

**Deferred from A8 (explicitly noted in FOLLOWUPS):**
- Word-diff (`--word-diff`)
- `--color` output
- `--patience` / `--histogram` algorithms
- `--dirstat`
- Whitespace family (`-w`, `-b`, `--ignore-blank-lines`)
- `--relative` paths
- `-S`/`-G` pickaxe
- Stat width hard-coded to 80 columns

**Known deviations from A8:**
- Exact rename scoring differs from C's `diffcore-rename`
- `--diff-filter` lowercase = exclude (C's is undocumented)
- `T` (typechange) status from mode changes only

---

## Phase B — Workflow Core ("usable repo")

*Prerequisites: A8 (diff), A11 (ignore/attributes)*

| Item | Status | Description | Crate | Test Gate |
|------|--------|-------------|-------|-----------|
| **B1** | ✅ DONE | `git init` (templates, `--bare`, `--separate-git-dir`, default branch) | `git-command/init.rs` | `t/t0001`, `phaseB01_crosswise` |
| **B2** | 🟡 PARTIAL | Index extensions: cache-tree (`TREE`) done; REUC, index v3/v4 pending | `git-index` | `t/t0060`, `t/t3007`, `phaseB04_crosswise` |
| **B3** | ✅ DONE | `git add` (pathspecs, ignore integration, `-A`/`-u`/`-n`/`-v`/`-f`, racy-aware, cache-tree parity) | `git-command/add.rs` | `t/t3700`, `phaseB03_crosswise` |
| **B4** | 🟡 PARTIAL | `git write-tree` (full) + `read-tree` one-way/`--empty`/`-n`/`--index-output`; `read-tree -m`/`-u`/`--prefix` pending | `git-command` | `t/t1000`, `phaseB04_crosswise` |
| **B5** | ✅ DONE | `git commit` (`-a`, `--amend`, `--allow-empty`, author/committer, editor, signoff) | `git-command/commit.rs` | `t/t7501`, `phaseB05_crosswise` |
| **B6** | ❌ NOT DONE | `git status` full: long/short, `-z`, `--branch`, rename detection, `--ignored` | `git-command/status.rs`, `git-index` | `t/t7508`, `t/t7010` |
| **B7** | ❌ NOT DONE | `unpack-trees` + `git checkout`/`switch`/`restore`/`reset` (mixed/soft/hard) | new `git-worktree` + `git-command` | `t/t2000`–`t/t2030`, `t/t7102` |
| **B8** | ❌ NOT DONE | `git rm`, `mv`, `clean` | `git-command` | `t/t3600`, `t/t7001`, `t/t7300` |
| **B9** | ❌ NOT DONE | `git show`, `shortlog`, `describe`, `name-rev`, `whatchanged` | `git-command` | `t/t4000`, `t/t4201`, `t/t6120` |
| **B10** | ❌ NOT DONE | `git apply` completion: `--3way`, `--index`, `--reject`, whitespace, binary | `git-command/apply.rs` | `t/t4103`–`t/t4137` |

---

## Phase C — History & Merge Completeness

| Item | Status | Description | Crate | Test Gate |
|------|--------|-------------|-------|-----------|
| **C1** | ❌ NOT DONE | `git merge` via **merge-ort**: rename detection, dir/file conflicts, recursive criss-cross, index merge | `git-merge` | `t/t6402`–`t/t6430` |
| **C2** | ❌ NOT DONE | `merge-tree`, `merge-base --octopus/--independent/--is-ancestor`, `cherry` | `git-merge` | `t/t6010`, `t/t6602` |
| **C3** | ❌ NOT DONE | `cherry-pick` / `revert` (sequencer subset) | new `git-sequencer` | `t/t3501`–`t/t3510` |
| **C4** | ❌ NOT DONE | `git rebase` (am- and merge-based backends) | `git-sequencer` | `t/t3400` family |
| **C5** | ❌ NOT DONE | Reflog: read/write `logs/<ref>`, `git reflog`, reflog-aware update/branch/delete | `git-refs` + `git-command` | `t/t1410` |
| **C6** | ❌ NOT DONE | Packed-refs write, ref locking/transactions, `update-ref --stdin`, symrefs, worktree refs | `git-refs` | `t/t1400`, `t/t3210` |
| **C7** | ❌ NOT DONE | Small independent commands: `pack-refs`, `update-server-info`, `mktag`, `check-ref-format`, `stripspace`, `var`, `patch-id`, `check-mailmap`, `interpret-trailers`, `check-ignore`, `check-attr`, `show-index`, `unpack-file`, `diff-files`, `diff-index`, `diff-pairs`, `for-each-repo`, `url-parse` | `git-command` | respective `t/t` |
| **C8** | ❌ NOT DONE | `git config` command: read/write, scope resolution, includes, env vars | `git-config` + `git-command/config.rs` | `t/t1300` |
| **C9** | ❌ NOT DONE | `notes`, `replace`, `worktree`, `bisect`, `stash`, `range-diff`, `rerere`, `blame`, `grep`, `archive` | various | respective suites |
| **C10** | ❌ NOT DONE | `fsck` completion: `--strict`, `--connectivity-only`, `--no-dangling`, `--full`, `--lost-found`, message catalog | `git-command/fsck.rs` | `t/t1450` full pass |

---

## Phase D — On-Disk Format Completeness (ODB)

| Item | Status | Description | Crate | Test Gate |
|------|--------|-------------|-------|-----------|
| **D1** | ❌ NOT DONE | Pack bitmaps (EWAH, `pack-bitmap` + MIDX bitmap), reachability queries | `git-odb` | `t/t5310` |
| **D2** | ❌ NOT DONE | Cruft packs / `pack-mtimes` | `git-odb` | `t/t7704` |
| **D3** | ❌ NOT DONE | Commit-graph **write** + chains + bloom **query** | `git-commitgraph` | `t/t5318`, `t/t5324` |
| **D4** | ❌ NOT DONE | MIDX: `RIDX`/`BTMP`/`BASE` chunks, incremental MIDX, `--preferred-pack` | `git-odb/midx` | `t/t5319`, `t/t5334` |
| **D5** | ❌ NOT DONE | `git index-pack` (`--stdin`, thin-pack base resolution, `--verify`) | `git-command/index_pack.rs` | `t/t5302` |
| **D6** | ❌ NOT DONE | `git repack`, `gc`, `prune`, `prune-packed`, `maintenance`, `pack-redundant`, `replay` | `git-command` | `t/t7700`–`t/t7704`, `t/t5304` |
| **D7** | ❌ NOT DONE | sha1↔sha256 object conversion (`compatObjectFormat`, `gpgsig`↔`gpgsig-sha256`, LMAP) | `git-odb`/`git-hash` | `t/t1016` |

---

## Phase E — Network & Transport

*Start after D6 (needs repack/gc for maintenance-on-fetch, D1 for bitmap negotiation)*

| Item | Status | Description | Crate | Test Gate |
|------|--------|-------------|-------|-----------|
| **E1** | ❌ NOT DONE | pkt-line framing + protocol v2 state machine | new `git-transport` | `t/t5500` |
| **E2** | ❌ NOT DONE | Local/file transport → `ls-remote` | `git-transport`, `git-command` | `t/t5510`, `t/t5503` |
| **E3** | ❌ NOT DONE | `fetch-pack`/`upload-pack` negotiation + `fetch` | `git-transport` | `t/t5510`–`t/t5538` |
| **E4** | ❌ NOT DONE | `send-pack`/`receive-pack` + `push` (refspec rules, force, atomic) | `git-transport` | `t/t5528`–`t/t5541` |
| **E5** | ❌ NOT DONE | git:// daemon transport | `git-transport` | `t/t5570` |
| **E6** | ❌ NOT DONE | HTTP smart transport (`http-fetch`, `http-push`, `http-backend`) | `git-transport` | `t/t5539`, `t/t5541` |
| **E7** | ❌ NOT DONE | SSH transport | `git-transport` | `t/t5601` |
| **E8** | ❌ NOT DONE | `git clone` / `pull` (compose fetch + checkout + refs) | `git-command` | `t/t5601`, `t/t5603` |
| **E9** | ❌ NOT DONE | `git remote`, credential stack, `request-pull`, `fetch-pack` extras | `git-command` | `t/t5505`, `t/t5550` |
| **E10** | ❌ NOT DONE | `bundle` + `bundle-uri`, `fast-export` / `fast-import` | `git-command` | `t/t5607`, `t/t9300`, `t/t9350` |

---

## Phase F — Stretch / Low Priority

| Item | Status | Description |
|------|--------|-------------|
| ❌ | NOT DONE | `verify-commit`/`verify-tag` (signature parsing + verification) |
| ❌ | NOT DONE | `filter-branch` |
| ❌ | NOT DONE | `am` + `format-patch` completion, `mailinfo`/`mailsplit` |
| ❌ | NOT DONE | `send-email`, `imap-send` |
| ❌ | NOT DONE | `mergetool`, `difftool`, `daemon`, `scalar` |
| ❌ | NOT DONE | `backfill`, `diagnose`, `history`, `repo`, `bugreport` |
| ❌ | NOT DONE | `hook`, `instaweb`, `gitweb`, `gui`/`citool`, `shell` |
| ❌ | NOT DONE | `sh-i18n`/`sh-setup`, `submodule` family |
| ❌ | NOT DONE | `clean` filters (clean/smudge + `core.autocrlf`) |
| ❌ | NOT DONE | Sparse index/split index/checkout (`sparse-checkout`, `maintenance`) |

**Explicitly OUT OF SCOPE:**
- `cvs*`, `svn`, `p4`, `quiltimport`, `archimport`, GUI tools

---

## Test Infrastructure (Parallel Track)

| Item | Status | Description |
|------|--------|-------------|
| **B3** | ❌ NOT DONE | `git-test` crate (Rust `test-tool` replacement): `test-sha1`, `test-sha256`, `test-date`, `test-config`, `test-varint`, `test-zlib`, `test-delta`, `test-pack-deltas`, `test-find-pack`, `test-read-midx`, `test-read-graph`, `test-bloom`, `test-revision-walking`, `test-reach`, `test-read-cache`, `test-write-cache`, `test-dump-cache-tree`, `test-dump-split-index`, `test-ref-store`, `test-reftable`, `test-wildmatch` |
| **B4** | ❌ NOT DONE | Fuzz targets (`cargo-fuzz`) for pack/idx/midx/commit-graph/bitmap/index/config/reftable/loose-object/xdiff |
| **B7** | ❌ NOT DONE | Coverage gate: `cargo llvm-cov --fail-under-lines 90` in CI |
| — | ⚠️ PARTIAL | t/ scoreboard: `cargo xtask scoreboard` works for differential suites; real `t/` suite needs C `test-tool` |

---

## Known Deviations to Revisit (FOLLOWUPS Section C)

| Item | Status | Description |
|------|--------|-------------|
| — | ✅ DONE (A6/A12) | `git-date`: local-timezone parsing, calendar-aware month/year relative math, local ident offsets |
| — | ❌ NOT DONE | `pack-objects`: non-deltified packs |
| — | ❌ NOT DONE | `hash-object` outside repo: always SHA-1 (confirm `-t`/`--stdin` parity vs `t1007`) |

---

## Phase Status Summary (from FOLLOWUPS Section D)

| Phase | Status | Key Remaining |
|-------|--------|---------------|
| **Phase 3** (MIDX, bitmaps, commit-graph) | Partially done | commit-graph write, bloom query, pack bitmaps, cruft packs, chains, MIDX optional chunks |
| **Phase 4** (Object model & revwalk) | Partially done | HEAD/ref resolution, pretty-printing, revision ordering, commit-graph walks |
| **Phase 5** (Diff) | Partially done | rename/copy detection, pickaxe, output formats (word-diff, color), `diff-tree --exit-code` |
| **Phase 6** (Index & worktree) | Partially done | cache-tree, split/sparse index, checkout/reset, `diff-files`, racy-clean, `.gitignore` |
| **Phase 7** (Refs & reftable) | Partially done | reflog, reftable backend, packed-refs write, ref transactions, symrefs |
| **Phase 8** (Merge & reachability) | Partially done | merge-ort, recursive criss-cross, index merging, cherry-pick/revert |
| **Phase 9** (Conversion & fsck) | Partially done | sha1↔sha256, LMAP, fsck options, repack/gc |

---

## Definition of Done (Full Conversion)

- [ ] Every in-scope command from `command-list.txt` routed in `scripts/shim-git` with crosswise suite
- [ ] All 8 phase gates from `docs/plan/README.md` green (unit+proptests, crosswise, scoreboard, coverage ≥90%)
- [ ] `t/` scoreboard baseline at 100% for in-scope scripts (shim routes nothing to C git)
- [ ] FOLLOWUPS.md sections A and C empty; section B fully implemented

---

## Critical Path (Single-Agent)

```
A2 (done) → A8 (done) → A9 (done) → A11 → A13
                          → B3 → B4 → B5 → B7 → C1 → D6 → E8
```

## Parallelizable Tracks

- **A7/A9/A11** (independent crates)
- **C5–C9** (small independent commands)
- **D1–D5** (ODB format work)
- **Test infrastructure** (git-test, fuzz, coverage)

---

## Next Immediate Action Items

1. **A11** — `.gitignore` + attributes engine (new crate `git-ignore` or in `git-index`)
   - Blocks B3 (`git add`), B6 (`status --ignored`), and full `.gitattributes` support

2. **A13** — Delta compression in `pack-objects` (in `git-odb/pack`)
   - Prerequisite for real-world repo sizes; enables `git verify-pack` compatibility

3. **B3/B6** — `git add` and full `git status` (depends on A11)

4. **Test infra B3** — `git-test` crate (unblocks running real `t/` suite without C build)

5. **Phase 3 completion** — commit-graph write, bloom query, pack bitmaps

---

*Last updated: A9 (userdiff hunk headers) completed and crosswise-verified.*