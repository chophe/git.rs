# A5 — `rev-parse` Completion

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §D
Phase 7 "Remaining": `rev-parse` options (`--abbrev-ref`, `--short`,
`@{...}`, ranges `A..B`).

## Goal

Bring `rev-parse` to near-full option parity; it is the plumbing every other
command's argument handling leans on, and `t/` scripts use it pervasively.

## Current Rust state (observed)

Options implemented in `crates/git-command/src/rev_parse.rs`: `--verify`,
`--short`, `--abbrev-ref`, `--git-dir`, `--git-common-dir`,
`--show-toplevel`, `--is-inside-work-tree`.

## C reference

- `builtin/rev-parse.c` (the whole option table), `object-name.c` for
  `@{...}` expansions, `revision.c` for range parsing.
- Gate scripts: `t/t1500-rev-parse.sh` through `t/t1506`, `t/t1512`.

## Deliverables

1. Range syntax: `A..B` → `^A B`, `A...B` → symmetric difference with
   `--not` bookkeeping (pairs with A6's consumer side).
2. `@{upstream}` / `@{push}` (branch.<name>.remote/merge resolution),
   `@{-N}` (previous checkout), and date-based `@{...}` **deferred** to the
   reflog item C5 — only the syntax error message is needed now.
3. Remaining flags, grouped:
   - Output shaping: `--symbolic`, `--symbolic-full-name`, `--abbrev[=n]`,
     `--short[=n]`, `--quiet`, `--sq`, `--sq-quote`, `--local-env-vars`,
     `--parseopt` (script mode), `--keep-dashdash`, `--stop-at-non-option`,
     `--sticky-default`.
   - Repo shape: `--is-bare-repository`, `--is-shallow-repository`,
     `--show-cdup`, `--show-prefix`, `--show-superproject-working-tree`
     (submodule-dependent; may stub with C-parity error initially),
     `--absolute-git-dir`, `--shared-index-path`.
   - Verification: `--verify` with `--quiet`, `--default <arg>`,
     `--resolve-git-dir`, `--resolve-git-dir-quiet` equivalents
     (`--git-path`).
4. `--parseopt` mode: exact output contract (option-parsing shell helper) —
   heavily used by shell scripts in `t/`.

## Sub-tasks (ordered)

1. Ranges + `--symbolic`/`--abbrev` group (most-used in scripts).
2. Repo-shape flags (cheap, depends on A2's RepoContext).
3. `--sq`, `--local-env-vars`, `--keep-dashdash`/`--stop-at-non-option`.
4. `@{upstream}`/`@{push}`/`@{-N}`; date-form `@{...}` returns C's error
   message until reflog lands (C5).
5. `--parseopt` last (self-contained but has an exact byte contract tested
   by `t/t1502-rev-parse-parseopt.sh`).

## Test gates

- `t/t1500`, `t/t1501`, `t/t1502`, `t/t1503`, `t/t1506`, `t/t1512`.
- Crosswise suite covering: ranges output, `--symbolic-full-name` on packed
  and loose refs, `--is-bare-repository` in bare and non-bare clones,
  `--parseopt` round trip.

## Risks / notes

- `--parseopt` output is consumed by `eval` in shell — any quoting deviation
  breaks script tests silently; test with shell `eval` like `t/t1502` does.
- `--git-path` semantics depend on A2 (`GIT_DIR` and per-worktree rules).
