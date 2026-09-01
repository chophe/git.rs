//! Crosswise tests for Phase A item A9 (userdiff hunk headers) against
//! the system C git.

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
    let dir = std::env::temp_dir().join(format!("git-a9-xwise-{}-{n}", std::process::id()));
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

/*
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
*/

#[test]
fn userdiff_hunk_header_parity() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let run = |args: &[&str]| {
        let st = Command::new(git().unwrap()).args(args).current_dir(&dir).status().unwrap();
        assert!(st.success());
    };
    run(&["init", "-q", "-b", "main"]);
    
    // Test Rust function hunk header
    let rs = "fn RIGHT() {\n    let x = 1;\n    ChangeMe;\n}";
    std::fs::write(dir.join("f.rs"), rs).unwrap();
    run(&["add", "f.rs"]);
    run(&["commit", "-m", "one"]);
    
    let rs_new = "fn RIGHT() {\n    let x = 1;\n    IWasChanged;\n}";
    std::fs::write(dir.join("f.rs"), rs_new).unwrap();
    
    let real_output = with_cwd(&dir, || {
        let out = Command::new(git().unwrap()).args(["diff", "HEAD", "--", "f.rs"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    });
    
    let (our_output, _) = ours(&dir, &["diff", "HEAD", "--", "f.rs"]);
    
    assert_eq!(our_output, real_output, "Hunk header mismatch");
}
