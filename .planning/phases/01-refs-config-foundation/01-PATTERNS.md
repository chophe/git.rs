# Phase 01: refs-config-foundation - Pattern Map

**Mapped:** 2026-09-28
**Files analyzed:** 15 new/modified
**Analogs found:** 13 / 15

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/git-refs/src/lib.rs` (extend) | model/store | file-I/O (atomic write) | `crates/git-refs/src/lib.rs` itself (`RefStore::update`, lines 101-120; `packed()` lines 138-156; `validate_refname` lines 183-207) | exact (self-extend) |
| `crates/git-refs/src/lock.rs` (new) | service/store-helper | file-I/O (lock lifecycle) | `crates/git-refs/src/lib.rs:101-120` (`update` temp+rename) + `crates/git-command/src/checkout_core.rs:224-229` (`write_file_atomic`) | role-match |
| `crates/git-refs/src/reflog.rs` (new) | service/store-helper | file-I/O (append-only log) | `crates/git-command/src/checkout_core.rs:252-280` (`log_all_ref_updates` + `reflog_append`) | exact |
| `crates/git-refs/src/packed.rs` (new) | service/store-helper | file-I/O (batch rewrite) | `crates/git-refs/src/lib.rs:138-156` (`packed()` reader) + `crates/git-command/src/checkout_core.rs:224-229` (atomic rename) | role-match |
| `crates/git-config/src/lib.rs` (extend) | model/service | transform (parse/layer) | `crates/git-config/src/lib.rs` itself (`parse_into` 120-199, `get/get_in/get_all` 201-224, `append/set/set_cli` 236-284, `load_file` cycle guard 105-118, `parse_bool` 397-403, `expand_path` 407-435) | exact (self-extend) |
| `crates/git-config/src/file.rs` (new) | service | file-I/O (read-modify-write) | `crates/git-config/src/lib.rs:120-199` (parser; splice must reuse its line grammar) + `crates/git-command/src/checkout_core.rs:224-229` (atomic-rename shape, but with `<ref>.lock` naming per Pitfall 1) | role-match |
| `crates/git-command/src/reflog.rs` (new) | controller (builtin) | request-response | `crates/git-command/src/show_ref.rs:1-29` (`ShowRef`: ctx→repo→RefStore→writeln to `out`) + `crates/git-command/src/update_ref.rs:1-54` (flag-parse + dispatch-error shape) | exact |
| `crates/git-command/src/config_cmd.rs` (new) | controller (builtin) | request-response | `crates/git-command/src/show_ref.rs:31-74` (`ForEachRef`: `--format=`-style long-option parsing + `CommandError::usage` on unknown flags) + `crates/git-command/src/update_ref.rs:56-92` (`SymbolicRef`: read path + fatal mapping) | exact |
| `crates/git-command/src/update_ref.rs` (extend `--stdin`) | controller (builtin) | request-response + batch/transaction | `crates/git-command/src/update_ref.rs:1-54` itself (single-ref path to keep) | exact (self-extend) |
| `crates/git-command/src/lib.rs` (dispatch wiring) | config/route | request-response | `crates/git-command/src/lib.rs:371-428` (`dispatch_with` match arms) | exact (self-extend) |
| `crates/git-core/src/lib.rs` (scope loading in `discover_from`) | model | file-I/O + transform | `crates/git-core/src/lib.rs:113-154` (`discover_from` + single-file config load) + `crates/git-command/src/lib.rs:270-299` (`RepoContext::repository()` overlay order) | exact |
| `crates/git-command/src/{checkout_core,commit,reset,...}.rs` (reflog call-site consolidation) | service-helper call sites | file-I/O (append side effect) | `crates/git-command/src/reset.rs:575-585` (canonical HEAD+branch gated write) | exact |
| `scripts/shim-git` (add `reflog\|config`) | config | batch | `scripts/shim-git:14-21` (`case` list + `exec "$RUST_GIT"` arm) | exact (self-extend) |
| `crates/git-command/tests/phase1_crosswise.rs` (new) | test | batch | `crates/git-command/tests/phase4_crosswise.rs:1-75` (harness: `git()` probe, `tempdir()`, `run()`, `with_cwd`, `ours_output`, `build_repo`) | exact |
| `crates/xtask/src/main.rs` (`suites()` registration) | config | batch | `crates/xtask/src/main.rs:101-115` (`suites()` entries) | exact (self-extend) |

## Pattern Assignments

### `crates/git-refs/src/lib.rs` (extend — lock/transaction/reflog/packed-refs-write on `RefStore`)

**Analog:** itself (`crates/git-refs/src/lib.rs`)

**Imports pattern** (lines 8-14):
```rust
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};
```

**Core atomic-write pattern to replace** (lines 101-120) — current `.lock.<pid>` suffix is the flagged anti-pattern; new lock helper must use C-exact `<ref>.lock`:
```rust
/// Create or update a ref (atomic: temp file + rename).
pub fn update(&self, name: &str, oid: Option<&Oid>) -> Result<(), RefError> {
    validate_refname(name)?;
    let path = self.common_dir.join(name);
    match oid {
        Some(oid) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| RefError::Io(e.to_string()))?;
            }
            let tmp = path.with_extension(format!("lock.{}", std::process::id()));
            std::fs::write(&tmp, format!("{oid}\n"))
                .map_err(|e| RefError::Io(e.to_string()))?;
            std::fs::rename(&tmp, &path).map_err(|e| RefError::Io(e.to_string()))?;
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}
```

**Packed-refs read pattern to extend into write** (lines 138-156) — keep header/`^`-line skipping; writer must emit sorted entries + `# pack-refs with: peeled fully-peeled sorted` header + `^<peeled>` tag continuations:
```rust
fn packed(&self) -> Option<HashMap<String, Oid>> {
    let path = self.common_dir.join("packed-refs");
    let content = std::fs::read_to_string(&path).ok()?;
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut it = line.splitn(2, ' ');
        let oid_s = it.next()?;
        let name = it.next()?;
        if let Ok(oid) = Oid::from_hex(oid_s, self.algo) {
            map.insert(name.to_string(), oid);
        }
    }
    Some(map)
}
```

