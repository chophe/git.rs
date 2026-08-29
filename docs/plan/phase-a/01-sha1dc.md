# A1 — sha1dc: Collision-Detecting SHA-1

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §A1.

## Goal

Replace the hand-written standard SHA-1 in `git-hash` with the SHA-1
collision-detection algorithm (sha1dc) so that known-colliding inputs are
rejected at hash time, matching C git's safety behavior. At the end,
`HashAlgorithm::Sha1.hasher().is_safe()` must return `true`
(currently asserted `false` in `crates/git-hash/src/lib.rs:450`).

## Current Rust state (observed)

- `crates/git-hash` uses a hand-written standard SHA-1.
- `CryptoHasher::is_safe()` returns `false` for SHA-1 (only SHA-256 is safe).
- There is an existing test asserting `!HashAlgorithm::Sha1.hasher().is_safe()`
  — this test must be inverted as part of this item.

## C reference

- `sha1dc/` (vendored upstream library: `sha1.c`, `sha1.h`, `ubc_check.c`,
  licensing in `sha1dc/LICENSE.txt`) plus the git glue `sha1dc_git.c/.h`.
- The in-tree `sha1collisiondetection/` submodule directory exists but is
  empty in this checkout — port from the vendored `sha1dc/` sources.
- Test oracle: `t/t0013-sha1dc.sh`.

## Deliverables

1. Pure-Rust port of sha1dc (SHA-1 with the ubiquity-in-collision checks and
   `ubc_check` fast path), no unsafe code.
2. Wire it into `CryptoHasher::Sha1`: normal inputs hash identically to the
   current SHA-1 (all existing golden/crosswise hashes must be unchanged);
   colliding input must return an error rather than a wrong digest.
3. `is_safe()` returns `true` for SHA-1; invert the existing negative test.
4. A collision-block test mirroring `t/t0013-sha1dc.sh` semantics: feed the
   SHAttered-collision blocks and assert the error path + C-parity error
   message/exit code.

## Sub-tasks (ordered)

1. Read `sha1dc/sha1.h` public surface; define the Rust equivalent API
   (compress/streaming/finalize, detection flag out).
2. Port `ubc_check.c` (the "safe" fast path) and `sha1.c` message-schedule
   checks; keep a proptest asserting round-trip digest equality with the
   current standard SHA-1 on arbitrary inputs.
3. Add the "unavoidable bit conditions" detection to `CryptoHasher::Sha1`.
4. Port the C-side error reporting (`sha1dc_git.c` maps detection to
   `die("SHA-1 collision detected ...")`) to the `git-hash` error type, and
   thread it through `hash-object`/`index`/pack writing call sites.
5. Add the collision fixture test; wire `t/t0013` into the scoreboard set.
6. Update FOLLOWUPS.md §A1 to DONE.

## Test gates

- `t/t0013-sha1dc.sh` via the shim.
- Existing `git-hash` proptests (digest stability on arbitrary input) and the
  new collision test.

## Risks / notes

- sha1dc is performance-sensitive; keep the `ubc_check` fast path faithful or
  hashing will regress measurably. The proptest in step 2 is the regression
  guard against subtly-wrong output.
- SHA-256 paths are untouched.
