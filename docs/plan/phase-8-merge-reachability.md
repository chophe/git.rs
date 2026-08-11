# Phase 8 — Merge Machinery & Reachability

## Goal

Implement commit reachability (merge-base, bounded walks, octopus) and the
merge machinery (rename detection + 3-way content merge with merge-ort-grade
correctness, diff3 conflict markers, `merge-file`). This is the hardest
correctness surface outside the revision walk.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-merge::CommitReach` — merge-base, bounded walks, octopus | `commit-reach.c` |
| `git-merge::CommitReach` — `PriQueue` | `prio-queue.c`, `prio-queue.h` |
| `git-merge::Merge` — 3-way content merge (lines) | `xdiff/xmerge.c`, `merge-blobs.c`, `merge-ll.c` |
| `git-merge::Merge` — merge-ort rename detection + conflict resolution | `merge-ort.c` (~184K), `merge-ort.h` |
| `git-merge::Merge` — diff3 markers | `xdiff/xmerge.c` (XDL_MERGE_* flags) |
| `git-merge::Merge` — `MergeFile` | `builtin/merge-file.c` |
| `git-merge::Reachable` | `reachable.c`, `bisect.c` (marking) |

## Commands enabled

- `git merge-base`, `git merge-tree`, `git merge`, `git merge-file`,
  `git cherry-pick`/`git revert` (basic, no reword/rebase)

## Fully automated test plan

### Unit
- **Merge-base:** on known DAGs incl. criss-cross, octopus, and two-base
  cases; `--all`/`--octopus`/`--independent`/`--is-ancestor`.
- **3-way blob merge:** all twelve result cases from `t6403` (ours/theirs both
  changed, one side deleted, binary, rename, modes, etc.), via `merge-file`
  semantics.
- **Rename detection in merge:** renamed-added, renamed-deleted,
  rename/rename (same + different target), directory/file conflicts.
- **Conflict markers:** diff3 vs `merge.conflictStyle` marker layout
  (`t6427`).
- **Symlink / exec-bit / mode conflicts** (`t6405`, `t6411`).
- **Criss-cross and recursion** (`t6401`, `t6431`).

### Property
- Random DAGs → `git merge-base --all` == C.
- Random base/ours/theirs triples → 3-way result (merged lines + conflict
  markers + exit status) == C `git merge-file` output.

### Differential
- Run C `git merge` and Rust `git merge` on the same repos → resulting tree,
  index, and conflict markers identical.
- `git merge-tree --write-tree` output parity (Phase 9 gate).

### `git-test` additions
- `test-reach` (already in Phase 4), `test-merge-blobs` if needed.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-merge.
- **t/ scripts pass 100%:** `t6010` (merge-base), `t6400`–`t6439`,
  `t6427` (diff3 markers), `t7600s` (merge), `t3500s` (cherry-pick/revert).

## Risks

- `merge-ort.c` (~184K) is the largest and subtlest file in git's core;
  incremental porting behind the differential gate, starting with
  `merge-file`/`xmerge`, then merge-ort rename/conflict machinery.
- Conflict resolution ordering and "same content" detection must match C
  exactly for identical conflict markers.
- Recursive/criss-cross base computation and index-state merging must match C
  or `git merge` results diverge.
