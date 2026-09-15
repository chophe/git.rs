//! Crosswise tests for Phase B item B4 (`git write-tree` / `git read-tree`)
//! and the B2 cache-tree (`TREE`) index extension, against the system C git.
//! Skips when no system `git` is available.

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
    let dir = std::env::temp_dir().join(format!("git-b4-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(exe: &Path, dir: &Path, args: &[&str], extra: &[(&str, &str)]) -> (String, String, i32) {
    let out = run_raw(exe, dir, args, extra);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(128),
    )
}

fn run_raw(exe: &Path, dir: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let home = tempdir("home");
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", &home);
    cmd.env("TMPDIR", std::env::temp_dir());
    cmd.env("LC_ALL", "C");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    cmd.env("GIT_AUTHOR_NAME", "T");
    cmd.env("GIT_AUTHOR_EMAIL", "t@example.com");
    cmd.env("GIT_COMMITTER_NAME", "T");
    cmd.env("GIT_COMMITTER_EMAIL", "t@example.com");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn")
}

/// `git hash-object -w --stdin` with the hermetic environment.
fn hash_stdin(exe: &Path, dir: &Path, data: &str) -> String {
    use std::io::Write;
    let mut child = Command::new(exe)
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", tempdir("h"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(data.as_bytes()).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string()
}

/// Feed `data` to `git <args>` on stdin (used for `update-index --index-info`).
fn feed_stdin(exe: &Path, dir: &Path, args: &[&str], data: &str) {
    use std::io::Write;
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", tempdir("h"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(data.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repo with a nested tree, an executable, and a symlink, staged.
fn build_fixture(git: &str, dir: &Path) {
    run(Path::new(git), dir, &["init", "-q", "-b", "main"], &[]);
    std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
    std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("sub/b.txt"), "beta\n").unwrap();
    std::fs::write(dir.join("sub/deep/c.txt"), "gamma\n").unwrap();
    std::fs::write(dir.join("z last.txt"), "space in name\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("sub/b.txt"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::os::unix::fs::symlink("a.txt", dir.join("link.txt"));
    }
    let out = run_raw(Path::new(git), dir, &["add", "-A"], &[]);
    assert!(out.status.success(), "git add failed");
}