**Validation pattern to extend** (lines 183-207) — add C one-level/`@`/reflog-suffix rules on top, never a new validator:
```rust
pub fn validate_refname(name: &str) -> Result<(), RefError> {
    if !name.starts_with("refs/") {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name.contains("..")
        || name.contains("@{")
        || name.contains(".lock")
        // ... controls/space check at line 203
    { return Err(RefError::InvalidName(name.to_string())); }
    Ok(())
}
```

**Error type pattern** (lines 18-36) — extend `RefError` variants for lock/transaction failures; keep `Display` + `Error` impls:
```rust
pub enum RefError {
    Io(String),
    InvalidName(String),
    NotFound,
}
```

**Inline-test pattern** (lines 209-243) — `AtomicU32`-namespaced temp repos via `Repository::discover_from` + `RepoEnv::default()`; copy for lock/transaction/reflog unit tests:
```rust
static COUNTER: AtomicU32 = AtomicU32::new(0);
fn repo() -> (Repository, PathBuf) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-refs-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let git = dir.join(".git");
    std::fs::create_dir_all(git.join("refs/heads")).unwrap();
    // ...
    let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
    (repo, dir)
}
```

---

### `crates/git-refs/src/lock.rs` (new — C `lockfile.c` port)

**Analog:** `crates/git-command/src/checkout_core.rs:224-229` + `crates/git-refs/src/lib.rs:101-120`

**Atomic-rename skeleton to copy** (`checkout_core.rs:224-229`):
```rust
fn write_file_atomic(path: &Path, content: &[u8]) -> Result<(), CommandError> {
    let tmp = path.with_extension(format!("lock.{}", std::process::id()));
    std::fs::write(&tmp, content).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    Ok(())
}
```
**Required divergence (Pitfall 1):** temp name MUST be `<ref>.lock` (C `lockfile.c`), not `.lock.<pid>`. Lifecycle: `create (.lock, fail `unable to create lock file %s.lock` on contention) → write+fsync → commit(rename) / rollback(unlink)`. One helper owns all call sites (refs, HEAD, ORIG_HEAD, packed-refs, config file writes).

