//! Crosswise tests for Phase B item B1 (`git init`) against the system C git.
//! Skips when no system `git` is available.
//!
//! The environment is pinned in every test (`GIT_CONFIG_NOSYSTEM` /
//! `GIT_CONFIG_GLOBAL` / a controlled `HOME`) so platform config cannot make
//! the Rust port and the oracle disagree for environmental reasons.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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

fn rust_git() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/git");
    p.canonicalize().ok()
}

fn tempdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-b1-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Run `exe args...` in `dir` with a hermetic environment.
fn run(exe: &Path, dir: &Path, args: &[&str]) -> (String, String, i32) {
    let out = run_raw(exe, dir, args, &[]);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(128),
    )
}

fn run_raw(exe: &Path, dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let home = tempdir("home");
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", &home);
    cmd.env("TMPDIR", std::env::temp_dir());
    cmd.env("LC_ALL", "C");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    // A permissive committer identity is harmless for init.
    cmd.env("GIT_AUTHOR_NAME", "T");
    cmd.env("GIT_AUTHOR_EMAIL", "t@example.com");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn")
}

/// Replace the absolute path of `dir` (and its parent) with `<D>` so two
/// sibling fixtures can be compared.
fn norm(text: &str, dir: &Path) -> String {
    let mut s = text.replace(&dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()).display().to_string(), "<D>");
    // Some messages use the parent-prefixed form.
    if let Some(parent) = dir.parent() {
        s = s.replace(&parent.display().to_string(), "<P>");
    }
    s
}

