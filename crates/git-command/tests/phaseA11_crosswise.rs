//! Crosswise tests for Phase A item A11 (`.gitignore` + `.gitattributes`
//! engine) against the system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

static COUNTER: AtomicU32 = AtomicU32::new(0);
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn tempdir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-a11-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn with_cwd<R>(dir: &Path, f: impl FnOnce() -> R) -> R {
    let _guard = CWD_LOCK.lock().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::env::set_current_dir(prev).unwrap();
    match r {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}

fn ours(dir: &Path, args: &[&str]) -> (String, i32) {
    with_cwd(dir, || {
        let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/debug/git")
            .canonicalize()
            .expect("rust git binary must be built");
        let out = Command::new(exe).args(args).output().unwrap();
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (text, out.status.code().unwrap_or(128))
    })
}

fn check(dir: &Path, args: &[&str]) {
    let out = Command::new(git().unwrap())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let mut rtext = String::from_utf8_lossy(&out.stdout).into_owned();
    rtext.push_str(&String::from_utf8_lossy(&out.stderr));
    let rcode = out.status.code().unwrap_or(128);
    let (otext, ocode) = ours(dir, args);
    assert_eq!(rcode, ocode, "args {args:?}");
    assert_eq!(rtext, otext, "args {args:?}\nreal: {rtext}\nours: {otext}");
}

/// Build a directory tree with tricky `.gitignore` patterns.
fn build_ignore_fixture() -> PathBuf {
    let dir = tempdir();
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(&dir).status().unwrap();
    assert!(st.success());
    std::fs::write(
        dir.join(".gitignore"),
        "one\nignored-*\nbuild/\n*.log\n!keep.log\n?oo\nfoo/**/bar\ntrail  \n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("deep")).unwrap();
    std::fs::write(dir.join("deep/.gitignore"), "*.tmp\nnested/\n!special.tmp\n").unwrap();
    std::fs::create_dir_all(dir.join("deep/nested")).unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/.gitignore"), "*.o\n!special.o\n").unwrap();
    std::fs::create_dir_all(dir.join("build")).unwrap();
    std::fs::create_dir_all(dir.join("foo/a/b/bar")).unwrap();
    std::fs::write(dir.join("one"), "x").unwrap();
    std::fs::write(dir.join("two"), "x").unwrap();
    std::fs::write(dir.join("ignored.txt"), "x").unwrap();
    std::fs::write(dir.join("keep.log"), "x").unwrap();
    std::fs::write(dir.join("a.log"), "x").unwrap();
    std::fs::write(dir.join("build/out.o"), "x").unwrap();
    std::fs::write(dir.join("boo"), "x").unwrap();
    std::fs::write(dir.join("deep/x.tmp"), "x").unwrap();
    std::fs::write(dir.join("deep/special.tmp"), "x").unwrap();
    std::fs::write(dir.join("deep/nested/ignored.tmp"), "x").unwrap();
    std::fs::write(dir.join("sub/other.o"), "x").unwrap();
    std::fs::write(dir.join("sub/special.o"), "x").unwrap();
    std::fs::write(dir.join("foo/a/b/bar/f"), "x").unwrap();
    std::fs::write(dir.join("trail"), "x").unwrap();
    dir
}

#[test]
fn check_ignore_verbose_parity() {
    if git().is_none() {
        return;
    }
    let dir = build_ignore_fixture();
    let paths = [
        "one",
        "two",
        "ignored.txt",
        "keep.log",
        "a.log",
        "build",
        "build/out.o",
        "boo",
        "deep/x.tmp",
        "deep/special.tmp",
        "deep/nested/ignored.tmp",
        "sub/other.o",
        "sub/special.o",
        "foo/a/b/bar/f",
        "trail",
    ];
    let modes: &[&[&str]] = &[&[], &["-v"], &["-v", "-n"], &["-q"]];
    for mode in modes.iter() {
        for p in paths.iter().copied() {
            let mut args: Vec<&str> = vec!["check-ignore"];
            args.extend_from_slice(&mode);
            args.extend_from_slice(&["--", p]);
            check(&dir, &args);
        }
    }
}

#[test]
fn check_attr_parity() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(&dir).status().unwrap();
    assert!(st.success());
    std::fs::write(
        dir.join(".gitattributes"),
        "*.c text\n*.o -text\nfoo/* binary\n**/Makefile whitespace=tab-in-indent\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("foo")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("a.c"), "x").unwrap();
    std::fs::write(dir.join("b.o"), "x").unwrap();
    std::fs::write(dir.join("foo/x.bin"), "x").unwrap();
    std::fs::write(dir.join("src/Makefile"), "x").unwrap();
    let files = ["a.c", "b.o", "foo/x.bin", "src/Makefile", "src/noattr.txt"];
    let attrs: &[&[&str]] = &[&["-a"], &["-a", "-z"], &["text", "diff", "binary"], &["text", "-z"]];
    for file in files.iter().copied() {
        for a in attrs.iter() {
            let mut args: Vec<&str> = vec!["check-attr"];
            args.extend_from_slice(a);
            args.extend_from_slice(&["--", file]);
            check(&dir, &args);
        }
    }
}