---

### `crates/git-refs/src/reflog.rs` (new — line format/parse, gating, expire, delete)

**Analog:** `crates/git-command/src/checkout_core.rs:252-280`

**Gating pattern** (lines 252-256) — port to `LOG_REFS_{UNSET,NONE,NORMAL,ALWAYS}` enum (`always` string + bool + unset→non-bare default) with per-refname prefix rule (`refs/heads/`, `refs/remotes/`, `refs/notes/`, `HEAD`); HEAD force-logged:
```rust
/// Whether ref updates should be logged (`core.logallrefupdates`, default
/// true except in bare repos).
pub(crate) fn log_all_ref_updates(repo: &git_core::Repository) -> bool {
    repo.config.get_bool("core", "logallrefupdates").unwrap_or(!repo.bare)
}
```

**Append pattern, verbatim split to reuse** (lines 258-280):
```rust
/// Append one line to `logs/<refname>` (creating parent directories).
pub(crate) fn reflog_append(
    repo: &git_core::Repository,
    refname: &str,
    old: &Oid,
    new: &Oid,
    ident: &str,
    message: &str,
) {
    let (who, when) = match ident.find('>') {
        Some(gt) => (&ident[..=gt], ident[gt + 1..].trim()),
        None => (ident, ""),
    };
    let line = format!("{old} {new} {who} {when}\t{message}\n");
    let path = repo.git_dir.join("logs").join(refname);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
```
Line contract: `<old-hex> <new-hex> <Name> <email> <ts> <tz>\t<message>\n`, no tab when message empty (C `log_ref_write_fd`).

**Canonical call-site shape** — `crates/git-command/src/reset.rs:575-585` is the template every mutating command converges on (HEAD always, branch only on change):
```rust
if checkout_core::log_all_ref_updates(repo) {
    let ident = checkout_core::committer_ident(repo)?;
    let old = head.oid.unwrap_or(*repo.hash_algo.null_oid());
    let msg = checkout_core::reflog_action(format!("reset: moving to {rev}"));
    checkout_core::reflog_append(repo, "HEAD", &old, target, &ident, &msg);
    if let Some(sym) = head.symref {
        if old != *target {
            checkout_core::reflog_append(repo, &sym, &old, target, &ident, &msg);
        }
    }
}
```

**`GIT_REFLOG_ACTION` pattern** (`checkout_core.rs:909-915`):
```rust
/// Read `$GIT_REFLOG_ACTION` (used as the reflog message verbatim when set).
pub(crate) fn reflog_action(default_msg: String) -> String {
    match std::env::var("GIT_REFLOG_ACTION") {
        Ok(a) if !a.is_empty() => a,
        _ => default_msg,
    }
}
```

**Ident pattern** (`checkout_core.rs:282-285`): `crate::ident::user_ident(repo, false)` supplies the committer line.

---

### `crates/git-refs/src/packed.rs` (new — packed-refs atomic sorted write)

**Analog:** `crates/git-refs/src/lib.rs:138-156` (reader) + `checkout_core.rs:224-229` (rename)

Copy the reader's line grammar (skip `#`/`^`/blank, `splitn(2,' ')`, `Oid::from_hex`), invert for write: sort by refname, header `# pack-refs with: peeled fully-peeled sorted`, `^<peeled>` continuation after annotated tags, all under the packed-refs lock (`<common_dir>/packed-refs.lock`) with fsync→rename; unlink loose refs only after commit.

---

### `crates/git-config/src/lib.rs` (extend — scopes, includeIf, multivar, typed get)

**Analog:** itself (`crates/git-config/src/lib.rs`)

**Entry/parse skeleton** (lines 12-21, 120-199): `ConfigEntry { section, subsection, key, value, origin }`, `parse_into` handles sections/subsections, `key value`/`key=value`, continuations, quotes/escapes, inline comments; `BadLine{line,file}` on unclosed `[`. Extend: six includeIf conditions (`gitdir:`, `gitdir/i:`, `onbranch:`, `hasconfig:remote.*.url:`, `worktree:`, `worktree/i:`; unknown → silently false), `MAX_INCLUDE_DEPTH 10` die on overflow (cycle-vs-depth are distinct errors).

