//! Crosswise tests for Phase B item B1 (`git init`) against the system C
//! git. Skips when no system `git` is available.
//!
//! Each case runs both binaries in fresh twin directories with identical,
//! tightly controlled environments (fresh `$HOME`, `GIT_CONFIG_NOSYSTEM=1`
//! unless the case says otherwise) and asserts byte-identical
//! stdout/stderr/exit code plus identical trees, file bytes, modes, and
//! symlink targets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn rust_git() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/git")
}

fn tempdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-b01-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Environment variables scrubbed for hermetic runs (ambient values must not
/// leak into either side).
fn scrubbed(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home);
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("LC_ALL", "C");
    // NOTE: GIT_CONFIG_COUNT/KEY_*/VALUE_* intentionally flow through to
    // both sides (ambient values apply identically; explicit cases below
    // pin the semantics).
    for v in [
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_TEMPLATE_DIR",
        "GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME",
        "GIT_DEFAULT_HASH",
        "GIT_DEFAULT_REF_FORMAT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_INDEX_FILE",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(v);
    }
}

struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
    /// relpath -> (bytes with dirs normalized, mode bits, symlink target)
    files: BTreeMap<String, (Vec<u8>, u32, Option<String>)>,
}

fn snapshotted(dir: &Path) -> BTreeMap<String, (Vec<u8>, u32, Option<String>)> {
    let mut map = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    let prefix = dir.to_string_lossy().into_owned();
    while let Some(d) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(path);
                continue;
            }
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o7777
            };
            #[cfg(not(unix))]
            let mode = 0u32;
            let link = std::fs::read_link(&path).ok().map(|p| p.to_string_lossy().into_owned());
            let bytes = std::fs::read(&path)
                .map(|b| String::from_utf8_lossy(&b).replace(&prefix, "@D@").into_bytes())
                .unwrap_or_default();
            map.insert(rel, (bytes, mode, link));
        }
    }
    map
}

fn run_one(
    bin: &str,
    root: &Path,
    cwd: &Path,
    home: &Path,
    extra_env: &[(String, String)],
    args: &[String],
) -> Outcome {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd).args(args);
    scrubbed(&mut cmd, home);
    for (k, v) in extra_env {
        if v.is_empty() {
            cmd.env_remove(k);
        } else {
            cmd.env(k, v);
        }
    }
    let out = cmd.output().expect("git runs");
    let here = root.to_string_lossy().into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).replace(&here, "@D@");
    let stderr = String::from_utf8_lossy(&out.stderr).replace(&here, "@D@");
    Outcome {
        stdout,
        stderr,
        code: out.status.code().unwrap_or(128),
        files: snapshotted(root),
    }
}

/// A hermetic setup command (same scrubbed env as the case run).
fn setup_cmd(bin: &str, dir: &Path, home: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir);
    scrubbed(&mut cmd, home);
    cmd
}

/// Expand `@HOME@`/`@TWIN@` placeholders in argv per side.
fn expand_args(args: &[&str], home: &Path, twin: &Path) -> Vec<String> {
    let h = home.to_string_lossy().into_owned();
    let t = twin.to_string_lossy().into_owned();
    args.iter().map(|a| a.replace("@HOME@", &h).replace("@TWIN@", &t)).collect()
}

fn check_case(
    name: &str,
    setup: &dyn Fn(&Path, &str, &Path),
    extra_env: &[(String, String)],
    args: &[&str],
) {
    check_case_in(name, setup, extra_env, "", args);
}