#[test]
fn write_tree_matches_c_and_refreshes_index() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("wt");
    build_fixture(git, &dir);
    let index_path = dir.join(".git/index");
    let original = std::fs::read(&index_path).unwrap();

    let (cout, _, ccode) = run(Path::new(git), &dir, &["write-tree"], &[]);
    let c_index = std::fs::read(&index_path).unwrap();
    std::fs::write(&index_path, &original).unwrap();
    let (rout, rerr, rcode) = run(&rust, &dir, &["write-tree"], &[]);
    let r_index = std::fs::read(&index_path).unwrap();

    assert_eq!(rout, cout, "write-tree output differs");
    assert_eq!(rcode, ccode);
    assert_eq!(rerr, "", "unexpected stderr: {rerr}");
    assert_eq!(r_index, c_index, "index (with cache-tree) differs after write-tree");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_tree_prefix_matches_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("wtp");
    build_fixture(git, &dir);

    for prefix in ["sub/", "sub/deep/"] {
        let arg = format!("--prefix={prefix}");
        let (cout, _, ccode) = run(Path::new(git), &dir, &["write-tree", &arg], &[]);
        let (rout, rerr, rcode) = run(&rust, &dir, &["write-tree", &arg], &[]);
        assert_eq!(rout, cout, "prefix {prefix}: output differs");
        assert_eq!(rerr, "", "prefix {prefix}: stderr {rerr}");
        assert_eq!(rcode, ccode);
    }
    // Missing prefix fails identically.
    let (_, cerr, ccode) = run(Path::new(git), &dir, &["write-tree", "--prefix=nope/"], &[]);
    let (_, rerr, rcode) = run(&rust, &dir, &["write-tree", "--prefix=nope/"], &[]);
    assert_eq!(cerr, rerr, "missing-prefix stderr differs");
    assert_eq!(ccode, rcode);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_tree_unmerged_errors_like_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("wtu");
    run(Path::new(git), &dir, &["init", "-q", "-b", "main"], &[]);
    let a = hash_stdin(Path::new(git), &dir, "a");
    let b = hash_stdin(Path::new(git), &dir, "b");
    let c = hash_stdin(Path::new(git), &dir, "c");
    let info = format!(
        "100644 {a} 1\tconflict\n100644 {b} 2\tconflict\n100644 {c} 3\tconflict\n"
    );
    feed_stdin(Path::new(git), &dir, &["update-index", "--index-info"], &info);

    let (cout, cerr, ccode) = run(Path::new(git), &dir, &["write-tree"], &[]);
    let (rout, rerr, rcode) = run(&rust, &dir, &["write-tree"], &[]);
    assert_eq!(rout, cout);
    assert_eq!(cerr, rerr, "unmerged stderr differs");
    assert_eq!(ccode, rcode);
    assert_eq!(ccode, 128);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_tree_index_matches_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("rt");
    build_fixture(git, &dir);
    let (tree, _, _) = run(Path::new(git), &dir, &["write-tree"], &[]);
    let tree = tree.trim().to_string();

    let cix = dir.join("cix");
    let rix = dir.join("rix");
    let (_, cerr, ccode) = run(
        Path::new(git),
        &dir,
        &["read-tree", &tree],
        &[("GIT_INDEX_FILE", cix.to_str().unwrap())],
    );
    let (_, rerr, rcode) = run(
        &rust,
        &dir,
        &["read-tree", &tree],
        &[("GIT_INDEX_FILE", rix.to_str().unwrap())],
    );
    assert_eq!(ccode, rcode);
    assert_eq!(cerr, rerr);
    assert_eq!(std::fs::read(&cix).unwrap(), std::fs::read(&rix).unwrap(), "read-tree index differs");

    // Both indexes must be readable by C and yield the same tree.
    let (ct, _, _) = run(Path::new(git), &dir, &["write-tree"], &[("GIT_INDEX_FILE", cix.to_str().unwrap())]);
    let (rt, _, _) = run(&rust, &dir, &["write-tree"], &[("GIT_INDEX_FILE", rix.to_str().unwrap())]);
    assert_eq!(ct.trim(), rt.trim());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_tree_empty_dry_run_and_errors_match_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("rte");
    build_fixture(git, &dir);
    let (tree, _, _) = run(Path::new(git), &dir, &["write-tree"], &[]);
    let tree = tree.trim().to_string();

    // --empty
    let cix = dir.join("cix");
    let rix = dir.join("rix");
    run(Path::new(git), &dir, &["read-tree", "--empty"], &[("GIT_INDEX_FILE", cix.to_str().unwrap())]);
    run(&rust, &dir, &["read-tree", "--empty"], &[("GIT_INDEX_FILE", rix.to_str().unwrap())]);
    assert_eq!(std::fs::read(&cix).unwrap(), std::fs::read(&rix).unwrap(), "--empty index differs");

    // -n (dry run) leaves the index untouched.
    let before = std::fs::read(&rix).unwrap();
    run(&rust, &dir, &["read-tree", "-n", &tree], &[("GIT_INDEX_FILE", rix.to_str().unwrap())]);
    assert_eq!(std::fs::read(&rix).unwrap(), before, "-n should not write the index");

    // Nonexistent object.
    let (_, cerr, ccode) = run(Path::new(git), &dir, &["read-tree", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"], &[]);
    let (_, rerr, rcode) = run(&rust, &dir, &["read-tree", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"], &[]);
    assert_eq!(cerr, rerr, "bad-object stderr differs");
    assert_eq!(ccode, rcode);

    // A blob is not a tree.
    let blob = hash_stdin(Path::new(git), &dir, "just a blob");
    let (_, cerr, ccode) = run(Path::new(git), &dir, &["read-tree", &blob], &[]);
    let (_, rerr, rcode) = run(&rust, &dir, &["read-tree", &blob], &[]);
    assert_eq!(cerr, rerr, "blob-not-tree stderr differs");
    assert_eq!(ccode, rcode);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_tree_index_output_flag_matches_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("rt-ixout");
    build_fixture(git, &dir);
    let (tree, _, _) = run(Path::new(git), &dir, &["write-tree"], &[]);
    let tree = tree.trim().to_string();

    let cout = dir.join("cout");
    let rout = dir.join("rout");
    let carg = format!("--index-output={}", cout.display());
    let rarg = format!("--index-output={}", rout.display());
    let (_, cerr, ccode) = run(Path::new(git), &dir, &["read-tree", &carg, &tree], &[]);
    let (_, rerr, rcode) = run(&rust, &dir, &["read-tree", &rarg, &tree], &[]);
    assert_eq!(ccode, rcode);
    assert_eq!(cerr, rerr);
    assert_eq!(std::fs::read(&cout).unwrap(), std::fs::read(&rout).unwrap(), "--index-output differs");

    let _ = std::fs::remove_dir_all(&dir);
}