**Cycle-guard pattern** (lines 105-118) to generalize for depth cap:
```rust
fn load_file(&mut self, path: &Path, seen: &mut Vec<PathBuf>) -> Result<(), ConfigError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&canonical) {
        return Err(ConfigError::IncludeCycle(canonical));
    }
    seen.push(canonical);
    // ...
}
```

**Layering pattern** (lines 236-239): `append` = later files win; scope load order system → XDG → global → local → worktree → `GIT_CONFIG_COUNT/K/V` → `-c`:
```rust
/// Append another set's entries (later files win on lookup).
pub fn append(&mut self, other: ConfigSet) {
    self.entries.extend(other.entries);
}
```

**Lookup family to extend** (lines 201-229): `get`/`get_in` (last-wins reverse find), `get_all` (file-order filter), `get_bool` via `parse_bool`. Add typed canonicalizer (`--type=bool/int/path/expiry-date/bool-or-int`) in one place, not per subcommand.

**Bool + path patterns to reuse** (lines 397-435): `parse_bool` (`""/yes/on/true/1` vs `no/off/false/0`), `expand_path` (`~/` + `~` + relative-to-including-file). Extend `expand_path` with `$HOME`/`${HOME}` if gates demand; keep relative-resolution-from-including-file.

**CLI overlay pattern** (lines 263-284): `set_cli` splits first-dot=section, last-dot-of-rest=subsection/key, lowercases section+key, missing `=` → `"true"`.

**Proptest pattern** (lines 568-589): `parse_never_panics` + round-trip props; add scope/includeIf/file-edit props here.

---

### `crates/git-config/src/file.rs` (new — scope-file read-modify-write)

**Analog:** `crates/git-config/src/lib.rs:120-199` (line grammar the splicer must respect) + atomic-rename shape (`checkout_core.rs:224-229`, with `<file>.lock` naming).

Requirements: locate target section/key lines, splice values in place, append sections as needed; preserve comments/ordering/whitespace (gates compare exact file content). Multivar ops (`--add` appends duplicate, `--replace-all` with optional `--value`/`--fixed-value` match, `--unset-all`) handle repeated keys. Never re-render from `entries()`.

---

### `crates/git-command/src/reflog.rs` (new) and `config_cmd.rs` (new)

**Analog:** `crates/git-command/src/show_ref.rs:1-29` (minimal builtin) + `crates/git-command/src/show_ref.rs:31-74` (`ForEachRef` long-option parsing) + `crates/git-command/src/update_ref.rs:1-54` (error mapping)

**Minimal builtin skeleton to copy** (`show_ref.rs:11-29`):
```rust
pub struct ShowRef;

impl Command for ShowRef {
    fn name(&self) -> &'static str {
        "show-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            if !a.starts_with('-') {
                return Err(CommandError::usage(format!("show-ref: unexpected argument '{a}'")));
            }
        }
        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        for (name, oid) in store.list() {
            writeln!(out, "{oid} {name}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
```
Rules: `use std::io::Write; use crate::{Command, CommandError, RepoContext};`, unit struct + `Command` trait, `ctx.repository()?`, `RefStore::from_repo(&repo)` / `ConfigSet` ops, `writeln!(out, ...)` never `println!`, `CommandError::usage` for bad flags / `fatal` for I/O / `error` for exit-1.

**Long-option parsing shape** (`show_ref.rs:38-50` — `ForEachRef`):
```rust
for a in args {
    if let Some(f) = a.strip_prefix("--format=") {
        format = f.to_string();
    } else if a.starts_with('-') && a.len() > 1 {
        return Err(CommandError::usage(format!("for-each-ref: option '{a}' not supported")));
    } else {
        pattern = Some(a.clone());
    }
}
```
Apply to `reflog`'s 7 subcommands (`show|list|exists|write|delete|drop|expire`) and `config`'s dual spellings (legacy flags `--get/--get-all/--replace-all/--unset/--unset-all/-l` AND `get/set/unset/list` subcommands per `t/t1300` mode loop).

