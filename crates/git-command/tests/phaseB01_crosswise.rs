//! Crosswise tests for Phase B item B1 (`git init`) against the system C
//! git. Skips when no system `git` is available.
//!
//! Each case runs both binaries in fresh twin directories with identical,
//! tightly controlled environments (fresh `$HOME`, `GIT_CONFIG_NOSYSTEM=1`
//! unless the case says otherwise) and asserts byte-identical
//! stdout/stderr/exit code plus identical trees, file bytes, and file modes.

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

struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
    /// relpath -> (bytes with @D@, mode bits, symlink target or None)
    files: BTreeMap<String, (Vec<u8>, u32, Option<String>)>,
}

fn snapshotted(dir: &Path, here: &Path) -> BTreeMap<String, (Vec<u8>, u32, Option<String>)> {
    let mut map = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
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
            let mut bytes = std::fs::read(&path).unwrap_or_default();
            // Normalize the twin dir prefix (messages embed absolute paths).
            let from = here.to_string_lossy().into_owned();
            let bytes_str = String::from_utf8_lossy(&bytes).replace(&from, "@D@");
            bytes = bytes_str.into_bytes();
            map.insert(rel, (bytes, mode, link));
        }
    }
    map
}

fn run_one(
    bin: &str,
    dir: &Path,
    home: &Path,
    extra_env: &[(String, String)],
    args: &[&str],
) -> Outcome {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir).args(args);
    cmd.env("HOME", home);
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("LC_ALL", "C");
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
    ] {
        cmd.env_remove(v);
    }
    // Scrub ambient numbered config env (credential.* in this shell).
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_CONFIG_KEY_") || k.starts_with("GIT_CONFIG_VALUE_") {
            cmd.env_remove(k);
        }
    }
    for (k, v) in extra_env {
        if v.is_empty() {
            cmd.env_remove(k);
        } else {
            cmd.env(k, v);
        }
    }
    let out = cmd.output().expect("git runs");
    let here = dir.to_string_lossy().into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).replace(&here, "@D@");
    let stderr = String::from_utf8_lossy(&out.stderr).replace(&here, "@D@");
    Outcome {
        stdout,
        stderr,
        code: out.status.code().unwrap_or(128),
        files: snapshotted(dir, dir),
    }
}

