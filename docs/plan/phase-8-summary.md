# Phase 8 — Summary

Status: **partial** (merge-base + 3-way `merge-file` done and cross-verified;
`merge`/merge-ort, rename detection in merges, `merge-tree`, octopus,
`cherry-pick` deferred).

## What was implemented

### `git-merge` crate (new)
- `reachable` / `merge_bases` — commit reachability; merge bases = common
  ancestors not reachable from another common ancestor. (Fixed the domination
  direction: a commit is dominated when it is reachable from another common
  ancestor, so `merge-base(A, A) == A`.)
- `diff_changes` — base-relative change regions built from the Myers edit
  script.
- `merge3` — diff3-style 3-way merge: applies non-overlapping changes from
  both sides, emits git-style conflict markers
  (`<<<<<<< <ours-label>` / `=======` / `>>>>>>> <theirs-label>`) when both
  sides changed the same region differently. Exit-code distinction between
  clean and conflicted merges.

### Commands (`git-command`)
- `git merge-base [--all] <A> <B>` — resolves refs/oids, prints the merge
  base(s).
- `git merge-file [-p] <ours> <base> <theirs>` — 3-way file merge using the
  file names as conflict labels; exits 1 on conflict.

## Verified against real git (automated)

`git-command/tests/phase8_crosswise.rs` (2 tests):
- `merge-base` (single and `--all`) on a real merge DAG is identical to system
  git.
- `merge-file` (conflict and clean cases) produces byte-identical output and
  exit codes.

## Test suite

- 169/169 tests pass (up from 163); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase8 crosswise suite.

## Deferred / known limitations

- **`git merge`** (merge-ort, ~184K LOC): rename detection, directory/file
  conflicts, recursive criss-cross bases, index merging — not started.
- **`merge-tree`, `cherry-pick`, `revert`, `merge-base --octopus` /
  `--independent` / `--is-ancestor`** — not implemented.
- **Conflict clustering**: git treats *adjacent* changes as conflicts (xdl_merge
  change context); ours only conflicts on *identical* regions, so close-but-not-
  identical edits may merge where git conflicts. Crosswise tests use clearly
  separated (clean) and identical-region (conflict) cases.
- **`merge-file --diff3`, `-L` labels, `--marker-size`** — `-L`/`--diff3` parsed
  but not honored.
- **Binary merges and mode/symlink handling** — not implemented.