**Ref-mutation + error-mapping shape** (`update_ref.rs:30-52`):
```rust
let repo = ctx.repository()?;
let store = RefStore::from_repo(&repo);
let algo = repo.hash_algo;
// ...
let oid = Oid::from_hex(&rest[1], algo)
    .map_err(|_| CommandError::error(format!("invalid object name '{}'", rest[1])))?;
store
    .update(&rest[0], Some(&oid))
    .map_err(|e| CommandError::fatal(e.to_string()))?;
```

---

### `crates/git-command/src/update_ref.rs` (extend — `--stdin` batch grammar)

**Analog:** itself; keep `UpdateRef::run` single-ref `-d`/set path (`update_ref.rs:11-54`) intact as the non-`--stdin` branch.

Batch requirements (C `parse_cmd_*`): two phases — (1) parse all lines into ops with byte-exact die strings (`"create %s: missing <new-oid>"`, `"delete %s: extra input: %s"`…), verbs `update|create|delete|verify|symref-update|symref-create|symref-delete|symref-verify|start|prepare|commit|abort|option`, `-z` NUL mode vs whitespace+C-quote dual mode via `parse_arg`/`unquote_c_style` port; (2) run transaction checks (old-oid, existence, D/F collisions, symref rules) then commit all renames or abort all (`t/t1404` asserts byte-unchanged set after failure). Add `-m <msg>` (reflog message), `--no-deref`, `--create-reflog` flag handling (currently `-m|--create-reflog` are silently-ok no-ops at lines 22).

---

### `crates/git-command/src/lib.rs` + `crates/git-core/src/lib.rs` + `scripts/shim-git` + `crates/xtask/src/main.rs`

**Dispatch wiring** (`lib.rs:371-428`): add arms following the existing one-line pattern —
```rust
"update-ref" => &update_ref::UpdateRef,
"symbolic-ref" => &update_ref::SymbolicRef,
// planner adds: "reflog" => &reflog::Reflog, "config" => &config_cmd::Config,
```
Register new modules in the `pub mod` list (lines 9-55, alphabetical: `config_cmd` near `commit*`, `reflog` near `rev_*`). New commands inherit `-C/--git-dir/-c/GIT_CONFIG_COUNT` via `ctx.repository()` (`lib.rs:270-299`); `config_count_overrides()` missing-key fatal (`lib.rs:312-331`) already exists — reuse, don't reimplement.

**Scope loading** (`git-core/src/lib.rs:144-148`): today only `$COMMONDIR/config` is read —
```rust
let config_path = common_dir.join("config");
let config = match std::fs::read(&config_path) {
    Ok(data) => ConfigSet::parse(&data).map_err(|e| e.with_file(config_path))?,
    Err(_) => ConfigSet::new(),
};
```
Grow into one scope-loading function (system → XDG → global → local → worktree gated on `extensions.worktreeConfig`, honoring `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`) used by both discovery and the `config` command's `--system/--global/--local/--worktree/-f/--blob` selectors. Note `ConfigSet::from_file` resolves includes but `parse` does not — scope loader must go through the include path.

**Shim** (`scripts/shim-git:14-21`): extend the `case` alternation with `reflog|config` (keep `exec "$RUST_GIT" "$@"` arm):
```sh
case "$CMD" in
	version|init|...|branch|tag|merge-base|merge-file|fsck|apply|check-ignore|check-attr)
		exec "$RUST_GIT" "$@"
		;;
```

**Crosswise test + suite registration**: new `crates/git-command/tests/phase1_crosswise.rs` copies `phase4_crosswise.rs:1-75` harness (`git()` probe over `/usr/bin/git` etc., `tempdir()` with `AtomicU32`, `run()`, `with_cwd` with `CWD_LOCK`, `ours_output<C: GitCommand>`, `build_repo` committing via system git). Register in `suites()` (`xtask/src/main.rs:101-115`):
```rust
(
    "phase4-crosswise",
    &["test", "-p", "git-command", "--test", "phase4_crosswise"],
),
```

---

## Shared Patterns

