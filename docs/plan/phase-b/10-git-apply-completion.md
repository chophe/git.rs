# B10 — `git apply` Completion

**Item**: `git apply` completion: `--3way`, `--index`, `--reject`, whitespace options, binary patches, `\ No newline`
**Crate**: `git-command/apply.rs`
**Gate**: `t/t4103`–`t/t4137`

## Description

Complete the `git apply` command with full option parity. This includes
three-way merge, index updates, reject handling, whitespace options, and
binary patch support.

## Requirements

### Core options
- Apply patch from stdin or file
- Support `-p<n>` to strip path components
- Support `-R` / `--reverse` to reverse apply
- Support `--check` to check if patch applies
- Support `--stat` to show stats without applying
- Support `--numstat` to show numeric stats
- Support `--summary` to show summary
- Support `-v` / `--verbose` for verbose output
- Support `-q` / `--quiet` for quiet mode

### Index options
- Support `--index` to apply to both index and working tree
- Support `--cached` to apply to index only
- Support `--build-fake-ancestor` for fake ancestor
- Support `--exclude=<path>` to exclude paths
- Support `--include=<path>` to include paths
- Support `--unsafe-paths` for unsafe path handling

### Three-way merge
- Support `--3way` / `-3` for three-way merge
- Support `--3way` with index for conflict resolution
- Handle merge conflicts gracefully
- Support `--build-fake-ancestor` for 3-way base

### Whitespace options
- Support `--whitespace=<mode>` (nowarn, warn, fix, error, error-all)
- Support `-w` / `--ignore-whitespace` to ignore whitespace
- Support `--whitespace=fix` to fix whitespace
- Support `--whitespace=error` to error on whitespace
- Support `--whitespace=error-all` to error on all whitespace
- Support `--inaccurate-eof` to handle inaccurate EOF

### Reject options
- Support `--reject` to apply what can be applied, leave rejects
- Support `--reject` with `.rej` files for rejected hunks

### Binary options
- Support `--binary` / `--allow-binary-replacement` for binary patches
- Support binary patch detection and application
- Handle binary diff format

### Other options
- Support `--recount` to recount lines
- Support `--directory=<root>` to prepend root
- Support `--exclude` / `--include` for path filtering
- Support `--verbose` for detailed output
- Support `--dry-run` / `--check` for dry run
- Handle `\ No newline at end of file` correctly
- Support context-less patches
- Support `--unidiff-zero` to allow zero context
- Support `--apply` to apply even if not detected as patch
- Support `--numstat` for numeric stats
- Support `--summary` for summary

## Test Gate

- `t/t4103-apply-nonl.sh` passes through the shim
- `t/t4104-apply-boundary.sh` passes through the shim
- `t/t4105-apply-binary.sh` passes through the shim
- `t/t4106-apply-binary-2.sh` passes through the shim
- `t/t4107-apply-binary-3.sh` passes through the shim
- `t/t4108-apply-binary-4.sh` passes through the shim
- `t/t4109-apply-binary-5.sh` passes through the shim
- `t/t4110-apply-binary-6.sh` passes through the shim
- `t/t4111-apply-binary-7.sh` passes through the shim
- `t/t4112-apply-binary-8.sh` passes through the shim
- `t/t4113-apply-binary-9.sh` passes through the shim
- `t/t4114-apply-binary-10.sh` passes through the shim
- `t/t4115-apply-binary-11.sh` passes through the shim
- `t/t4116-apply-binary-12.sh` passes through the shim
- `t/t4117-apply-binary-13.sh` passes through the shim
- `t/t4118-apply-binary-14.sh` passes through the shim
- `t/t4119-apply-binary-15.sh` passes through the shim
- `t/t4120-apply-binary-16.sh` passes through the shim
- `t/t4121-apply-binary-17.sh` passes through the shim
- `t/t4122-apply-binary-18.sh` passes through the shim
- `t/t4123-apply-binary-19.sh` passes through the shim
- `t/t4124-apply-binary-20.sh` passes through the shim
- `t/t4125-apply-binary-21.sh` passes through the shim
- `t/t4126-apply-binary-22.sh` passes through the shim
- `t/t4127-apply-binary-23.sh` passes through the shim
- `t/t4128-apply-binary-24.sh` passes through the shim
- `t/t4129-apply-binary-25.sh` passes through the shim
- `t/t4130-apply-binary-26.sh` passes through the shim
- `t/t4131-apply-binary-27.sh` passes through the shim
- `t/t4132-apply-binary-28.sh` passes through the shim
- `t/t4133-apply-binary-29.sh` passes through the shim
- `t/t4134-apply-binary-30.sh` passes through the shim
- `t/t4135-apply-binary-31.sh` passes through the shim
- `t/t4136-apply-binary-32.sh` passes through the shim
- `t/t4137-apply-binary-33.sh` passes through the shim
- Crosswise suite: byte-identical patch application results

## Dependencies

- Phase A2 (repository context)
- Phase A8 (diff engine for patch parsing)
- B2 (index extensions for `--index`)
- B4 (`read-tree` for three-way merge)

## Implementation Notes

- Match C git's `apply.c` implementation
- Handle unified diff format parsing
- Handle context diff format parsing
- Handle binary diff format (literal and delta)
- Handle patch fuzz factor
- Handle offset line counts
- Support `git apply --3way` with merge conflict markers
