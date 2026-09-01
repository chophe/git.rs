# B7 — `unpack-trees` + `git checkout` / `switch` / `restore` / `reset`

**Item**: `unpack-trees` port + `git checkout` / `switch` / `restore` / `reset` (mixed/soft/hard)
**Crate**: new `git-worktree` crate + `git-command`
**Gate**: `t/t2000`–`t/t2030`, `t/t7102`

## Description

Port the working tree manipulation commands. This includes the core `unpack-trees`
algorithm and the `checkout`, `switch`, `restore`, and `reset` commands that use it.

## Requirements

### `unpack-trees` (internal)
- One-way unpack (checkout single tree)
- Two-way unpack (branch switch)
- Three-way unpack (merge)
- Handle directory/file conflicts
- Handle delete/modify conflicts
- Support sparse checkout patterns
- Support overlay vs. checkout strategies

### `git checkout`
- Checkout branch: `git checkout <branch>`
- Create and checkout: `git checkout -b <new-branch>`
- Force checkout: `git checkout -f`
- Detached HEAD: `git checkout <commit-ish>`
- Checkout paths: `git checkout <tree-ish> -- <paths>`
- Merge during checkout: `git checkout -m`
- Quiet mode: `git checkout -q`
- Progress reporting: `--progress` / `--no-progress`
- Force: `--force` / `-f`
- `--ours` / `--theirs` for conflict resolution
- `--conflict=<style>` for conflict style
- `--overwrite-ignore` / `--no-overwrite-ignore`
- `--ignore-skip-worktree-bits`

### `git switch`
- Switch branch: `git switch <branch>`
- Create and switch: `git switch -c <new-branch>`
- Detached: `git switch --detach <commit-ish>`
- Force: `--force` / `-f` / `--discard-changes`
- Merge: `--merge`
- Conflict style: `--conflict=<style>`
- Quiet: `--quiet` / `-q`
- Force create: `--force-create` / `-C`
- Orphan branch: `--orphan <new-branch>`

### `git restore`
- Restore working tree: `git restore <paths>`
- Restore from stage: `git restore --staged <paths>`
- Restore from source: `git restore --source=<tree-ish> <paths>`
- Restore both: `git restore --worktree --staged <paths>`
- Patch mode: `--patch` / `-p`
- Overlay mode: `--overlay` / `--no-overlay`
- Ignore unmerged: `--ignore-unmerged`
- Progress: `--progress` / `--no-progress`

### `git reset`
- Soft reset: `git reset --soft <commit>`
- Mixed reset (default): `git reset [--mixed] <commit>`
- Hard reset: `git reset --hard <commit>`
- Keep: `git reset --keep <commit>`
- Merge reset: `git reset --merge <commit>`
- Path reset: `git reset <tree-ish> -- <paths>`
- Patch reset: `--patch` / `-p`
- Quiet: `--quiet` / `-q`
- Refresh index: `--no-refresh` / `--refresh`
- Intent to add: `--intent-to-add`

## Test Gate

- `t/t2000-checkout-cache.sh` passes through the shim
- `t/t2010-checkout-ambiguous.sh` passes through the shim
- `t/t2020-checkout-tracking.sh` passes through the shim
- `t/t2030-parallel-checkout.sh` passes through the shim
- `t/t7102-reset.sh` passes through the shim
- Crosswise suite: byte-identical working tree state

## Dependencies

- Phase A2 (repository context)
- Phase A8 (diff engine for change detection)
- B2 (index extensions)
- B4 (`read-tree` for tree reading)
- `git-object` for tree/commit objects

## Implementation Notes

- Match C git's `unpack-trees.c` implementation
- Handle `.git/info/sparse-checkout` patterns
- Support parallel checkout workers
- Handle checkout progress callbacks
- Handle skip-worktree and assume-unchanged bits
- Support `checkout.workers` config