/// Sorted relative paths under `.git` (or the bare dir).
fn tree(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let md = e.metadata().ok();
            if md.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                out.push(format!("{rel}/"));
                stack.push(p);
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn plain_init_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let root = tempdir("plain");
    let rdir = root.join("r");
    let cdir = root.join("c");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();

    let (rout, rerr, rcode) = run(&rust, &rdir, &["init"]);
    let (cout, cerr, ccode) = run(Path::new(git().unwrap()), &cdir, &["init"]);

    assert_eq!(rcode, ccode, "exit codes differ");
    assert_eq!(norm(&rout, &rdir), norm(&cout, &cdir), "stdout differs");
    assert_eq!(norm(&rerr, &rdir), norm(&cerr, &cdir), "stderr differs");

    // Core files must match byte-for-byte.
    assert_eq!(
        std::fs::read_to_string(rdir.join(".git/HEAD")).unwrap(),
        std::fs::read_to_string(cdir.join(".git/HEAD")).unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(rdir.join(".git/config")).unwrap(),
        std::fs::read_to_string(cdir.join(".git/config")).unwrap(),
    );
    // Same file/dir layout (templates, refs, objects).
    assert_eq!(tree(&rdir.join(".git")), tree(&cdir.join(".git")), ".git layout differs");

    // C git must accept the Rust-created repo.
    let (out, _, code) = run(
        Path::new(git().unwrap()),
        &rdir,
        &["rev-parse", "--is-bare-repository"],
    );
    assert_eq!((out.trim(), code), ("false", 0));
    let (out, _, _) = run(Path::new(git().unwrap()), &rdir, &["symbolic-ref", "HEAD"]);
    assert_eq!(out.trim(), "refs/heads/master");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn configured_default_branch_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let root = tempdir("dbranch");
    // A controlled global config so both implementations see the same
    // `init.defaultBranch` (system config is suppressed in `run_raw`).
    let globalcfg = root.join("globalconfig");
    std::fs::write(&globalcfg, "[init]\n\tdefaultBranch = main\n").unwrap();
    let globalcfg_s = globalcfg.display().to_string();

    let rdir = root.join("r");
    let cdir = root.join("c");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();

    let env: &[(&str, &str)] = &[("GIT_CONFIG_GLOBAL", &globalcfg_s)];
    let r = run_raw(&rust, &rdir, &["init"], env);
    let c = run_raw(Path::new(git().unwrap()), &cdir, &["init"], env);
    let (rout, rerr) = (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
    );
    let (cout, cerr) = (
        String::from_utf8_lossy(&c.stdout).into_owned(),
        String::from_utf8_lossy(&c.stderr).into_owned(),
    );
    assert_eq!(norm(&rout, &rdir), norm(&cout, &cdir), "stdout differs");
    assert_eq!(norm(&rerr, &rdir), norm(&cerr, &cdir), "stderr differs");
    assert_eq!(
        std::fs::read_to_string(rdir.join(".git/HEAD")).unwrap(),
        std::fs::read_to_string(cdir.join(".git/HEAD")).unwrap(),
    );
    assert_eq!(std::fs::read_to_string(rdir.join(".git/HEAD")).unwrap(), "ref: refs/heads/main\n");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn initial_branch_flag_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    for (flag, value) in [("-b", "trunk"), ("--initial-branch", "feature/x")] {
        let root = tempdir("ibranch");
        let rdir = root.join("r");
        let cdir = root.join("c");
        std::fs::create_dir_all(&rdir).unwrap();
        std::fs::create_dir_all(&cdir).unwrap();
        let r = run(&rust, &rdir, &["init", flag, value]);
        let c = run(Path::new(git().unwrap()), &cdir, &["init", flag, value]);
        assert_eq!(r.2, c.2, "{flag}: exit");
        assert_eq!(norm(&r.0, &rdir), norm(&c.0, &cdir), "{flag}: stdout");
        assert_eq!(norm(&r.1, &rdir), norm(&c.1, &cdir), "{flag}: stderr");
        assert_eq!(
            std::fs::read_to_string(rdir.join(".git/HEAD")).unwrap(),
            std::fs::read_to_string(cdir.join(".git/HEAD")).unwrap(),
            "{flag}: HEAD"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn bare_init_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let root = tempdir("bare");
    let rdir = root.join("r.git");
    let cdir = root.join("c.git");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();

    let r = run(&rust, &rdir, &["init", "--bare"]);
    let c = run(Path::new(git().unwrap()), &cdir, &["init", "--bare"]);
    assert_eq!(r.2, c.2);
    assert_eq!(norm(&r.0, &rdir), norm(&c.0, &cdir), "stdout");
    assert_eq!(norm(&r.1, &rdir), norm(&c.1, &cdir), "stderr");
    assert_eq!(
        std::fs::read_to_string(rdir.join("config")).unwrap(),
        std::fs::read_to_string(cdir.join("config")).unwrap(),
        "config"
    );
    assert_eq!(tree(&rdir), tree(&cdir), "bare layout differs");
    let (out, _, _) = run(Path::new(git().unwrap()), &rdir, &["rev-parse", "--is-bare-repository"]);
    assert_eq!(out.trim(), "true");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn separate_git_dir_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let root = tempdir("sep");
    let rwt = root.join("rwt");
    let cwt = root.join("cwt");
    std::fs::create_dir_all(&rwt).unwrap();
    std::fs::create_dir_all(&cwt).unwrap();
    let rgit = root.join("rgd");
    let cgit = root.join("cgd");

    let r = run(&rust, &rwt, &["init", "--separate-git-dir", &rgit.display().to_string()]);
    let c = run(
        Path::new(git().unwrap()),
        &cwt,
        &["init", "--separate-git-dir", &cgit.display().to_string()],
    );
    assert_eq!(r.2, c.2);
    let rlink = std::fs::read_to_string(rwt.join(".git")).unwrap();
    let clink = std::fs::read_to_string(cwt.join(".git")).unwrap();
    assert!(rlink.starts_with("gitdir: "), "rust link: {rlink:?}");
    assert!(clink.starts_with("gitdir: "), "c link: {clink:?}");
    // Both must point at the canonical absolute git dir.
    let rtgt = PathBuf::from(rlink.trim_start_matches("gitdir: ").trim());
    let ctgt = PathBuf::from(clink.trim_start_matches("gitdir: ").trim());
    assert_eq!(rtgt.canonicalize().unwrap(), rgit.canonicalize().unwrap());
    assert_eq!(ctgt.canonicalize().unwrap(), cgit.canonicalize().unwrap());
    // C git must resolve the Rust-created separated worktree.
    let (out, _, code) = run(Path::new(git().unwrap()), &rwt, &["rev-parse", "--git-dir"]);
    assert_eq!(code, 0, "C failed to read separated worktree");
    assert_eq!(out.trim(), rgit.canonicalize().unwrap().display().to_string());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reinit_message_and_config_preserved() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    for exe in [&rust, Path::new(git().unwrap())] {
        let root = tempdir("reinit");
        let (_, _, code) = run(exe, &root, &["init"]);
        assert_eq!(code, 0);
        // Add a user setting; a re-init must keep it.
        let cfg_path = root.join(".git/config");
        let mut cfg = std::fs::read_to_string(&cfg_path).unwrap();
        cfg.push_str("[user]\n\tname = Keep Me\n");
        std::fs::write(&cfg_path, &cfg).unwrap();

        let (out, err, code) = run(exe, &root, &["init"]);
        assert_eq!(code, 0);
        assert!(out.contains("Reinitialized existing Git repository in"), "out: {out}");
        assert!(err.is_empty(), "unexpected stderr on reinit: {err}");
        let after = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(after.contains("name = Keep Me"), "config lost on reinit:\n{after}");
    }
}

#[test]
fn shared_group_matches_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let root = tempdir("shared");
    let rdir = root.join("r");
    let cdir = root.join("c");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();

    let r = run(&rust, &rdir, &["init", "--shared=group"]);
    let c = run(Path::new(git().unwrap()), &cdir, &["init", "--shared=group"]);
    assert_eq!(r.2, c.2);
    assert_eq!(norm(&r.1, &rdir), norm(&c.1, &cdir), "stderr");
    let rcfg = std::fs::read_to_string(rdir.join(".git/config")).unwrap();
    let ccfg = std::fs::read_to_string(cdir.join(".git/config")).unwrap();
    assert_eq!(rcfg, ccfg, "config differs");
    assert!(rcfg.contains("sharedrepository = 1"), "missing sharedrepository: {rcfg}");
    assert!(rcfg.contains("denyNonFastforwards = true"), "missing denyNonFastforwards: {rcfg}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn template_skip_and_custom_match_c_git() {
    let (Some(_git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    // `--template=` copies nothing at all.
    let root = tempdir("tpl-skip");
    let rdir = root.join("r");
    let cdir = root.join("c");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();
    let r = run(&rust, &rdir, &["init", "--template="]);
    let c = run(Path::new(git().unwrap()), &cdir, &["init", "--template="]);
    assert_eq!(r.2, c.2);
    assert_eq!(norm(&r.0, &rdir), norm(&c.0, &cdir));
    assert!(!rdir.join(".git/description").exists(), "description should be absent");
    assert_eq!(tree(&rdir.join(".git")), tree(&cdir.join(".git")));
    let _ = std::fs::remove_dir_all(&root);

    // A custom template directory is copied.
    let root = tempdir("tpl-custom");
    let tpl = root.join("tpl");
    std::fs::create_dir_all(tpl.join("info")).unwrap();
    std::fs::write(tpl.join("description"), "custom description\n").unwrap();
    std::fs::write(tpl.join("info/exclude"), "# custom exclude\n").unwrap();
    let rdir = root.join("r");
    let cdir = root.join("c");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::create_dir_all(&cdir).unwrap();
    let r = run(&rust, &rdir, &["init", &format!("--template={}", tpl.display())]);
    let c = run(
        Path::new(git().unwrap()),
        &cdir,
        &["init", &format!("--template={}", tpl.display())],
    );
    assert_eq!(r.2, c.2);
    assert_eq!(
        std::fs::read_to_string(rdir.join(".git/description")).unwrap(),
        "custom description\n"
    );
    assert_eq!(tree(&rdir.join(".git")), tree(&cdir.join(".git")));

    let _ = std::fs::remove_dir_all(&root);
}
