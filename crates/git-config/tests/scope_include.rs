//! Scope precedence, conditional includes, layout-preserving edits, multivar
//! operations, and typed canonicalization — the plan 01-02 storage engine
//! verified end to end through the public API.

use git_config::file::{
    add_value, remove_section, rename_section, replace_all, set_value, unset_all, write_config_file,
    ValueMatcher,
};
use git_config::{canonicalize_typed, ConfigSet, ConfigValueType, IncludeContext, RepoScopes};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fresh_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-config-scopeinc-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Isolate scope resolution from the developer's ambient config. Holds the
/// process-global env lock; restores everything on drop.
struct EnvGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(String, Option<std::ffi::OsString>)>,
    pub dir: PathBuf,
}

impl EnvGuard {
    fn new(tag: &str) -> EnvGuard {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = fresh_dir(tag);
        let mut saved = Vec::new();
        for key in ["GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "HOME", "XDG_CONFIG_HOME"] {
            saved.push((key.to_string(), std::env::var_os(key)));
        }
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_CONFIG_GLOBAL", dir.join("global.conf"));
        std::env::set_var("GIT_CONFIG_SYSTEM", dir.join("system.conf"));
        std::env::set_var("HOME", &dir);
        std::env::remove_var("XDG_CONFIG_HOME");
        EnvGuard { _guard: guard, saved, dir }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            match v {
                Some(val) => std::env::set_var(&k, val),
                None => std::env::remove_var(&k),
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn make_repo(dir: &Path, name: &str, head: &str) -> PathBuf {
    let git_dir = dir.join(name).join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), head).unwrap();
    git_dir
}

fn scopes(git_dir: &Path, worktree: Option<&Path>) -> RepoScopes {
    RepoScopes {
        git_dir: git_dir.to_path_buf(),
        commondir: git_dir.to_path_buf(),
        worktree: worktree.map(|p| p.to_path_buf()),
        git_dir_verbatim: None,
    }
}

#[test]
fn scope_precedence_local_beats_global_beats_system_with_cli_last() {
    let env = EnvGuard::new("precedence");
    std::fs::write(env.dir.join("system.conf"), "[user]\n\tname = sys\n").unwrap();
    std::fs::write(env.dir.join("global.conf"), "[user]\n\tname = glob\n").unwrap();
    let git_dir = make_repo(&env.dir, "repo", "ref: refs/heads/main\n");
    std::fs::write(git_dir.join("config"), "[user]\n\tname = local\n").unwrap();

    let cfg = ConfigSet::load_repo_scopes(&scopes(&git_dir, None)).unwrap();
    assert_eq!(cfg.get("user", "name"), Some("local"));

    // CLI overlays (`-c` / `GIT_CONFIG_COUNT`) apply after every file.
    let mut cli = cfg;
    cli.set_cli("user.name", Some("cli"));
    assert_eq!(cli.get("user", "name"), Some("cli"));
}

#[test]
fn includeif_matrix_through_discovery() {
    let env = EnvGuard::new("matrix");
    let git_dir = make_repo(&env.dir, "myrepo", "ref: refs/heads/main\n");
    let wt = env.dir.join("myrepo");
    std::fs::write(env.dir.join("inc-gitdir.conf"), "[t]\n\tg = 1\n").unwrap();
    std::fs::write(env.dir.join("inc-branch.conf"), "[t]\n\tb = 1\n").unwrap();
    std::fs::write(env.dir.join("inc-remote.conf"), "[t]\n\tr = 1\n").unwrap();
    std::fs::write(env.dir.join("inc-wt.conf"), "[t]\n\tw = 1\n").unwrap();
    std::fs::write(env.dir.join("inc-no.conf"), "[t]\n\tn = 1\n").unwrap();
    let inc = |n: &str| env.dir.join(n).display().to_string();
    std::fs::write(
        git_dir.join("config"),
        format!(
            "[includeIf \"gitdir:myrepo/\"]\n\tpath = {}\n\
             [includeIf \"onbranch:main\"]\n\tpath = {}\n\
             [remote \"origin\"]\n\turl = https://example.com/r.git\n\
             [includeIf \"hasconfig:remote.*.url:https://*/r.git\"]\n\tpath = {}\n\
             [includeIf \"worktree:myrepo\"]\n\tpath = {}\n\
             [includeIf \"gitdir:nomatch-xyz/\"]\n\tpath = {}\n",
            inc("inc-gitdir.conf"),
            inc("inc-branch.conf"),
            inc("inc-remote.conf"),
            inc("inc-wt.conf"),
            inc("inc-no.conf"),
        ),
    )
    .unwrap();

    // `worktree:` needs the hint: pass the worktree explicitly like
    // `Repository::discover_from` does.
    let cfg = ConfigSet::load_repo_scopes(&scopes(&git_dir, Some(&wt))).unwrap();
    assert_eq!(cfg.get("t", "g"), Some("1"), "gitdir");
    assert_eq!(cfg.get("t", "b"), Some("1"), "onbranch");
    assert_eq!(cfg.get("t", "r"), Some("1"), "hasconfig");
    assert_eq!(cfg.get("t", "w"), Some("1"), "worktree");
    assert_eq!(cfg.get("t", "n"), None, "non-matching condition");
}

#[test]
fn layout_set_changes_only_that_line() {
    let before = "# top comment\n[user]\n    name = alice # trailing\n\temail = a@x.y\n; note\n[core]\n\tbare = false\n";
    let after = set_value(before, "user", None, "name", "bob", None).unwrap();
    // Single-hunk diff: same line count, exactly one line differs, and the
    // rest is byte-identical.
    let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    assert_eq!(b.len(), a.len());
    let diffs: Vec<usize> = b.iter().zip(a.iter()).enumerate().filter(|(_, (x, y))| x != y).map(|(i, _)| i).collect();
    assert_eq!(diffs.len(), 1, "expected a single changed line: {after:?}");
    assert_eq!(a[diffs[0]], "\tname = bob");
    // The value still resolves through the parser.
    let cfg = ConfigSet::parse(after.as_bytes()).unwrap();
    assert_eq!(cfg.get("user", "name"), Some("bob"));
    assert_eq!(cfg.get("user", "email"), Some("a@x.y"));
}

#[test]
fn layout_unset_keeps_comments_and_drops_empty_sections() {
    // Last key with a comment nearby: header and comment survive.
    let before = "[user]\n\t# c\n\tname = alice\n[core]\n\tbare = false\n";
    let (after, n) = unset_all(before, "user", None, "name", None);
    assert_eq!(n, 1);
    assert_eq!(after, "[user]\n\t# c\n[core]\n\tbare = false\n");

    // No comments: the emptied section header goes too (C `maybe_remove_section`).
    let before = "[user]\n\tname = alice\n[core]\n\tbare = false\n";
    let (after, n) = unset_all(before, "user", None, "name", None);
    assert_eq!(n, 1);
    assert_eq!(after, "[core]\n\tbare = false\n");
}

#[test]
fn multivar_add_replace_all_unset_all() {
    let base = "[a]\n\tk = one\n";
    // add appends a duplicate.
    let text = add_value(base, "a", None, "k", "two", None);
    assert_eq!(text, "[a]\n\tk = one\n\tk = two\n");
    let cfg = ConfigSet::parse(text.as_bytes()).unwrap();
    assert_eq!(cfg.get_all("a", "k"), vec!["one", "two"]);

    // Plain set refuses on multiple values (C CONFIG_NOTHING_SET).
    assert!(set_value(&text, "a", None, "k", "x", None).is_err());

    // replace-all with a value matcher rewrites only matching duplicates.
    let matcher = ValueMatcher::compile("^one$", false).unwrap();
    let (text, n) = replace_all(&text, "a", None, "k", "ONE", Some(&matcher), None);
    assert_eq!(n, 1);
    assert_eq!(text, "[a]\n\tk = ONE\n\tk = two\n");

    // Fixed-value matching is exact, not regex.
    let fixed = ValueMatcher::compile("t.o", true).unwrap();
    assert!(!fixed.matches("two"));
    assert!(fixed.matches("t.o"));

    // unset-all with a matcher removes only matches.
    let unmatcher = ValueMatcher::compile("^ONE$", false).unwrap();
    let (text, n) = unset_all(&text, "a", None, "k", Some(&unmatcher));
    assert_eq!(n, 1);
    assert_eq!(text, "[a]\n\tk = two\n");

    // Invalid regex fails before touching anything.
    assert!(ValueMatcher::compile("[unclosed", false).is_err());
    // `!` negation is the caller's job (C `do_not_match`): strip and flip.
    let m = ValueMatcher::compile("two", false).unwrap();
    assert!(m.matches("two"));
    assert!(!m.matches("three"));
}

#[test]
fn rename_and_remove_section() {
    // C `section_name_match` is case-sensitive and subsection-exact: `old`
    // renames only the plain `[old]` header (probed on the tree binary);
    // `[old "sub"]` needs the dotted name `old.sub` (which drops the
    // subsection, like C's `write_section`).
    let before = "[old]\n\ta = 1\n[old \"sub\"]\n\tb = 2\n[other]\n\tc = 3\n";
    let (after, n) = rename_section(before, "old", "new");
    assert_eq!(n, 1);
    assert_eq!(after, "[new]\n\ta = 1\n[old \"sub\"]\n\tb = 2\n[other]\n\tc = 3\n");

    let (after, n) = remove_section(&after, "new");
    assert_eq!(n, 1);
    assert_eq!(after, "[old \"sub\"]\n\tb = 2\n[other]\n\tc = 3\n");

    let (after, n) = rename_section(before, "old.sub", "new");
    assert_eq!(n, 1);
    assert_eq!(after, "[old]\n\ta = 1\n[new]\n\tb = 2\n[other]\n\tc = 3\n");

    let (after, n) = remove_section(before, "OLD");
    assert_eq!(n, 0);
    assert_eq!(after, before);
}

#[test]
fn typed_canonicalizer_matches_c() {
    use ConfigValueType as T;
    assert_eq!(canonicalize_typed(T::Bool, "a.b", "yes").unwrap(), "true");
    assert_eq!(canonicalize_typed(T::Bool, "a.b", "OFF").unwrap(), "false");
    assert_eq!(
        canonicalize_typed(T::Bool, "a.b", "maybe").unwrap_err().to_string(),
        "bad boolean config value 'maybe' for 'a.b'"
    );
    assert_eq!(canonicalize_typed(T::Int, "a.b", "1k").unwrap(), "1024");
    assert_eq!(canonicalize_typed(T::Int, "a.b", "0x10").unwrap(), "16");
    assert_eq!(canonicalize_typed(T::Int, "a.b", "010").unwrap(), "8");
    assert_eq!(
        canonicalize_typed(T::Int, "a.b", "10x").unwrap_err().to_string(),
        "bad numeric config value '10x' for 'a.b': invalid unit"
    );
    assert_eq!(canonicalize_typed(T::BoolOrInt, "a.b", "true").unwrap(), "true");
    assert_eq!(canonicalize_typed(T::BoolOrInt, "a.b", "5").unwrap(), "5");
    // Stored as-is (C `normalize_value` skips both).
    assert_eq!(canonicalize_typed(T::Path, "a.b", "~/x").unwrap(), "~/x");
    assert_eq!(canonicalize_typed(T::ExpiryDate, "a.b", "2.weeks.ago").unwrap(), "2.weeks.ago");
}

#[test]
fn atomic_write_replaces_and_locks() {
    let dir = fresh_dir("atomic");
    let path = dir.join("config");
    write_config_file(&path, "[a]\n\tk = 1\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[a]\n\tk = 1\n");
    // No `.lock` turd left behind.
    assert!(!dir.join("config.lock").exists());

    // A competing lock file blocks the write instead of tearing state.
    std::fs::write(dir.join("config.lock"), "stale").unwrap();
    let err = write_config_file(&path, "[a]\n\tk = 2\n").unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("could not lock config file {}: File exists (os error 17)", path.display())
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[a]\n\tk = 1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn include_context_default_is_documented() {
    // Standalone `from_file` has no repo context; this just pins the default.
    let ctx = IncludeContext::default();
    assert!(ctx.git_dir.is_none());
    assert!(ctx.head_branch.is_none());
}
