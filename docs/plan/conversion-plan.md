# Remaining Conversion Plan: C git → Rust

This is the actionable, ordered plan for converting everything not yet ported.
The evidence base (what is and isn't done, per command and per subsystem) lives
in [gap-analysis.md](gap-analysis.md); the itemized deviations and deferred
code live in [FOLLOWUPS.md](FOLLOWUPS.md). This document defines **what order
to build it and how to know each step is done**.

## Principles (unchanged from the locked strategy)

- **Standalone, pure Rust, no FFI.** C git stays the spec and the oracle.
- **Depth before breadth**: every routed command must reach full option parity
  before new commands are started (the gap analysis shows routed commands are
  currently shallow, e.g. `diff` = 2 options vs ~100 in C).
- **Every step closes with its gates**: unit+proptests green, crosswise suite
  byte-identical, `cargo xtask scoreboard` no-regression, coverage target on
  touched crates, and the step's named `t/` script(s) passing through the shim.
- **Small landable units**: each item below is one PR-sized change with a test
  artifact; nothing here requires a mega-branch.

## Phase A — Deepen what's routed (foundation fixes)

Dependency-free; everything later builds on these.

| # | Item | Crate/Module | Spec / test gate | Notes |
|---|---|---|---|---|
| A0 | Phase A overview & order | [phase-a/00-overview.md](phase-a/00-overview.md) | shared gates | per-item expanded docs |
| A1 | sha1dc collision-detecting SHA-1; `is_safe()` true | `git-hash` | `t/t0013` | FOLLOWUPS A1 — [phase-a/01-sha1dc.md](phase-a/01-sha1dc.md) |
| A2 | `--git-dir`/`--work-tree` threading through all commands | `git-command` (shared context struct) | existing suites stay green | FOLLOWUPS C; do this **first** so later commands are born with it — [phase-a/02-repo-discovery-env.md](phase-a/02-repo-discovery-env.md) |
| A3 | `cat-file --batch` / `--batch-check` / `%(format)` | `git-command/cat_file.rs` | `t/t1006` | FOLLOWUPS A3 — [phase-a/03-cat-file-batch.md](phase-a/03-cat-file-batch.md) |
| A4 | abbreviation resolution (short OIDs, refs, `HEAD~n`) as shared helper | `git-revision` + `git-command` | `t/t1514`, `t/t1400` | unblocks commit-tree, log ranges, rev-parse — [phase-a/04-abbrev-resolution.md](phase-a/04-abbrev-resolution.md) |
| A5 | `rev-parse` completion: `@{...}`, `A..B`/`A...B`, `--all`, `--is-bare-repository`, `--sq` | `git-command/rev_parse.rs` | `t/t1500`–`t/t1503` | Phase 7 leftover — [phase-a/05-rev-parse-completion.md](phase-a/05-rev-parse-completion.md) |
| A6 | `rev-list`/`log` options: `--objects`, `--count`, `-n`, ranges, `--topo-order`, `--date-order`, `--first-parent`, `--all`, path limiting, grafts/replace | `git-revision`, `git-command/rev_list.rs`, `log.rs` | `t/t6001`–`t/t6019` | Phase 4 leftover — [phase-a/06-rev-list-log-options.md](phase-a/06-rev-list-log-options.md) |
| A7 | pretty-printing engine (`pretty.c` port): `--format`, default `Date:` line, date formats, trailers, mailmap | new `git-pretty` crate | `t/t4205`, `t/t6006` | needed by `log`, `format-patch`, `show`, `shortlog` — [phase-a/07-pretty-engine.md](phase-a/07-pretty-engine.md) |
| A8 | diff engine completion: `--cached`, `-U<n>`, `--stat`/`--numstat`/`--shortstat`/`--summary`, rename/copy detection, `--diff-filter`, pickaxe, no-newline-at-EOF, binary, `--exit-code` | `git-diff`, `git-command/diff.rs`, `diff_tree.rs` | `t/t4001`–`t/t4014`, `t/t4002` (rename) | Phase 5 leftover; largest algorithm item in Phase A — [phase-a/08-diff-options.md](phase-a/08-diff-options.md) |
| A9 | hunk-header function context (userdiff drivers) | `git-diff` | `t/t4018` | port `userdiff.c` driver table — [phase-a/09-userdiff-hunk-headers.md](phase-a/09-userdiff-hunk-headers.md) |
| A10 | `count-objects -v` real sizes | `git-command/count_objects.rs` | `t/t1450` (fsck suite covers -v indirectly) | sizes implemented; close-out — [phase-a/10-count-objects-v.md](phase-a/10-count-objects-v.md) |
| A11 | **`.gitignore` + attributes engine**: `ignore.c`, `attr.c` port, `GIT_*` env | new `git-ignore` (or inside `git-index`) | `t/t0007` (attr), `t/t0008` (ignore) | hard prerequisite for `add`, `status --ignored`, `clean`, `ls-files --others` — [phase-a/11-gitignore-attributes.md](phase-a/11-gitignore-attributes.md) |
| A12 | local timezone for dates + idents (fixes UTC-only) | `git-date`, `git-command/ident.rs` | `t/t0006` | FOLLOWUPS A6/A7 — [phase-a/12-local-timezone-dates.md](phase-a/12-local-timezone-dates.md) |
| A13 | delta compression in `pack-objects` write path | `git-odb/pack` | existing `pack_crosswise` + `git verify-pack` | FOLLOWUPS A4; also prerequisite for real-world repo size — [phase-a/13-pack-delta-compression.md](phase-a/13-pack-delta-compression.md) |

## Phase B — Workflow core ("usable repo")

Ordered by internal dependency. A11 (ignore) and A8 (diff) from Phase A are
prerequisites for B3/B6.

| # | Item | Crate | Gate |
|---|---|---|---|
| B1 | `git init` (templates, `--bare`, `--separate-git-dir`, default branch) | `git-command/init.rs` | `t/t0001` |
| B2 | index extensions: cache-tree (`TREE`), REUC; index v3/v4 read/write | `git-index` | `t/t0060`, `t/t3007` |
| B3 | `git add` (pathspec, ignore integration, refresh, stat handling, `-p` interactive later) | `git-command/add.rs` | `t/t3700`, `t/t3701` |
| B4 | `git write-tree`, `read-tree` (one/two/three-way) | `git-command` | `t/t1000`, `t/t2000` |
| B5 | `git commit` (`-a`, `--amend`, `--allow-empty`, author/committer plumbing, editor, signoff) | `git-command/commit.rs` | `t/t7501`, `t/t7502` |
| B6 | `git status` full: long/short, `-z`, `--branch`, rename detection, `--ignored`, stat-based scan + racy-clean | `git-command/status.rs`, `git-index` | `t/t7508`, `t/t7010` |
| B7 | `unpack-trees` port + `git checkout` / `switch` / `restore` / `reset` (mixed/soft/hard) | new `git-worktree` crate + `git-command` | `t/t2000`–`t/t2030`, `t/t7102` |
| B8 | `git rm`, `mv`, `clean` | `git-command` | `t/t3600`, `t/t7001`, `t/t7300` |
| B9 | `git show`, `shortlog`, `describe`, `name-rev`, `whatchanged` | `git-command` | `t/t4000`, `t/t4201`, `t/t6120` |
| B10 | `git apply` completion: `--3way`, `--index`, `--reject`, whitespace options, binary patches, `\ No newline` | `git-command/apply.rs` | `t/t4103`–`t/t4137` |

## Phase C — History & merge completeness

| # | Item | Crate | Gate |
|---|---|---|---|
| C1 | `git merge` via **merge-ort** port: rename detection, dir/file conflicts, recursive criss-cross bases, index merge, conflict clustering parity | `git-merge` (major expansion) | `t/t6402`–`t/t6430` |
| C2 | `merge-tree`, `merge-base --octopus/--independent/--is-ancestor`, `cherry` | `git-merge` | `t/t6010`, `t/t6602` |
| C3 | `cherry-pick` / `revert` (sequencer subset) | new `git-sequencer` | `t/t3501`–`t/t3510` |
| C4 | `git rebase` (am- and merge-based backends; interactive later) | `git-sequencer` | `t/t3400` family |
| C5 | reflog: read/write `logs/<ref>`, `git reflog`, reflog-aware update/branch/delete | `git-refs` + `git-command` | `t/t1410` |
| C6 | packed-refs **write**, ref **locking/transactions**, `update-ref --stdin`/`--no-deref`, symref writes, worktree-specific refs | `git-refs` | `t/t1400`, `t/t3210` |
| C7 | `pack-refs`, `update-server-info`, `mktag`, `check-ref-format`, `stripspace`, `var`, `patch-id`, `check-mailmap`, `interpret-trailers`, `check-ignore`, `check-attr`, `show-index`, `unpack-file`, `diff-files`, `diff-index`, `diff-pairs`, `for-each-repo`, `url-parse` | `git-command` (small, independent) | respective `t/t` scripts; good parallel-track wins |
| C8 | `git config` command: read/write, scope resolution (system/global/local/worktree), includes, env vars, `--get*`/`--list`/`--unset`/`--edit` | `git-config` + `git-command/config.rs` | `t/t1300` |
| C9 | `notes`, `replace`, `worktree`, `bisect`, `stash`, `range-diff`, `rerere`, `blame`, `grep`, `archive` | various | respective suites; order: `stash` → `worktree` → `notes` → `bisect` → `grep` → `blame` → `range-diff` → `rerere` → `archive` |
| C10 | fsck completion: `--strict`, `--connectivity-only`, `--no-dangling`, `--full`, `--lost-found`, message-catalog parity | `git-command/fsck.rs` | `t/t1450` full pass |

## Phase D — On-disk format completeness (finishes the ODB story)

| # | Item | Crate | Gate |
|---|---|---|---|
| D1 | pack bitmaps (EWAH, `pack-bitmap` + MIDX bitmap), reachability queries | `git-odb` | `t/t5310` |
| D2 | cruft packs / `pack-mtimes` | `git-odb` | `t/t7704` |
| D3 | commit-graph **write** + chains + bloom **query** | `git-commitgraph` | `t/t5318`, `t/t5324` |
| D4 | MIDX: `RIDX`/`BTMP`/`BASE` chunks, incremental MIDX, `--preferred-pack` | `git-odb/midx` | `t/t5319`, `t/t5334` |
| D5 | `git index-pack` (incl. `--stdin`, thin-pack base resolution, `--verify`) | `git-command/index_pack.rs` | `t/t5302` |
| D6 | `git repack`, `gc`, `prune`, `prune-packed`, `maintenance`, `pack-redundant`, `replay` | `git-command` | `t/t7700`–`t/t7704`, `t/t5304` |
| D7 | sha1↔sha256 object conversion (`compatObjectFormat`, `gpgsig`↔`gpgsig-sha256`, LMAP loose-object map) | `git-odb`/`git-hash` | `t/t1016` |

## Phase E — Network & transport

Self-contained subsystem; start only after D6 (needs repack/gc context for
maintenance-on-fetch behavior parity, and D1 bitmaps for negotiation).

| # | Item | Crate | Gate |
|---|---|---|---|
| E1 | pkt-line framing + protocol v2 state machine | new `git-transport` | `t/t5500` |
| E2 | local/file transport → `ls-remote` | `git-transport`, `git-command` | `t/t5510`, `t/t5503` |
| E3 | `fetch-pack`/`upload-pack` negotiation + `fetch` | `git-transport` | `t/t5510`–`t/t5538` |
| E4 | `send-pack`/`receive-pack` + `push` (incl. refspec rules, force rules, atomic push) | `git-transport` | `t/t5528`–`t/t5541` |
| E5 | git:// daemon transport | `git-transport` | `t/t5570` |
| E6 | HTTP smart transport (`http-fetch`, `http-push`, `http-backend`) | `git-transport` | `t/t5539`, `t/t5541` |
| E7 | SSH transport | `git-transport` | `t/t5601` |
| E8 | `git clone` / `pull` (compose fetch + checkout + refs wiring) | `git-command` | `t/t5601`, `t/t5603` |
| E9 | `git remote`, `credential` stack (store/cache/helpers), `request-pull`, `fetch-pack` extras | `git-command` | `t/t5505`, `t/t5550` |
| E10 | `bundle` + `bundle-uri`, `fast-export` / `fast-import` | `git-command` | `t/t5607`, `t/t9300`, `t/t9350` |

## Phase F — Stretch / low priority (schedule last, defer freely)

`verify-commit`/`verify-tag` (needs signature parsing + verification stack),
`filter-branch`, `am` + `format-patch` completion, `mailinfo`/`mailsplit`,
`send-email`, `imap-send`, `mergetool`, `difftool`, `daemon`, `scalar`,
`backfill`, `diagnose`, `history`, `repo`, `bugreport`, `hook`, `instaweb`,
`gitweb`, `gui`/`citool`, `shell`, `sh-i18n`/`sh-setup`, `submodule` family,
`clean` filters (clean/smudge + core.autocrlf if not done in Phase A), sparse
index/split index/checkout (`sparse-checkout`, `maintenance` extras).

**Explicitly out of scope**: `cvs*`, `svn`, `p4`, `quiltimport`,
`archimport`, GUI tools — record as "not planned" in FOLLOWUPS.

## Parallel track — test infrastructure (run alongside every phase)

1. **`git-test` crate** (test-tool replacement, FOLLOWUPS B3): port the
   subcommands `t/` scripts need — start with `test-tool`'s most-used ones
   (`test-sha1`, `test-sha256`, `test-date`, `test-config`, `test-varint`,
   `test-zlib`, `test-delta`, `test-read-cache`, `test-ref-store`,
   `test-wildmatch`, `test-reach`). Do it **before Phase B** so `t/` scripts
   stop requiring a C build.
2. **Coverage gate** (FOLLOWUPS B7): wire `cargo llvm-cov --fail-under-lines 90`
   into `.github/workflows/rust-port.yml` for the crates touched by each PR.
3. **Fuzz targets** (FOLLOWUPS B4): `cargo-fuzz` for pack/idx/midx/
   commit-graph/index/config parsers as each phase lands; seed from
   `t/t5302`, `t/t5303`, `t/t5313` + `tests/fixtures`.
4. **Per-item test artifacts**: every item above must add (a) a crosswise
   suite if CLI-visible, (b) proptests for parsers/serializers, (c) a shim
   entry, (d) FOLLOWUPS/summary updates.

## Sequencing & parallelism

```
Phase A  ──────────────────────────────────────────────┐
  A2 first (context plumbing)                          │
  A1, A12, A10, A11, A3..A9 mostly independent        │
                                                      ▼
Phase B  (A8, A11 prerequisites)                    B1 → B2 → B3 → B4 → B5
  B6..B10 parallelizable after B3/B4                  │
                                                      ▼
Phase C  (C1..C4 algorithmic; C5..C10 parallel)       │
                                                      ▼
Phase D  (D1..D7 mostly parallel)                     │
                                                      ▼
Phase E  (E1 → E2 → E3 → E4 → E5..E8, E9/E10 parallel)│
Phase F  (any time, lowest priority) ◄────────────────┘
```

- **Single-agent critical path**: A2 → A8 → B3 → B4 → B5 → B7 → C1 → D6 → E8.
- **Parallelizable tracks**: A7/A9/A11 (independent crates), C5–C9 (small
  commands), D1–D5 (ODB formats), the whole test-infra track.
- **Milestones** (from gap-analysis §8): M1 = Phase A, M2 = Phase B,
  M3 = Phase C, M4 = Phase D, M5 = Phase E.

## Definition of done for the whole conversion

1. Every entry in `command-list.txt` that the repo declares in scope is
   routed in `scripts/shim-git` with a full crosswise suite.
2. All eight phase gates from `docs/plan/README.md` green on the full
   workspace, including the ≥90% coverage gate in CI.
3. `t/` scoreboard baseline at 100% for in-scope scripts (i.e. the shim
   routes nothing to C git anymore except declared out-of-scope commands).
4. FOLLOWUPS.md sections A and C empty; section B fully implemented.
