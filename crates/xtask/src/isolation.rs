//! Isolation runner (SC-002): every component's unit tests must pass
//! stand-alone in a bare directory with a scrubbed environment.
//!
//! For each workspace member (except `xtask` itself), runs
//! `cargo test -p <crate> --lib --bins --doc --offline` with: cwd set to a
//! fresh bare temp dir, environment cleared except a minimal allowlist
//! (PATH/HOME for the toolchain, TMPDIR pointing at the bare dir, LANG=C),
//! and `CARGO_NET_OFFLINE=true`.
//!
//! Scope note: only unit targets (`--lib --bins --doc`) are exercised here.
//! Integration suites under `tests/` are crosswise end-to-end tests that by
//! contract shell out to both the Rust binary and the system C git; they can
//! never be hermetic and are covered by `cargo xtask differential` instead.
//! Reports per-crate PASS/FAIL. Only std is used.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

fn workspace_members(root: &PathBuf) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("read workspace Cargo.toml");
    let mut members = Vec::new();
    let mut in_members = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("members") {
            in_members = true;
            continue;
        }
        if in_members {
            if t.starts_with(']') {
                break;
            }
            let name = t.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ' ');
            if !name.is_empty() && !name.starts_with('#') && !name.starts_with('[') {
                members.push(name.to_string());
            }
        }
    }
    members
}

fn bare_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-isolation-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create bare dir");
    dir
}

fn scrubbed_env(cmd: &mut Command, bare: &std::path::Path) {
    cmd.env_clear();
    for key in ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("TMPDIR", bare);
    cmd.env("LANG", "C");
    cmd.env("CARGO_NET_OFFLINE", "true");
    // Deliberately NOT passed: GIT_* (must not leak repo layout),
    // GIT_DIR/WORK_TREE/CEILING_DIRECTORIES, config overrides.
}

pub fn run() -> bool {
    let root = workspace_root();
    let members: Vec<String> = workspace_members(&root)
        .into_iter()
        .filter(|m| m != "xtask")
        .collect();

    let mut ok = true;
    let manifest = root.join("Cargo.toml");
    let manifest = manifest.to_str().expect("utf8 manifest path").to_string();
    // `--doc` cannot be mixed with other target selectors: two runs per crate.
    let runs: &[&[&str]] = &[&["--lib", "--bins"], &["--doc"]];
    for m in &members {
        let bare = bare_dir(&m.replace('-', "_"));
        let mut crate_ok = true;
        for targets in runs {
            let mut cmd = Command::new("cargo");
            // `--manifest-path` lets cargo run with cwd in the bare dir: the
            // test processes themselves must not depend on repo layout.
            // Unit targets only (see scope note above).
            let mut args = vec!["test", "--manifest-path", manifest.as_str(), "-p", m.as_str()];
            args.extend_from_slice(targets);
            args.push("--offline");
            cmd.args(&args).current_dir(&bare);
            scrubbed_env(&mut cmd, &bare);
            match cmd.status() {
                Ok(s) if s.success() => {}
                Ok(s) => {
                    println!("isolation {m} ({}): FAIL (exit {s})", targets.join(" "));
                    crate_ok = false;
                }
                Err(e) => {
                    println!("isolation {m} ({}): FAIL (could not run: {e})", targets.join(" "));
                    crate_ok = false;
                }
            }
        }
        if crate_ok {
            println!("isolation {m}: PASS (bare dir, scrubbed env)");
        } else {
            println!("isolation {m}: FAIL — hidden dependence on repo layout or environment?");
            ok = false;
        }
        let _ = std::fs::remove_dir_all(&bare);
    }
    println!("isolation: {} crates: {}", members.len(), if ok { "PASS" } else { "FAIL" });
    ok
}
