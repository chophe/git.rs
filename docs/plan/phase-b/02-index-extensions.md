# B2 — Index Extensions

**Item**: Index extensions: cache-tree (`TREE`), REUC; index v3/v4 read/write
**Crate**: `git-index`
**Gate**: `t/t0060`, `t/t3007`

## Description

Complete the index (staging area) implementation to support all index format
versions and extensions. This includes cache-tree for fast tree traversal,
REUC (Resolve Undo) entries for merge recovery, and v3/v4 format support.

## Requirements

- Read and write index v2, v3, and v4 formats
- Support cache-tree extension (`TREE`) for efficient tree comparison
- Support REUC (Resolve Undo) extension for merge conflict resolution state
- Support untracked cache extension (`UNTR`)
- Support file system monitor extension (`FSMN`)
- Handle index entry flags correctly (assume-valid, extended flags, name length)
- Support index v4 (reduced entry size with path compression)
- Validate index checksums on read
- Handle corrupted index gracefully

## Test Gate

- `t/t0060-pathspec.sh` passes through the shim
- `t/t3007-merge-rename.sh` passes through the shim
- Crosswise suite: byte-identical index format with C git

## Dependencies

- Phase A2 for repository context
- `git-hash` for checksum verification

## Implementation Notes

- Match C git's `read_index()` and `write_index()` in `cache.h` / `read-cache.c`
- Cache-tree must be invalidated correctly on index changes
- REUC entries must be written during merge conflicts
- Handle endianness correctly (index is big-endian)