fn check_case_in(
    name: &str,
    setup: &dyn Fn(&Path, &str, &Path),
    extra_env: &[(String, String)],
    cwd_rel: &str,
    args: &[&str],
) {
    let real = git().expect("system git required");
    let ours = rust_git();
    let ours = ours.to_str().unwrap();
    let home = tempdir(&format!("{name}-home"));
    let c = tempdir(&format!("{name}-c"));
    let r = tempdir(&format!("{name}-r"));
    setup(&c, real, &home);
    setup(&r, ours, &home);
    let cw_c = c.join(cwd_rel);
    let cw_r = r.join(cwd_rel);
    // Twin-relative normalization must cover both the root and the cwd.
    let a = run_one(real, &c, &cw_c, &home, extra_env, &expand_args(args, &home, &c));
    let b = run_one(ours, &r, &cw_r, &home, extra_env, &expand_args(args, &home, &r));
    assert_eq!(
        a.code, b.code,
        "[{name}] exit code\nreal stdout: {}\nreal stderr: {}\nours stdout: {}\nours stderr: {}",
        a.stdout, a.stderr, b.stdout, b.stderr
    );
    assert_eq!(a.stdout, b.stdout, "[{name}] stdout");
    assert_eq!(a.stderr, b.stderr, "[{name}] stderr");
    assert_eq!(
        a.files.keys().collect::<Vec<_>>(),
        b.files.keys().collect::<Vec<_>>(),
        "[{name}] tree"
    );
    for (rel, (bytes, mode, link)) in &a.files {
        let (obytes, omode, olink) =
            b.files.get(rel).unwrap_or_else(|| panic!("[{name}] missing in rust: {rel}"));
        assert_eq!(
            bytes,
            obytes,
            "[{name}] content {rel}\nreal:\n{}\nours:\n{}",
            String::from_utf8_lossy(bytes),
            String::from_utf8_lossy(obytes)
        );
        assert_eq!(mode, omode, "[{name}] mode {rel}: {mode:o} vs {omode:o}");
        assert_eq!(link, olink, "[{name}] symlink {rel}");
    }
}

