---
last_mapped_commit: a1577158665b621947c436f66115b03e91f199f7
last_mapped_at: 2026-09-25
---
# Codebase Concerns

**Analysis Date:** 2026-09-25

## Tech Debt

**Shim/dispatch drift (`index-pack` not routed to Rust):**

- Issue: `git-command` dispatches `"index-pack"` (`crates/git-command/src/lib.rs:422`) but `scripts/shim-git` does not list `index-pack` in its `case` routing, so the `t/` scoreboard path sends `index-pack` to system C git and never exercises the Rust implementation (`crates/git-command/src/index_pack.rs`).
- Files: `scripts/shim-git`, `crates/git-command/src/lib.rs`, `crates/git-command/src/index_pack.rs`
- Impact: Crosswise confidence for `index-pack` is overstated; regressions in the Rust path go undetected by the shim-driven suite.
- Fix approach: Add `index-pack` to the `case` list in `scripts/shim-git`; add a CI assertion that every dispatch key (except `version`, owned by `git-cli`) appears in the shim.

**Hand-rolled CLI parsing per command:**

- Issue: Each of the ~40 command modules parses `args` with bespoke loops (e.g. `crates/git-command/src/apply.rs:25-28`, `crates/git-command/src/clean.rs:30-91`). There is no shared option-parsing layer, so C-git option compatibility (`--no-` negation, combined shorts, `--opt=val` vs `--opt val`) drifts per command.
- Files: `crates/git-command/src/*.rs` (notably `apply.rs`, `clean.rs`, `diff.rs`, `checkout.rs`)
- Impact: New flags reintroduce the same parsing bugs; `--diff-filter` lowercase already diverges from C semantics (see `docs/plan/FOLLOWUPS.md` A8).
- Fix approach: Introduce a small shared argv parser in `git-command` (or adopt `lexopt`) and migrate commands one at a time, keeping crosswise suites green.

**God modules with `clippy::too_many_arguments` suppressions:**

- Issue: `crates/git-command/src/init.rs` (1723 lines), `crates/git-command/src/checkout.rs` (1277 lines), `crates/git-command/src/checkout_core.rs` (1083 lines) concentrate unrelated logic. Six `#[allow(clippy::too_many_arguments)]` sites paper over missing parameter structs.
- Files: `crates/git-command/src/init.rs:1375`, `crates/git-command/src/checkout.rs:589,755,1082`, `crates/git-command/src/worktree.rs:114`, `crates/git-command/src/diff.rs:200`
- Impact: High change-blast-radius; checkout/init fixes risk collateral breakage.
- Fix approach: Extract context structs (e.g. `CheckoutCtx`, `InitOpts`) and split `init.rs` template-copy vs repo-layout logic; remove `allow` attributes as structs land.

**Ref/index locking is pid-tempfile + rename with no inter-process exclusion:**

- Issue: `git-refs` writes via `path.with_extension(format!("lock.{}", std::process::id()))` (`crates/git-refs/src/lib.rs:110-113`); `git-index` does the same (`crates/git-index/src/lib.rs:286-288`). No `flock`/`lockfile` protocol, no stale-lock cleanup, no directory `fsync` after rename. Concurrent writers (e.g. two `update-ref` processes) can interleave temp files and last-writer-wins silently. Predictable temp names in the target directory are also symlink-precreation sensitive (`File::create` follows symlinks; contrast the safer counter-suffixed scheme in `crates/git-odb/src/lib.rs:202-206`).
- Files: `crates/git-refs/src/lib.rs:101-120`, `crates/git-index/src/lib.rs:283-290`, `crates/git-command/src/commit.rs:660-676` (reflog append, below)
- Impact: Lost ref updates under concurrency; divergence from C git's `lock-ref` semantics (multi-ref atomicity, transactions — both listed as Phase 7 remaining work in `docs/plan/FOLLOWUPS.md`).
- Fix approach: Implement C-compatible `.lock` files with `O_CREAT|O_EXCL`, stale-lock detection, and `fsync` of file + parent dir; add a concurrency stress test.

**Reflog append silently swallows all errors:**

