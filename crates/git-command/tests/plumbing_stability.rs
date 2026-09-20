//! Plumbing-output stability (T013, porcelain/plumbing split edge case).
//!
//! Human-oriented formatting and display configuration must never alter
//! machine-readable plumbing output. Runs ported plumbing commands under
//! display-affecting configs (`color.ui=always`, `format.pretty=fuller`,
//! `core.abbrev`) and asserts byte-identical stdout to the plain run.
//! Skips without system git (used only to seed the fixture repo).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn system_git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn rust_git() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/debug/git")
        .canonicalize()
        .expect("rust git binary must be built (cargo build --workspace)")
}

fn tempdir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-plumbing-stable-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn sys(git: &str, dir: &Path, args: &[&str]) {
    let ok = Command::new(git)
        .args(args)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "system git {args:?} must succeed");
}

fn ours(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new(rust_git()).args(args).current_dir(dir).output().expect("run rust git");
    assert!(out.status.success(), "rust git {args:?} must succeed: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

fn seed() -> Option<PathBuf> {
    let git = system_git()?;
    let dir = tempdir();
    sys(git, &dir, &["init", "-q", "-b", "main"]);
    sys(git, &dir, &["config", "user.name", "Stable"]);
    sys(git, &dir, &["config", "user.email", "s@example.com"]);
    std::fs::write(dir.join("f.txt"), "one\ntwo\n").unwrap();
    sys(git, &dir, &["add", "-A"]);
    sys(git, &dir, &["commit", "-qm", "seed"]);
    Some(dir)
}

#[test]
fn display_config_cannot_alter_plumbing_output() {
    let Some(dir) = seed() else {
        return;
    };
    let head = String::from_utf8(ours(&dir, &["rev-parse", "HEAD"]))
        .expect("utf8 oid")
        .trim()
        .to_string();
    // (plumbing invocation, display-config overlays that must not leak in).
    // `cat-file` takes the resolved oid: rev-expression support in `cat-file`
    // itself is a separate parity gap (see FOLLOWUPS), not a stability axis.
    let commit_text =
        String::from_utf8(ours(&dir, &["cat-file", "-p", &head])).expect("utf8 commit");
    let tree = commit_text
        .lines()
        .find_map(|l| l.strip_prefix("tree "))
        .expect("commit lists its tree")
        .trim()
        .to_string();
    let blob = String::from_utf8(ours(&dir, &["hash-object", "f.txt"]))
        .expect("utf8 oid")
        .trim()
        .to_string();
    let cases: Vec<Vec<String>> = vec![
        vec!["hash-object".into(), "f.txt".into()],
        vec!["cat-file".into(), "-p".into(), blob],
        vec!["cat-file".into(), "-t".into(), head.clone()],
        vec!["ls-tree".into(), tree],
        vec!["rev-parse".into(), "HEAD".into()],
        vec!["log".into(), "--format=%H".into()],
    ];
    let overlays: &[&[&str]] = &[
        &[],
        &["-c", "color.ui=always"],
        &["-c", "format.pretty=fuller"],
        &["-c", "core.abbrev=4"],
    ];
    for case in &cases {
        let args: Vec<&str> = case.iter().map(|s| s.as_str()).collect();
        let plain = ours(&dir, &args);
        for overlay in overlays.iter().skip(1) {
            let mut shaded_args: Vec<&str> = overlay.to_vec();
            shaded_args.extend_from_slice(&args);
            let shaded = ours(&dir, &shaded_args);
            assert_eq!(
                plain, shaded,
                "plumbing {case:?} changed under display config {overlay:?}"
            );
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}