fn no_setup(_dir: &Path, _bin: &str, _home: &Path) {}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A template fixture: regular file, hook, info file, and a config that
/// overrides one of init's defaults plus an extra key.
fn make_template(dir: &Path) -> PathBuf {
    let tpl = dir.join("tpl");
    std::fs::create_dir_all(tpl.join("hooks")).unwrap();
    std::fs::create_dir_all(tpl.join("info")).unwrap();
    std::fs::write(tpl.join("file"), "content\n").unwrap();
    std::fs::write(tpl.join("hooks").join("myhook.sample"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(tpl.join("info").join("custom-exclude"), "*.custom\n").unwrap();
    std::fs::write(
        tpl.join("config"),
        "[core]\n\tlogallrefupdates = false\n\tcustomkey = customvalue\n",
    )
    .unwrap();
    tpl
}

fn setup_init_q_branch(branch: &str, target: &str) -> impl Fn(&Path, &str, &Path) {
    let (branch, target) = (branch.to_string(), target.to_string());
    move |dir: &Path, bin: &str, home: &Path| {
        assert!(setup_cmd(bin, dir, home)
            .args(["init", "-q", "-b", &branch, &target])
            .status()
            .unwrap()
            .success());
    }
}

#[test]
fn init_layout_matrix() {
    if git().is_none() {
        return;
    }
    check_case("plain", &no_setup, &[], &["init"]);
    check_case("bare", &no_setup, &[], &["init", "--bare", "b1"]);
    check_case("branch-b", &no_setup, &[], &["init", "-b", "feat", "f1"]);
    check_case("branch-eq", &no_setup, &[], &["init", "--initial-branch=hello", "f2"]);
    check_case("quiet", &no_setup, &[], &["init", "-q", "q1"]);
    check_case("deep", &no_setup, &[], &["init", "a/b/c"]);
    check_case("sepdir", &no_setup, &[], &["init", "--separate-git-dir", "rg", "w1"]);
    check_case("reinit", &setup_init_q_branch("keep", "x"), &[], &["init", "x"]);
    check_case(
        "reinit-branch",
        &setup_init_q_branch("keep", "x"),
        &[],
        &["init", "--initial-branch=ignore", "x"],
    );
    check_case(
        "reinit-quiet-branch",
        &setup_init_q_branch("keep", "x"),
        &[],
        &["init", "-q", "--initial-branch=ignore", "x"],
    );
}

#[test]
fn init_branch_selection() {
    if git().is_none() {
        return;
    }
    // Unconfigured default: master + advice hint (system config suppressed).
    check_case("default-hint", &no_setup, &[], &["init", "h1"]);
    check_case("cli-config-branch", &no_setup, &[], &["-c", "init.defaultBranch=nmb", "init", "h2"]);
    check_case(
        "env-branch",
        &no_setup,
        &env(&[("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "envname")]),
        &["init", "h3"],
    );
    check_case(
        "env-branch-empty",
        &no_setup,
        &env(&[("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "")]),
        &["init", "h4"],
    );
    check_case(
        "env-branch-invalid",
        &no_setup,
        &env(&[("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "with space")]),
        &["init", "h5"],
    );
    check_case(
        "advice-off",
        &no_setup,
        &[],
        &["-c", "advice.defaultBranchName=false", "init", "h6"],
    );
    check_case("invalid-cli-branch", &no_setup, &[], &["init", "-b", "bad..name", "h7"]);
    // GIT_CONFIG_COUNT entries behave like -c ...
    check_case(
        "count-branch",
        &no_setup,
        &env(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
            ("GIT_CONFIG_VALUE_0", "countbranch"),
        ]),
        &["init", "cb"],
    );
    // ... a missing KEY_n is fatal ...
    check_case(
        "count-missing-key",
        &no_setup,
        &env(&[("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "")]),
        &["init", "cbm"],
    );
    // ... and real -c wins over COUNT.
    check_case(
        "count-vs-cli",
        &no_setup,
        &env(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
            ("GIT_CONFIG_VALUE_0", "countbranch"),
        ]),
        &["-c", "init.defaultBranch=ccbranch", "init", "cbv"],
    );
}

#[test]
fn init_templates() {
    if git().is_none() {
        return;
    }
    check_case(
        "tpl-custom",
        &|dir, _, _| {
            make_template(dir);
        },
        &[],
        &["init", "--template=tpl", "t1"],
    );
    check_case(
        "tpl-separate-arg",
        &|dir, _, _| {
            make_template(dir);
        },
        &[],
        &["init", "--template", "tpl", "t1b"],
    );
    check_case("tpl-empty", &no_setup, &[], &["init", "--template=", "t2"]);
    check_case("tpl-missing", &no_setup, &[], &["init", "--template=/nonexistent-xyz-pdq", "t3"]);
    // NOTE: no overlong-template case: Apple truncates the path in its
    // warning (~4K cap) while upstream prints it whole; both succeed (rc 0),
    // verified manually. t/t0001 only requires success.
    check_case(
        "tpl-env",
        &|dir, _, _| {
            make_template(dir);
        },
        &env(&[("GIT_TEMPLATE_DIR", "tpl")]),
        &["init", "t5"],
    );
    // init.templatedir via -c, resolved under the shared $HOME for both twins.
    check_case(
        "tpl-config",
        &|_, _, home| {
            let tpl = home.join("tpl");
            std::fs::create_dir_all(&tpl).unwrap();
            std::fs::write(tpl.join("from-config"), "yes\n").unwrap();
        },
        &[],
        &["-c", "init.templatedir=@HOME@/tpl", "init", "t6"],
    );
    // ~/ expansion for init.templatedir.
    check_case(
        "tpl-config-tilde",
        &|_, _, home| {
            let tpl = home.join("tdir");
            std::fs::create_dir_all(&tpl).unwrap();
            std::fs::write(tpl.join("from-tilde"), "yes\n").unwrap();
        },
        &[],
        &["-c", "init.templatedir=~/tdir", "init", "t7"],
    );
}

#[test]
fn init_shared() {
    if git().is_none() {
        return;
    }
    check_case("shared", &no_setup, &[], &["init", "--shared", "s1"]);
    check_case("shared-group", &no_setup, &[], &["init", "--shared=group", "s2"]);
    check_case("shared-all", &no_setup, &[], &["init", "--shared=all", "s3"]);
    check_case("shared-0660", &no_setup, &[], &["init", "--shared=0660", "s4"]);
    check_case("shared-0666", &no_setup, &[], &["init", "--shared=0666", "s5"]);
    check_case("shared-false", &no_setup, &[], &["init", "--shared=false", "s6"]);
    check_case("shared-bad", &no_setup, &[], &["init", "--shared=banana", "s7"]);
    check_case("shared-bare", &no_setup, &[], &["init", "--bare", "--shared=0666", "s8.git"]);
    // Global core.sharedRepository applies without --shared ...
    check_case(
        "shared-global",
        &|_, _, home| {
            std::fs::write(home.join(".gitconfig"), "[core]\n\tsharedRepository = 0666\n").unwrap();
        },
        &[],
        &["init", "s9"],
    );
    // ... and --shared overrides it.
    check_case(
        "shared-global-override",
        &|_, _, home| {
            std::fs::write(home.join(".gitconfig"), "[core]\n\tsharedRepository = 0640\n").unwrap();
        },
        &[],
        &["init", "--shared=group", "s10"],
    );
}

#[test]
fn init_object_and_ref_formats() {
    if git().is_none() {
        return;
    }
    check_case("sha256", &no_setup, &[], &["init", "--object-format=sha256", "fsha"]);
    check_case("sha1", &no_setup, &[], &["init", "--object-format=sha1", "fsha1"]);
    check_case("bad-hash", &no_setup, &[], &["init", "--object-format=bad", "x"]);
    check_case("bad-ref", &no_setup, &[], &["init", "--ref-format=garbage", "x"]);
    check_case("ref-files", &no_setup, &[], &["init", "--ref-format=files", "fref"]);
    check_case("env-hash", &no_setup, &env(&[("GIT_DEFAULT_HASH", "sha256")]), &["init", "eh"]);
    check_case(
        "env-hash-bad",
        &no_setup,
        &env(&[("GIT_DEFAULT_HASH", "bogus")]),
        &["init", "ehb"],
    );
    check_case(
        "cfg-hash",
        &|_, _, home| {
            std::fs::write(home.join(".gitconfig"), "[init]\n\tdefaultObjectFormat = sha256\n")
                .unwrap();
        },
        &[],
        &["init", "ch"],
    );
    check_case(
        "cfg-hash-bad",
        &|_, _, home| {
            std::fs::write(home.join(".gitconfig"), "[init]\n\tdefaultObjectFormat = bogus\n")
                .unwrap();
        },
        &[],
        &["init", "chb"],
    );
    check_case(
        "reinit-hash-same",
        &|dir, bin, home| {
            assert!(setup_cmd(bin, dir, home)
                .args(["init", "-q", "--object-format=sha256", "r1"])
                .status()
                .unwrap()
                .success());
        },
        &env(&[("GIT_DEFAULT_HASH", "sha256")]),
        &["init", "r1"],
    );
    check_case(
        "reinit-hash-diff",
        &|dir, bin, home| {
            assert!(setup_cmd(bin, dir, home)
                .args(["init", "-q", "r2"])
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "--object-format=sha256", "r2"],
    );
    check_case(
        "reinit-ref-diff",
        &|dir, bin, home| {
            assert!(setup_cmd(bin, dir, home)
                .args(["init", "-q", "r3"])
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "--ref-format=reftable", "r3"],
    );
}

#[test]
fn init_env_layout() {
    if git().is_none() {
        return;
    }
    check_case(
        "gitdir-bare",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("g.git")).unwrap();
        },
        &env(&[("GIT_DIR", "g.git")]),
        &["init"],
    );
    check_case(
        "gitdir-dotgit",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("nb")).unwrap();
        },
        &env(&[("GIT_DIR", ".git")]),
        &["init", "nb"],
    );
    check_case(
        "gitdir-worktree",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("gw.git")).unwrap();
        },
        &env(&[("GIT_DIR", "gw.git"), ("GIT_WORK_TREE", ".")]),
        &["init"],
    );
    // GIT_WORK_TREE without GIT_DIR must fail; --bare with worktree too.
    check_case("wt-alone-fails", &no_setup, &env(&[("GIT_WORK_TREE", "w")]), &["init"]);
    check_case(
        "bare-wt-fails",
        &no_setup,
        &env(&[("GIT_WORK_TREE", "w")]),
        &["init", "--bare", "bw"],
    );
    // Explicit global --bare makes a bare repo; CLI operand beats GIT_DIR.
    check_case("global-bare", &no_setup, &[], &["--bare", "init", "gb"]);
    check_case("global-bare-no-operand", &no_setup, &[], &["--bare", "init"]);
    check_case(
        "cli-beats-gitdir",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("otherdir")).unwrap();
        },
        &env(&[("GIT_DIR", "otherdir")]),
        &["--bare", "init", "newdir"],
    );
    check_case(
        "objdir",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("custom-odb")).unwrap();
        },
        &env(&[("GIT_OBJECT_DIRECTORY", "custom-odb")]),
        &["init", "odb"],
    );
}

#[test]
fn init_separate_errors() {
    if git().is_none() {
        return;
    }
    check_case(
        "explicit-bare-sep",
        &no_setup,
        &[],
        &["init", "--bare", "--separate-git-dir", "g.git", "b.git"],
    );
    check_case(
        "implicit-bare-sep",
        &|dir, _, _| {
            std::fs::create_dir_all(dir.join("bare.git")).unwrap();
        },
        &env(&[("GIT_DIR", ".")]),
        &["init", "--separate-git-dir", "goop.git"],
    );
}

#[test]
fn init_arg_errors() {
    if git().is_none() {
        return;
    }
    check_case("unknown-opt", &no_setup, &[], &["init", "--bogus"]);
    check_case("unknown-switch", &no_setup, &[], &["init", "-Z"]);
    check_case("ambiguous", &no_setup, &[], &["init", "--s"]);
    check_case("extra-args", &no_setup, &[], &["init", "a", "b"]);
    check_case("missing-template", &no_setup, &[], &["init", "--template"]);
    check_case("missing-b", &no_setup, &[], &["init", "-b"]);
    check_case("takes-no-value", &no_setup, &[], &["init", "--bare=x", "foo"]);
    check_case("help", &no_setup, &[], &["init", "-h"]);
}

#[test]
fn init_fs_errors() {
    if git().is_none() {
        return;
    }
    check_case(
        "eexist-file",
        &|dir, _, _| {
            std::fs::write(dir.join("blockfile"), "x").unwrap();
        },
        &[],
        &["init", "blockfile"],
    );
    check_case(
        "eexist-mid",
        &|dir, _, _| {
            std::fs::write(dir.join("a"), "x").unwrap();
        },
        &[],
        &["init", "a/b"],
    );
    #[cfg(unix)]
    check_case(
        "eperm",
        &|dir, _, _| {
            use std::os::unix::fs::PermissionsExt;
            let d = dir.join("noaccess");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
        },
        &[],
        &["init", "noaccess/sub"],
    );
}

#[test]
fn init_gitfile_reinit() {
    if git().is_none() {
        return;
    }
    // Re-init with cwd inside a work tree whose .git is a gitfile.
    check_case_in(
        "gitfile",
        &|dir, bin, home| {
            assert!(setup_cmd(bin, dir, home)
                .args(["init", "--separate-git-dir", "real", "wt"])
                .status()
                .unwrap()
                .success());
        },
        &[],
        "wt",
        &["init"],
    );
}