- Issue: `append_reflog` ignores `create_dir_all`, `open`, and `write_all` failures via `let _ =` (`crates/git-command/src/commit.rs:660-676`).
- Files: `crates/git-command/src/commit.rs:660-676`
- Impact: Silent reflog loss (e.g. read-only `logs/` dir) with exit 0; debugging ref history gaps is hard.
- Fix approach: Propagate errors as warnings on stderr (matching C git's warn-and-continue) or fail the command; never discard silently.

**Non-UTF8 config silently becomes empty:**

- Issue: `parse_into` maps undecodable bytes to `""` (`crates/git-config/src/lib.rs:121`: `std::str::from_utf8(data).unwrap_or("")`), discarding the entire file content without error.
- Files: `crates/git-config/src/lib.rs:120-121`
- Impact: A config file with any non-UTF8 byte is treated as empty instead of erroring like C git; `include.path` and identity values silently vanish.
- Fix approach: Operate on bytes (git config is byte-oriented) or return `ConfigError::BadLine`; add a property test with arbitrary bytes asserting no silent emptying.

**Stale root `Cargo.toml` / `Cargo.lock`:**

- Issue: Root `Cargo.toml` still defines the leftover `gitcore` staticlib crate (edition 2018, rust 1.49) while the real workspace lives at `crates/Cargo.toml`. A root `Cargo.lock` (138 bytes) is also stale. Running `cargo` from the repo root builds the wrong project.
- Files: `Cargo.toml`, `Cargo.lock`, `crates/Cargo.toml`
- Impact: Contributor confusion; CI and docs must always pass `--manifest-path crates/Cargo.toml` (they do, but any new automation that forgets silently tests nothing).
- Fix approach: Delete the root `Cargo.toml`/`Cargo.lock` stub or convert the root into a virtual workspace that includes `crates/`; document that all cargo commands run from `crates/`.

**Committed C build artifacts pollute the tree:**

- Issue: Compiled C artifacts (`.o` files, `.depend/`, `bin-wrappers/`, `git*` binaries, `scalar`) are committed in the working tree alongside the Rust build dir `crates/target/`, risking confusion between C-built `git` and Rust-built `crates/target/debug/git`.
- Files: `*.o`, `.depend/`, `bin-wrappers/`, `git`, `scalar` (repo root)
- Impact: Bloated checkout, stale-oracle risk (crosswise suites shell out to system git, but a stray `./git` on `PATH` can shadow it), noisy `git status`.
- Fix approach: Remove committed artifacts; extend `.gitignore` (already ignores `/git`, `/crates/target/`) to cover `*.o`, `.depend/`, `bin-wrappers/` outputs that are currently tracked.

**`watch-and-commit.sh` is a footgun:**

- Issue: The script auto-stages everything and commits with `--no-verify` on a timer (`watch-and-commit.sh` at root). Running it bypasses hooks and fabricates history.
- Files: `watch-and-commit.sh`
- Impact: Accidental mass commits including secrets, build artifacts, or `scoreboard.json` churn.
- Fix approach: Never run it (per `AGENTS.md`); consider deleting it or gating on an explicit opt-in env var.

## Known Bugs

**`git apply` has no path-traversal guard (writes outside the tree):**

- Symptoms: A patch naming `../evil.txt` or `/abs/path` is written verbatim via `std::fs::write(&path, …)` after `strip_path`, with `create_dir_all` on the parent (`crates/git-command/src/apply.rs:176-184`, `:196-197`, `:246-249`). The sibling worktree layer explicitly rejects escapes (`crates/git-worktree/src/lib.rs:78-96` `resolve_inside`), but `apply` never calls it.
- Files: `crates/git-command/src/apply.rs:164-250`
- Trigger: `git apply` a crafted patch containing `diff --git a/../evil b/../evil` (new-file, delete, or modify hunk).
- Workaround: None in-code; only apply trusted patches.
- Fix approach: Route every `apply` target through `git_worktree::resolve_inside`-equivalent confinement and reject absolute paths; add a crosswise test with a malicious patch asserting refusal.

**Rename scoring is an approximation, not C `diffcore-rename`:**

- Symptoms: Post-edit renames may be detected/scored differently from C git; `--diff-filter` lowercase acts as exclude while C treats it as undocumented/unsupported; `status` typechange (`T`) classification is mode-derived (`docs/plan/FOLLOWUPS.md` A8 known deviations).
- Files: `crates/git-command/src/diff.rs`, `crates/git-command/src/patch.rs`, `crates/git-diff/src/tree.rs`, `crates/git-command/src/status.rs`
- Trigger: Similarity renames with edits; lowercase `--diff-filter` usage.
- Workaround: Exact renames and standard filters match; crosswise suites cover the byte-identical subset.
- Fix approach: Tracked as A8 follow-up; port `diffcore-rename` scoring faithfully when tackling renames.

**`hash-object` outside a repo always uses SHA-1:**

- Symptoms: Matches git's default today but skips object-format discovery; `-t`/`--stdin` parity against `t1007` unconfirmed (`docs/plan/FOLLOWUPS.md` §C).
- Files: `crates/git-command/src/hash_object.rs`
- Fix approach: Resolve algorithm from repo config/`extensions.objectFormat` with CLI override; verify against `t1007`.

**MIDX preferred-pack is first-sorted-wins; chains rejected:**

- Symptoms: Multi-pack reads pick the first sorted pack instead of `--preferred-pack`; incremental MIDX chains and `RIDX`/`BTMP`/`BASE` chunks are rejected or ignored; commit-graph chains are rejected at parse (`crates/git-odb/src/pack/midx.rs:87`, `crates/git-commitgraph/src/commit_graph.rs:112`).
- Files: `crates/git-odb/src/pack/midx.rs`, `crates/git-commitgraph/src/commit_graph.rs:99-112`, `crates/git-commitgraph/src/bloom.rs:6` (query unimplemented)
- Fix approach: Phase 3 follow-ups in `docs/plan/FOLLOWUPS.md` §D; implement chunk readers before writers.

## Security Considerations

**Arbitrary file write via `git apply` (see Known Bugs):**

- Risk: Patch-controlled absolute/escape paths lead to arbitrary file creation, overwrite, and deletion (`remove_file` on delete hunks, `crates/git-command/src/apply.rs:188-193`).
- Files: `crates/git-command/src/apply.rs:175-250`
- Current mitigation: None — no confinement, no symlink check.
- Recommendations: Confine with `resolve_inside`, refuse absolute paths and `..` escapes, refuse symlinks-as-directories on write, add hostile-patch tests. Treat as the highest-priority security fix.

**Hook execution is currently dead code — wire carefully when enabling:**

- Risk: `git-hooks` defines discovery + `run_hook` (`crates/git-hooks/src/lib.rs:86-100`, `:130-`), but no command module calls it (no `run_hook`/`find_hook` callers outside `git-hooks` itself). When `commit`/`merge`/`push` start invoking hooks, any executable `$GIT_DIR/hooks/*` runs with the user's privileges — expected git behavior, but a ported-then-enabled hook surface without C's `core.hooksPath`, `--no-verify`, and safe-path gating is an RCE vector via cloned repos.
- Files: `crates/git-hooks/src/lib.rs`, `crates/git-command/src/commit.rs` (no hook call sites yet)
- Current mitigation: Hooks never execute today.
- Recommendations: When wiring hooks, port `core.hooksPath`, `--no-verify`/`--no-post-rewrite` policy per command, and C's safe-directory behavior together; never enable execution without the skip policy in the same change.

**Credential handling is deliberately log-safe (positive pattern to preserve):**

- Risk: Helper protocol values include secrets; debug-logging a credential would leak tokens.
- Files: `crates/git-credentials/src/lib.rs:14` (errors never carry secret material), `:93-113` (redacting `Display`/`Debug` via `is_secret_key`)
- Current mitigation: Redaction by design; malformed lines rejected, never silently kept (`:77`, `:86`).
- Recommendations: Keep the invariant — no `{:?}` on raw credentials, extend `is_secret_key` coverage as new attributes are added, and ensure future transport code reuses this type instead of raw strings.

**Ref/ODB temp-file predictability:**

- Risk: `lock.<pid>` and `tmp_obj_<pid>_<counter>` names are predictable; `File::create` truncates/follows existing paths, so a pre-planted symlink in `.git/refs/` or `objects/` could redirect a write.
- Files: `crates/git-refs/src/lib.rs:110`, `crates/git-index/src/lib.rs:286`, `crates/git-odb/src/lib.rs:202-211` (ODB at least `sync_all`s before rename)
- Current mitigation: ODB uses atomic rename + `sync_all`; refs/index use rename without `O_EXCL`.
- Recommendations: Create temp files with `O_CREAT|O_EXCL` (retry on collision), verify the temp is a fresh regular file before rename, `fsync` file and parent dir.

**`git clean` / `checkout` / `reset --hard` are destructive by design — guard parity matters:**

- Risk: Any divergence in untracked/ignored classification or dirty-guard logic turns a safe command into data loss. `clean` refuses to remove the CWD (`crates/git-command/src/clean.rs:241-244`) and honors `clean.requireForce` (`:143-149`), but `-x`/`-X`/nested-repo collapsing (`:160-162`, `:251-254`) and checkout's `verify_uptodate` (`crates/git-command/src/checkout_core.rs:369-419`) are hand-ported C logic with known deferred cases (`-m`, `--merge/--keep`, submodules per FOLLOWUPS B7).
- Files: `crates/git-command/src/clean.rs`, `crates/git-command/src/checkout_core.rs:340-460`, `crates/git-command/src/checkout.rs:654-657,1253`, `crates/git-command/src/reset.rs`
- Current mitigation: Dirty guards + dry-run paths exist and are crosswise-tested for the covered subset.
- Recommendations: Expand crosswise coverage before touching guard logic; treat every deferred guard case as a data-loss risk, not a cosmetic gap.

**No network code yet — keep it that way until the trust model is ported:**

- Risk: `git-transport` owns only endpoint classification + negotiation state machine (`crates/git-transport/src/lib.rs`); `git-protocol` handles packet-line framing (`crates/git-protocol/src/lib.rs:25-64`); no socket/SSH/HTTP exists. Fetch/push/clone/daemon are Phase 10+ stretch (not started).
- Files: `crates/git-transport/src/lib.rs`, `crates/git-protocol/src/lib.rs`, `crates/git-credentials/src/lib.rs`
- Current mitigation: Nothing dials out, so no SSRF/credential-exfil surface exists yet.
- Recommendations: When transport lands, port credential-helper scoping, `http.*` ssl/redirect controls, and daemon allowlists together with the dialer — not after.

## Performance Bottlenecks

**`status` content-compares instead of racy-clean:**

- Problem: Worktree freshness falls back to hashing file contents rather than C's racy-clean stat-cache shortcut (FOLLOWUPS Phase 6 remaining).
- Files: `crates/git-command/src/status.rs:467`, `crates/git-command/src/worktree.rs`
- Cause: `cache-tree`/`stat-dirty` handling incomplete.
- Improvement path: Port racy-clean + `TREE` extension writeback (B2 partial) and measure on a large worktree before optimizing elsewhere.

**Revision walks lack commit-graph acceleration:**

- Problem: Walks parse individual commits; no generation-number ordering, no bloom-assisted path limiting, no bitmaps (all Phase 3 remaining per FOLLOWUPS §D).
- Files: `crates/git-revision/src/rev_info.rs:197-218`, `crates/git-commitgraph/src/`
- Cause: `commit-graph write`, bloom query, and EWAH bitmaps unimplemented.
- Improvement path: Implement in dependency order (parse → walk → bloom query → bitmaps), each with a bench vs C git on a mid-size repo.

**Pack object reads materialize whole objects:**

- Problem: `read_raw` inflates the full object into memory (`crates/git-odb/src/lib.rs:227-243`; documented as bounded-by-object-size with a paging note for bulk callers). Large blobs spike RSS during `rev-list --objects`-style traversals.
- Files: `crates/git-odb/src/lib.rs:224-243`, `crates/git-odb/src/pack/file.rs`
- Improvement path: Streaming inflate for large blobs; page bulk walks via size queries first (as the doc comment suggests).

**Delta search is sliding-window by construction:**

- Problem: `pack-objects` delta compression does a same-type window search (`crates/git-odb/src/pack/delta.rs:682`, `crates/git-odb/src/pack/write.rs:462`, `crates/git-command/src/pack_objects.rs`) — correct port of C behavior but inherently O(window × size).
- Files: `crates/git-odb/src/pack/delta.rs`, `crates/git-odb/src/pack/write.rs`, `crates/git-command/src/pack_objects.rs`
- Improvement path: Match C defaults first (done, A13); only then consider indexing (C's own tradeoff); keep `--window/--depth` respected.

## Fragile Areas

**Checkout/reset/restore worktree rewriting:**

- Files: `crates/git-command/src/checkout.rs` (1277 lines), `crates/git-command/src/checkout_core.rs` (1083 lines), `crates/git-command/src/reset.rs` (694 lines), `crates/git-worktree/src/lib.rs`
- Why fragile: Dirty-guard matrix (`verify_uptodate`), symlink-vs-dir handling (`checkout.rs:654-657,813-816,1253`), mode/`core.symlinks` interplay, and deferred `-m`/`--merge/--keep`/`--orphan`/submodule cases. A wrong guard destroys user data.
- Safe modification: Change only with a new crosswise test that runs both binaries on identical dirty trees; assert identical stdout/stderr/exit AND identical worktree/index/HEAD/ORIG_HEAD/reflog (the `phaseB07_crosswise` pattern).
- Test coverage: Partial — `crates/git-command/tests/phaseB07_crosswise.rs` covers reset soft/mixed/hard + paths, checkout/switch/detach/create, restore paths, dirty guards; merge modes and submodules uncovered.

**Index read/write + cache-tree:**

- Files: `crates/git-index/src/lib.rs:452`, `crates/git-index/src/cache_tree.rs`, `crates/git-command/src/read_tree.rs`, `crates/git-command/src/write_tree.rs`
- Why fragile: Version-gated binary format with checksum trailer; cache-tree invalidation must mirror C's `add_to_index` racy semantics (B3 claims parity); REUC, v3/v4, split/sparse indexes unimplemented so any version sniffing change widens the blast radius.
- Safe modification: Round-trip tests plus C-git validation in both directions (C reads our index, we read C's) for every change.
- Test coverage: Round-trip unit tests exist; crosswise B03/B04 cover the exercised subset.

**Revision resolution (`rev-parse` / resolver):**

- Files: `crates/git-revision/src/resolve.rs:145-190`, `crates/git-command/src/rev_parse.rs`, `crates/git-refs/src/lib.rs:122-136`
- Why fragile: Abbrev-oid disambiguation, `~`/`^` peels, `<rev>:<path>`, `@{...}`, and ref-vs-file ambiguity diagnostics must match C exactly; small divergences cascade into every command that takes a rev.
- Safe modification: Add the exact C diagnostic string to a crosswise test before changing resolution order.
- Test coverage: Partial — ranges/`@{...}`/`--all`/grafts remain deferred (FOLLOWUPS Phase 4).

**Delta + pack write path:**

- Files: `crates/git-odb/src/pack/delta.rs:682`, `crates/git-odb/src/pack/write.rs:462`, `crates/git-command/src/pack_objects.rs`, `crates/git-hash/src/sha1dc.rs:524`
- Why fragile: Byte-compat is verified externally (C `verify-pack`/`index-pack --verify` accept our packs), so any change to delta encoding, header sizes, or index fanout breaks interop silently until a crosswise run.
- Safe modification: Keep `phaseA13_crosswise` + `pack_crosswise` green; regenerate golden fixtures via `cargo xtask gen-fixtures` when intended output changes.
- Test coverage: Strong for the covered subset (proptests on delta round-trip/apply, crosswise acceptance).

**`init` layout + template copy:**

- Files: `crates/git-command/src/init.rs` (1723 lines; template walk `:1093-1124`, reinit detection `:623`, `is_symlink:793-794`)
- Why fragile: Must reproduce `.git` layout, `HEAD`, config, `--separate-git-dir`, `--shared`, `--template` across platforms; template copy handles symlinks/permissions specially.
- Safe modification: Extend `phaseB01_crosswise` (8 tests) for each new flag; known config-only gaps (`--object-format=sha256`, `--ref-format=reftable`) must stay config-only until Phases 1/7 backends land.
- Test coverage: Good for plain/bare/separate-git-dir/`-b`/reinit/`--shared=group`/`--template`.

## Scaling Limits

**Single-threaded pack/index construction:**

- Current capacity: Correctness-scale repos (fixtures, crosswise suites); acceptance criteria demand byte-parity, not throughput.
- Limit: No `--threads` parallelism (flags accepted as compat no-ops per A13); large `pack-objects`/`index-pack` runs will lag C linearly with cores.
- Scaling path: Parallelize delta search after the single-threaded output is bit-stable; assert identical bytes with threads on/off.

**In-memory tree/diff structures:**

- Current capacity: Fine for test-scale trees.
- Limit: Whole-tree maps (`TreeMap`), full-file line vectors in `apply` (`apply.rs:198-248`), and full-index vectors bound memory to repo size.
- Scaling path: Stream `apply` hunks; page large-tree diffs; add a large-repo soak test (e.g. Linux-scale tree) before claiming readiness.

## Dependencies at Risk

**Minimal third-party surface (a strength, with two watch items):**

- `thiserror 1.0` (workspace error-derive) is in maintenance mode upstream (2.x current). No functional risk today; migration is mechanical when desired.
- `regex =1.11.1` is version-pinned in one crate while the rest float (`flate2 = "1"`, `proptest = "1"`, `anyhow = "1.0"`). Pinning is fine but should be deliberate and documented if it was for a compat reason.
- `flate2` sole ownership via `git-compress` (`crates/git-compress/src/lib.rs`) is the right call — do not add direct `flate2` deps elsewhere.
- No async runtime, no CLI framework, no lockfile concern beyond the stale root `Cargo.lock`: `crates/Cargo.lock` (committed Sep 20) pins the real workspace; keep it committed for reproducible crosswise runs. Toolchain is `rust 1.97.1` vs workspace `rust-version = "1.74"` floor — CI uses stable, so the MSRV floor is currently unverified.

## Missing Critical Features

**Test infrastructure (blocks verification of everything else):**

- Problem: `git-test` crate (Rust `test-tool` replacement: `test-sha1`, `test-date`, `test-delta`, `test-read-cache`, `test-reftable`, …) NOT DONE; `cargo-fuzz` targets NOT DONE; coverage gate (`llvm-cov --fail-under-lines 90`) NOT DONE (`docs/plan/FOLLOWUPS.md` §B). Clippy in CI is advisory (`|| true` in `.github/workflows/rust-port.yml:unit`). The committed `crates/scoreboard.json` baseline is mostly `pass:false`, so the regression gate only detects true→false flips and cannot confirm improvement.
- Blocks: Running the real `t/` suite through the shim; any coverage or fuzz claim.

**Network/transport (entire Phase 10+ stretch):**

- Problem: fetch/push/clone, protocol v2 session flow, credential-helper invocation, daemon, `gc`/`maintenance` missing. `git-transport`/`git-protocol`/`git-credentials` are scaffolds and types only.
- Blocks: Any multi-repo workflow; all clone/fetch/push `t/` gates.

**Ref backend completeness:**

- Problem: packed-refs writing, reflog (`logs/<ref>`), reftable backend, ref transactions/locking, symref writes, worktree-specific refs missing (FOLLOWUPS Phase 7).
- Blocks: `gc`, concurrent-safe updates, worktrees backed by per-worktree refs.

**Index format breadth:**

- Problem: REUC, v3/v4, split/sparse index missing (FOLLOWUPS B2/Phase 6).
- Blocks: Parity on repos produced by modern C git defaults (split index) and sparse checkouts.

**Merge engine:**

- Problem: Only `merge-base`/`merge-file`/3-way line merge exist; `git merge` (merge-ort), `merge-tree`, cherry-pick/revert, octopus/independent/is-ancestor (partly done), dir/file conflicts, criss-cross bases, index merging missing (FOLLOWUPS Phase 8).
- Blocks: All merge `t/` gates beyond `merge-base`/`merge-file`.

**Diff output breadth:**

- Problem: `--word-diff`, `--color`, patience/histogram, `--dirstat`, whitespace family, `--relative`, pickaxe, function-context headers missing; stat width hard-coded to 80 (FOLLOWUPS A8).
- Blocks: `log -S/-G`, user-facing diff parity on non-tty vs tty.

**Everyday porcelain gaps:**

- Problem: `commit` lacks pathspec commits, interactive editor, `--porcelain`/`--dry-run`, hooks, GPG; `add` lacks `-p`/`-i`/`-N`/`--chmod`/pathspec magic; `status` lacks ahead/behind, stash summary, pathspec limiting; `apply` lacks `--3way`/`--index`/`--reject`/whitespace/binary handling; `am`/`format-patch`, submodules, notes, blame, stash, bisect missing (FOLLOWUPS B3/B5/B6/Phase 10+).
- Blocks: Corresponding `t/` gates (B8–B10 not started).

## Test Coverage Gaps

**Real `t/` suite not runnable:**

- What's not tested: The C `t/` scripts themselves — the stated oracle — because they need a `test-tool` binary system git does not ship; the shim is ready but the Rust `git-test` replacement is NOT DONE.
- Files: `scripts/shim-git`, `crates/scoreboard.json`, `docs/plan/test-infrastructure.md` (per FOLLOWUPS §B)
- Risk: Crosswise suites (valuable but hand-picked) are the only end-to-end signal; whole behavior classes covered by `t/` remain unexercised.
- Priority: High

**No fuzzing on parser-heavy attack surface:**

- What's not tested: pack/idx/midx/commit-graph/bitmap/index/config/reftable/loose-header/xdiff parsers against adversarial input (fuzz targets NOT DONE; seed corpora from `t/t5302`, `t/t5303`, `t/t5313` identified but unused).
- Files: `crates/git-odb/src/pack/*`, `crates/git-commitgraph/src/*`, `crates/git-index/src/lib.rs`, `crates/git-config/src/lib.rs`, `crates/git-object/src/*`
- Risk: Malformed-object panics or OOMs (e.g. `decode_bounded` limits exist in `git-compress` but callers must thread limits) break unnoticed.
- Priority: High (parsers ingest untrusted repo data; `apply` ingests untrusted patches today)

**No coverage gate:**

- What's not tested: Unmeasured — `cargo llvm-cov --fail-under-lines 90` on core crates never enforced; no CI job.
- Files: `.github/workflows/rust-port.yml` (unit, differential, scoreboard jobs only)
- Risk: New code lands without tests and nobody notices; Phase "done" gates in `docs/plan/README.md` require ≥90% but nothing checks.
- Priority: Medium

**Concurrency and crash-safety untested:**

- What's not tested: Concurrent ref/index writers, kill-during-write recovery (stale `.lock`/tmp files), signalmid-rename behavior.
- Files: `crates/git-refs/src/lib.rs:101-120`, `crates/git-index/src/lib.rs:283-290`, `crates/git-odb/src/lib.rs:187-217`
- Risk: Repository corruption modes C git survives (or reports) go unnoticed until production use.
- Priority: Medium

**Production `unwrap`/`expect` audit:**

- What's not tested: A gate asserting `unwrap`/`expect`/`panic!` appear only in `#[cfg(test)]`, `xtask`, or documented-infallible sites. Current state is actually good — the audit finds `unwrap`/`expect` concentrated in test modules and `xtask` (`crates/xtask/src/main.rs:30,39,271,284,292,299,303,312,322,325`), with production `Result`-propagation throughout and only narrow exceptions: `git-compress` in-memory deflate `expect` (`crates/git-compress/src/lib.rs:50-51`, infallible by construction), the config-continuation `expect` (`crates/git-config/src/lib.rs:142`, guarded but brittle), and four `unreachable!` internal invariants (`crates/git-command/src/cat_file.rs:135`, `init.rs:301`, `multi_pack_index.rs:41`, `crates/git-diff/src/tree.rs:106`).
- Files: All `crates/*/src/*.rs`
- Risk: Low today; regressions creep in without a gate (`xtask safety` already enforces zero-unjustified-`unsafe` per `crates/xtask/src/safety.rs` — extend the pattern).
- Priority: Low

---

*Concerns audit: 2026-09-25*