### Thin CLI + `Command` trait
**Source:** `crates/git-command/src/lib.rs:333-343` (+ `dispatch_with` 371-428)
**Apply to:** `reflog.rs`, `config_cmd.rs`, `update_ref.rs` extension
```rust
pub trait Command {
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError>;
}
```
Never `println!`, never process-exit inside logic; output to injected `out: &mut dyn Write`.

### Error codes
**Source:** `crates/git-command/src/lib.rs:75-96`
**Apply to:** all new error paths
```rust
CommandError::usage(..)  // code 129
CommandError::fatal(..)  // code 128
CommandError::error(..)  // code 1
CommandError::silent(..) // code as given, no message
```
C lock/transaction dies → 128 with `fatal:` prefix; each string verified against built C binary. `RepoError→fatal` (`lib.rs:106-110`) and `OdbError/PackError/MidxError/GraphError→fatal` (112-134) `From` impls already exist.

### RepoContext (global options + overlays)
**Source:** `crates/git-command/src/lib.rs:144-299` (`from_global_args`, `at()` for tests, `repository()`)
**Apply to:** both new commands
CLI `-c` overlays beat files; `GIT_CONFIG_COUNT/K/V` applied before real `-c` (`lib.rs:291-296`). Tests use `RepoContext::at(dir)` or `with_cwd` + `RepoContext::new()` (see `phase4_crosswise.rs:70-75`).

### Atomic file write (with C-exact lock naming)
**Source:** `crates/git-command/src/checkout_core.rs:224-229`
**Apply to:** `git-refs/src/lock.rs`, `packed.rs`, `git-config/src/file.rs`
Rename shape as above, but temp name `<path>.lock` per C — the `.lock.<pid>` suffix in current code is superseded.

### Reflog write (single writer)
**Source:** `crates/git-command/src/checkout_core.rs:258-280` (writer) + `crates/git-command/src/reset.rs:575-585` (call-site shape) + `crates/git-command/src/checkout_core.rs:909-915` (`GIT_REFLOG_ACTION`)
**Apply to:** `git-refs/src/reflog.rs` (writer home) and every mutating command (branch/tag/update-ref/clone + existing checkout/reset/commit call sites converge on it)
Consolidate the three ad-hoc writers (`checkout_core.rs:259`, `commit.rs:659`, reset path); `logallrefupdates` read lives only inside the helper after the port.

### Config parse + layering
**Source:** `crates/git-config/src/lib.rs` (`parse_into` 120-199, `append` 237-239, `set_cli` 267-284, `parse_bool` 397-403, `expand_path` 407-435)
**Apply to:** scope loader (`git-core`), `config_cmd.rs`, `file.rs`
Build CLI on `ConfigSet`; don't re-parse. Type canonicalization (`--type=...`) in one shared function.

### Layering (depcheck)
**Source:** `crates/xtask/src/depcheck.rs:27-45`
**Apply to:** all new modules
`git-config`=mid(1), `git-refs`=store(2), `git-command`=surface(5); edges downward only; `git-command` is the sole composition-root exception. Storage logic in `git-refs`/`git-config`, composition only in `git-command`. Gate: `cargo xtask depcheck`.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/git-refs/src/transaction.rs` internals (two-phase prepare/commit op queue, D/F + old-oid + symref checks) | service | batch/transaction | No multi-ref transaction engine exists yet — only single-ref `RefStore::update`; semantics must come from C `refs/files-backend.c` + `t/t1404` (RESEARCH Pitfall 4). Place inside `git-refs` (planner decides `lib.rs` vs new file). |
| `git config --get-urlmatch` / `--get-colorbool` value semantics | controller-leaf | transform | No URL-match or color parsing exists in-tree; port from C `builtin/config.c` + `config.c` (no new deps). |

## Metadata

**Analog search scope:** `crates/git-refs/src`, `crates/git-config/src`, `crates/git-command/src`, `crates/git-core/src`, `crates/git-command/tests`, `crates/xtask/src`, `scripts/` (13 files read; search stopped at saturation per 3–5-strong-match rule — exceeded with exact self/role matches)
**Files scanned:** 13
**Pattern extraction date:** 2026-09-28
**Tracked-source gate:** all analog paths verified via `git ls-files` (13/13 tracked; no mirror paths emitted)
