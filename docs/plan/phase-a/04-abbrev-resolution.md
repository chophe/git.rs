# A4 — Abbreviation & Name Resolution Helper

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §C
("commit-tree requires full-length object ids ... that needs
rev-parse/refs").

## Goal

One shared resolution function used by every command: turn a user-supplied
argument (full OID, abbreviated OID, ref name, `HEAD~3`, `@{yesterday}` in
Phase A scope: refname + abbreviated OID only) into an OID, with C git's
error messages, ambiguity rules, and exit codes.

## Current Rust state (observed)

- `git-refs` resolves plain ref names (Phase 7: `resolve_arg` feeds
  `rev-list`/`log`).
- No abbreviated-OID lookup anywhere; `commit-tree` requires full-length
  OIDs; `rev-parse --verify` exists but its resolution surface is minimal
  (see A5).

## C reference

- `get_oid()` family: `object-name.c` (disambiguation, `repo_get_oid`,
  `get_oid_basic`, core.abbrev handling, the "is ambiguous" warning),
  `sha1-name.c`-era behaviors now live there.
- Error text: `Documentation/git-rev-parse.txt` + `t/t1512-rev-parse-disambiguation.sh`.

## Deliverables

1. `git_revision::resolve(rectx, arg, flags) -> Result<Oid, ResolveError>`
   with the C precedence: full hex → ref → abbreviated hex (minimum 4 chars
   or `core.abbrev`/`--abbrev=<n>`), searching loose + packed objects.
2. Ambiguity: if a prefix matches multiple objects, C git errors with the
   "short object ID ... is ambiguous" message — replicate exactly (text and
   exit code 128 from `die`).
3. `<rev>~<n>` and `<rev>^` peel operators (first-parent-only; full range
   syntax stays in A5/A6).
4. A `ResolveError` type carrying C-parity stderr text so every caller
   produces byte-identical diagnostics.

## Sub-tasks (ordered)

1. Extract C's lookup order and messages from `object-name.c` and the
   expectations of `t/t1512` into crosswise test cases.
2. Implement loose-OID prefix iteration (`git_odb::LooseStore` already has
   `iter_oids()`; add a prefix-filtered walk over the fanout dirs).
3. Add packed prefix lookup over `PackIdx` (idx is sorted — binary search).
4. Implement `~<n>`/`^` peeling using `git-object` commit parsing.
5. Integrate into `commit-tree` (drop the full-length requirement), then
   expose to `rev-parse --verify` (A5) and `rev-list`/`log` args (A6).

## Test gates

- `t/t1512-rev-parse-disambiguation.sh`, `t/t1514-rev-parse-push-scope.sh`
  (relevant subset) via the shim.
- Proptest: for randomly generated repos, a truncated-at-n prefix resolves
  exactly when C git says it does (generate with `gen-fixtures`).

## Risks / notes

- Ambiguity messages include the object type and count — extract the exact
  format string from `object-name.c` rather than paraphrasing.
- `core.abbrev` / `--abbrev` minimum/maximum clamp rules (4..40) must match.
