//! Crosswise tests for Phase A item A1 (collision-detecting SHA-1) against
//! the system C git. Skips when no system `git` is available.

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

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join(".git").exists() && dir.join("t/t0013").exists() {
            return dir;
        }
        dir = dir.parent().map(|p| p.to_path_buf()).unwrap_or(dir.clone());
    }
}

fn tempdir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-a1-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn real(dir: &Path, args: &[&str]) -> (String, i32) {
    let out = Command::new(git().unwrap())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(128))
}

fn ours(dir: &Path, args: &[&str]) -> (String, i32) {
    let (ctx, rest) = RepoContext::from_global_args(
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .unwrap();
    let cmd = rest[0].clone();
    let sub: Vec<String> = rest[1..].to_vec();
    let mut out: Vec<u8> = Vec::new();
    let code = match git_command::dispatch_with(&ctx, &cmd, &sub, &mut out) {
        Some(Ok(())) => 0,
        Some(Err(e)) => {
            if !e.message.is_empty() {
                out.extend_from_slice(e.message.as_bytes());
                out.push(b'\n');
            }
            e.code
        }
        None => 1,
    };
    (String::from_utf8_lossy(&out).into_owned(), code)
}

use git_command::RepoContext;

#[test]
fn hash_object_shattered_pdf_matches() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let pdf = repo_root().join("t/t0013/shattered-1.pdf").to_string_lossy().into_owned();
    let (rtext, rcode) = real(&dir, &["hash-object", &pdf]);
    let (otext, ocode) = ours(&dir, &["hash-object", &pdf]);
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}");
}

#[test]
fn hash_object_sizes_match() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    for size in [0usize, 1, 55, 56, 63, 64, 65, 127, 128, 1000] {
        let path = dir.join(format!("f{size}"));
        std::fs::write(&path, vec![b'x'; size]).unwrap();
        let ps = path.to_string_lossy().into_owned();
        let (rtext, rcode) = real(&dir, &["hash-object", &ps]);
        let (otext, ocode) = ours(&dir, &["hash-object", &ps]);
        assert_eq!(rcode, ocode, "size {size}");
        assert_eq!(rtext, otext, "size {size}\nreal: {rtext}\nours: {otext}");
    }
}

#[test]
fn hash_object_write_matches() {
    if git().is_none() {
        return;
    }
    let base = repo_root().join("t/t0013/shattered-1.pdf").to_string_lossy().into_owned();
    let dir = tempdir();
    let st = Command::new(git().unwrap())
        .args(["init", "-q", "-b", "main"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(st.success());
    let (rtext, rcode) = real(&dir, &["hash-object", "-w", &base]);
    let (otext, ocode) = ours(&dir, &["hash-object", "-w", &base]);
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext);
}
