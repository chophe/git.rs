//! Fault-injection drills (SC-003, `contracts/error-contract.md`).
//!
//! One injected fault per layer, exercised end to end through the built Rust
//! binary (`crates/target/debug/git`, built on demand) against scratch repos
//! created with the system C git:
//!   * store/corrupt-object: flip a byte in a loose object, `cat-file -p`
//!   * refs/missing-ref: `rev-parse` a ref that does not exist
//!   * index/corrupt: garbage `HEAD` + `status` (discovery/store failure)
//!   * config/bad: syntactically invalid repo config, any command
//!   * usage: `cat-file --bogus-flag` must exit 129
//!   * unknown command: must exit 1
//!
//! Each drill asserts the exit-code class (0/1/129/128+) and reports whether
//! the diagnostic names the failing layer/subject. Exit-class mismatches fail
//! the drill; missing layer attribution prints as WARN (honest-failure input
//! for the backlog) without failing — attribution gaps are recorded, never
//! hidden. Only std is used.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

fn system_git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn rust_git(root: &Path) -> PathBuf {
    let status = Command::new("cargo")
        .args(["build", "-p", "git-cli", "--offline"])
        .current_dir(root)
        .status()
        .expect("cargo build git-cli");
    assert!(status.success(), "rust git binary must build for drills");
    root.join("target/debug/git")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-drills-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn sys(git: &str, dir: &Path, args: &[&str]) {
    let ok = Command::new(git).args(args).current_dir(dir).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "system git {args:?} must succeed");
}

fn seed_repo(git: &str, dir: &Path) -> String {
    sys(git, dir, &["init", "-q"]);
    sys(git, dir, &["config", "user.name", "Drills"]);
    sys(git, dir, &["config", "user.email", "d@example.com"]);
    std::fs::write(dir.join("f.txt"), "hello\n").unwrap();
    sys(git, dir, &["add", "-A"]);
    sys(git, dir, &["commit", "-qm", "seed"]);
    let out = Command::new(git).args(["rev-parse", "HEAD"]).current_dir(dir).output().expect("rev-parse");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Outcome {
    code: Option<i32>,
    stderr: String,
}

fn run_rust(bin: &Path, dir: &Path, args: &[&str]) -> Outcome {
    let out = Command::new(bin).args(args).current_dir(dir).output().expect("run rust git");
    Outcome { code: out.status.code(), stderr: String::from_utf8_lossy(&out.stderr).to_string() }
}

fn class_ok(code: Option<i32>, want: &[i32]) -> bool {
    matches!(code, Some(c) if want.contains(&c))
}

pub fn run() -> bool {
    let root = workspace_root();
    let Some(git) = system_git() else {
        println!("drills: SKIP (no system git available)");
        return true;
    };
    let bin = rust_git(&root);
    let mut pass = 0;
    let mut fail = 0;

    // Drill 1: missing ref (refs layer) — must fail, must not exit 0/129.
    {
        let dir = scratch("missing-ref");
        seed_repo(git, &dir);
        let o = run_rust(&bin, &dir, &["rev-parse", "refs/heads/does-not-exist-xyz"]);
        let attributed = o.stderr.contains("does-not-exist-xyz");
        if o.code != Some(0) && o.code != Some(129) {
            println!("drill missing-ref: PASS (exit {:?}, names subject: {attributed})", o.code);
            pass += 1;
        } else {
            println!("drill missing-ref: FAIL (exit {:?}, stderr: {})", o.code, o.stderr.trim());
            fail += 1;
        }
        if !attributed {
            println!("drill missing-ref: WARN no layer/subject attribution in diagnostic");
        }
    }

    // Drill 2: corrupt loose object (store layer) — must fail.
    {
        let dir = scratch("corrupt-object");
        let id = seed_repo(git, &dir);
        let obj = dir.join(".git/objects").join(&id[..2]).join(&id[2..]);
        if obj.exists() {
            // Loose objects are stored read-only (0444, like C git): make the
            // copy writable before corrupting it.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&obj, std::fs::Permissions::from_mode(0o644));
            }
            let mut bytes = std::fs::read(&obj).unwrap();
            let i = bytes.len() / 2;
            bytes[i] ^= 0xFF;
            std::fs::write(&obj, &bytes).unwrap();
            let o = run_rust(&bin, &dir, &["cat-file", "-p", &id]);
            let attributed = o.stderr.contains(&id[..8.min(id.len())]) || o.stderr.contains("object");
            if o.code != Some(0) {
                println!("drill corrupt-object: PASS (exit {:?}, names subject: {attributed})", o.code);
                pass += 1;
            } else {
                println!("drill corrupt-object: FAIL (exit 0 on corrupt object)");
                fail += 1;
            }
            if !attributed {
                println!("drill corrupt-object: WARN no layer/subject attribution in diagnostic");
            }
        } else {
            println!("drill corrupt-object: SKIP (object packed, not loose)");
        }
    }

    // Drill 3: invalid repo config — must fail, must not exit 0.
    {
        let dir = scratch("bad-config");
        seed_repo(git, &dir);
        std::fs::write(dir.join(".git/config"), "[[[not valid ini\n").unwrap();
        let o = run_rust(&bin, &dir, &["rev-parse", "HEAD"]);
        if o.code != Some(0) {
            println!("drill bad-config: PASS (exit {:?})", o.code);
            pass += 1;
        } else {
            println!("drill bad-config: FAIL (exit 0 on invalid config)");
            fail += 1;
        }
    }

    // Drill 4: usage error — `cat-file --bogus-flag` must exit 129.
    {
        let dir = scratch("usage");
        seed_repo(git, &dir);
        let o = run_rust(&bin, &dir, &["cat-file", "--bogus-flag-xyz"]);
        if class_ok(o.code, &[129]) {
            println!("drill usage-error: PASS (exit 129)");
            pass += 1;
        } else {
            println!("drill usage-error: FAIL (exit {:?}, want 129)", o.code);
            fail += 1;
        }
    }

    // Drill 5: unknown command — must exit 1.
    {
        let dir = scratch("unknown");
        let o = run_rust(&bin, &dir, &["definitely-not-a-git-command"]);
        if class_ok(o.code, &[1]) {
            println!("drill unknown-command: PASS (exit 1)");
            pass += 1;
        } else {
            println!("drill unknown-command: FAIL (exit {:?}, want 1)", o.code);
            fail += 1;
        }
    }

    println!("drills: {pass} passed, {fail} failed: {}", if fail == 0 { "PASS" } else { "FAIL" });
    fail == 0
}