fn check_case(
    name: &str,
    setup: &dyn Fn(&Path, &str),
    extra_env: &[(String, String)],
    args: &[&str],
) {
    let real = git().expect("system git required");
    let ours = rust_git();
    let ours = ours.to_str().unwrap();
    let home = tempdir(&format!("{name}-home"));
    let c = tempdir(&format!("{name}-c"));
    let r = tempdir(&format!("{name}-r"));
    setup(&c, real);
    setup(&r, ours);
    let a = run_one(real, &c, &home, extra_env, args);
    let b = run_one(ours, &r, &home, extra_env, args);
    assert_eq!(a.code, b.code, "[{name}] exit code");
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

fn no_setup(_dir: &Path, _bin: &str) {}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Write `$HOME/.gitconfig`-style global config for cases that need one.
fn write_global(home: &Path, content: &str) {
    std::fs::write(home.join(".gitconfig"), content).unwrap();
}

/// A template fixture: regular file, hook, info file, and a config that
/// overrides one of our defaults plus an extra key.
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
    check_case(
        "reinit",
        &|dir, bin| {
            assert!(Command::new(bin)
                .args(["init", "-q", "-b", "keep", "x"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "x"],
    );
    check_case(
        "reinit-branch",
        &|dir, bin| {
            assert!(Command::new(bin)
                .args(["init", "-q", "-b", "keep", "x"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "--initial-branch=ignore", "x"],
    );
}

#[test]
fn init_branch_selection() {
    if git().is_none() {
        return;
    }
    // Unconfigured default: master + advice hint (system config suppressed).
    check_case("default-hint", &no_setup, &[], &["init", "h1"]);
    check_case(
        "cli-config-branch",
        &no_setup,
        &[],
        &["-c", "init.defaultBranch=nmb", "init", "h2"],
    );
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
}

#[test]
fn init_templates() {
    if git().is_none() {
        return;
    }
    check_case(
        "tpl-custom",
        &|dir, _| {
            make_template(dir);
        },
        &[],
        &["init", "--template=tpl", "t1"],
    );
    check_case(
        "tpl-relative-sep",
        &|dir, _| {
            make_template(dir);
        },
        &[],
        &["init", "--template", "tpl", "t1b"],
    );
    check_case("tpl-empty", &no_setup, &[], &["init", "--template=", "t2"]);
    check_case(
        "tpl-missing",
        &no_setup,
        &[],
        &["init", "--template=/nonexistent-xyz-pdq", "t3"],
    );
    check_case(
        "tpl-long",
        &no_setup,
        &[],
        &[&format!("--template={}", "x".repeat(9999)), "t4"],
    );
    check_case(
        "tpl-env",
        &|dir, _| {
            make_template(dir);
        },
        &env(&[("GIT_TEMPLATE_DIR", "tpl")]),
        &["init", "t5"],
    );
    // init.templatedir via -c, plus ~/ expansion through $HOME.
    check_case(
        "tpl-config",
        &|dir, _| {
            // Twin-local dir referenced by absolute path through -c.
            let tpl = make_template(dir);
            std::fs::write(dir.join("tpl-path"), tpl.to_string_lossy().into_owned()).unwrap();
        },
        &[],
        &["init", "t6"],
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
    check_case(
        "shared-bare",
        &no_setup,
        &[],
        &["init", "--bare", "--shared=0666", "s8.git"],
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
    check_case(
        "env-hash",
        &no_setup,
        &env(&[("GIT_DEFAULT_HASH", "sha256")]),
        &["init", "eh"],
    );
    check_case(
        "env-hash-bad",
        &no_setup,
        &env(&[("GIT_DEFAULT_HASH", "bogus")]),
        &["init", "ehb"],
    );
    check_case(
        "reinit-hash-same",
        &|dir, bin| {
            assert!(Command::new(bin)
                .args(["init", "-q", "--object-format=sha256", "r1"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        },
        &env(&[("GIT_DEFAULT_HASH", "sha256")]),
        &["init", "r1"],
    );
    check_case(
        "reinit-hash-diff",
        &|dir, bin| {
            assert!(Command::new(bin)
                .args(["init", "-q", "r2"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "--object-format=sha256", "r2"],
    );
}

#[test]
fn init_env_layout() {
    if git().is_none() {
        return;
    }
    check_case(
        "gitdir-bare",
        &|dir, _| {
            std::fs::create_dir_all(dir.join("g.git")).unwrap();
        },
        &env(&[("GIT_DIR", "g.git")]),
        &["init"],
    );
    check_case(
        "gitdir-dotgit",
        &|dir, _| {
            std::fs::create_dir_all(dir.join("nb")).unwrap();
        },
        &env(&[("GIT_DIR", ".git")]),
        &["init", "nb"],
    );
    check_case(
        "gitdir-worktree",
        &|dir, _| {
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
    check_case(
        "cli-beats-gitdir",
        &|dir, _| {
            std::fs::create_dir_all(dir.join("otherdir")).unwrap();
        },
        &env(&[("GIT_DIR", "otherdir")]),
        &["--bare", "init", "newdir"],
    );
    check_case(
        "objdir",
        &|dir, _| {
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
        &|dir, _| {
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
        &|dir, _| {
            std::fs::write(dir.join("blockfile"), "x").unwrap();
        },
        &[],
        &["init", "blockfile"],
    );
    check_case(
        "eexist-mid",
        &|dir, _| {
            std::fs::write(dir.join("a"), "x").unwrap();
        },
        &[],
        &["init", "a/b"],
    );
    #[cfg(unix)]
    check_case(
        "eperm",
        &|dir, _| {
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
    // Re-init inside a work tree whose .git is a gitfile.
    check_case(
        "gitfile",
        &|dir, bin| {
            assert!(Command::new(bin)
                .args(["init", "--separate-git-dir", "real", "wt"])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        },
        &[],
        &["init", "wt"],
    );
}

#[test]
fn init_global_shared_and_branch_config() {
    if git().is_none() {
        return;
    }
    // Global core.sharedRepository applies without --shared ...
    let setup_shared = |dir: &Path, _bin: &str| {
        write_global(dir, "[core]\n\tsharedRepository = 0666\n");
    };
    // ... but --shared overrides it. $HOME must point at the twin dir, so
    // build the home inside setup and re-run via a wrapper below.
    for (name, args) in [
        ("g-shared-plain", vec!["init", "g1"]),
        ("g-shared-override", vec!["init", "--shared=group", "g2"]),
        ("g-branch", vec!["-c", "init.defaultBranch=nmb", "init", "g3"]),
    ] {
        let real = git().unwrap();
        let ours = rust_git();
        let ours = ours.to_str().unwrap();
        let c = tempdir(&format!("{name}-c"));
        let r = tempdir(&format!("{name}-r"));
        setup_shared(&c, real);
        setup_shared(&r, ours);
        // Point $HOME at each twin so the "global" config differs per side
        // but carries identical content.
        let ac = run_one(real, &c, &c, &[], &args);
        let bc = run_one(ours, &r, &r, &[], &args);
        assert_eq!(ac.code, bc.code, "[{name}] exit");
        assert_eq!(ac.stdout, bc.stdout, "[{name}] stdout");
        assert_eq!(ac.stderr, bc.stderr, "[{name}] stderr");
        assert_eq!(
            ac.files.keys().collect::<Vec<_>>(),
            bc.files.keys().collect::<Vec<_>>(),
            "[{name}] tree"
        );
    }
    let _ = setup_shared;
}
