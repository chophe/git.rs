# B4 — `git write-tree`, `read-tree`

**Item**: `git write-tree`, `read-tree` (one/two/three-way)
**Crate**: `git-command`
**Gate**: `t/t1000`, `t/t2000`

## Description

Port the tree object manipulation commands. `write-tree` creates a tree object
from the current index, while `read-tree` reads tree objects into the index.

## Requirements

### `git write-tree`
- Create tree object from current index state
- Support `--missing-ok` to allow missing objects
- Support `--prefix=<subdirectory>` for subtree writes
- Return the SHA of the created tree object

### `git read-tree`
- Read tree object into index (one-way)
- Support two-way merge (`-m` flag) for branch switching
- Support three-way merge (`-m` with `--trivial-merge` / `--aggressive`)
- Support `--reset` to reset index to a tree
- Update working tree with `-u` flag
- Support `--dry-run` / `-n` for dry run
- Support `--exclude-per-directory` (deprecated, but handle gracefully)
- Support `--index-output` for writing to alternate index file
- Handle sparse checkout patterns with `--sparse-checkout` (can be basic)

## Test Gate

- `t/t1000-read-tree-m-3way.sh` passes through the shim
- `t/t2000-checkout-cache.sh` passes through the shim
- Crosswise suite: byte-identical tree objects and index state

## Dependencies

- Phase A2 (repository context)
- B2 (index extensions)
- `git-object` for tree object format

## Implementation Notes

- Match C git's `write_tree()` in `builtin/write-tree.c`
- Match C git's `read_tree()` in `builtin/read-tree.c`
- Tree object format: sorted entries with mode, name, SHA
- Handle tree parsing for two-way and three-way merge scenarios
