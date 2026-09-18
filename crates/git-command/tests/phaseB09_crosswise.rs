use git_command::{diff, patch};
use git_command::{Command, CommandError, RepoContext};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};
use std::sync::atomic::{AtomicU32, Ordering};

#[path = "../src/show.rs"]
mod show;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn oracle(dir: &Path, args: &[&str]) -> Output {
    Process::new("/usr/bin/git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Show Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "Show Committer")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .env("GIT_AUTHOR_DATE", "1234567890 +0000")
        .env("GIT_COMMITTER_DATE", "1234567890 +0000")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap()
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let result = oracle(dir, args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}

fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "git-b09-show-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    ok(&dir, &["init", "-q", "-b", "main"]);
    ok(&dir, &["config", "color.ui", "false"]);
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/nested"), "nested\n").unwrap();
    std::fs::write(dir.join("file"), "first\n").unwrap();
    std::fs::write(dir.join("binary"), b"a\0b\xff").unwrap();
    ok(&dir, &["add", "."]);
    ok(&dir, &["commit", "-qm", "root subject\n\nroot body"]);
    std::fs::write(dir.join("file"), "first\nsecond\n").unwrap();
    ok(&dir, &["add", "."]);
    ok(&dir, &["commit", "-qm", "second subject\n\nsecond body"]);
    ok(&dir, &["tag", "-a", "release", "-m", "release message"]);
    Fixture(dir)
}

fn rust(dir: &Path, args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let args = args.iter().map(|s| (*s).into()).collect::<Vec<_>>();
    let mut out = Vec::new();
    match show::Show.run(&RepoContext::at(dir), &args, &mut out) {
        Ok(()) => (0, out, Vec::new()),
        Err(e) => {
            let err = if e.message.is_empty() {
                Vec::new()
            } else {
                format!("{}\n", e.message).into_bytes()
            };
            (e.code, out, err)
        }
    }
}

fn compare(dir: &Path, args: &[&str]) {
    let mut c_args = vec!["show"];
    c_args.extend_from_slice(args);
    let c = oracle(dir, &c_args);
    let (code, stdout, stderr) = rust(dir, args);
    assert_eq!(
        code,
        c.status.code().unwrap(),
        "status {args:?}: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&stderr),
        String::from_utf8_lossy(&c.stderr),
        "stderr {args:?}"
    );
    assert!(
        stdout == c.stdout,
        "stdout {args:?}\nRust: {:?}\nC: {:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&c.stdout)
    );
}

#[test]
fn default_head_matches_c() {
    let repo = fixture();
    compare(&repo.0, &[]);
}

#[test]
fn commits_formats_and_summaries_match_c() {
    let repo = fixture();
    for rev in ["HEAD", "HEAD~1"] {
        for options in [
            vec![],
            vec!["-s"],
            vec!["--oneline"],
            vec!["--stat"],
            vec!["--shortstat"],
            vec!["--numstat"],
            vec!["--name-only"],
            vec!["--name-status"],
            vec!["--patch-with-stat"],
            vec!["-U0"],
            vec!["--format="],
            vec!["--format=%h %s"],
            vec!["--pretty=format:%H", "-s"],
            vec!["--pretty=tformat:%s"],
            vec!["--pretty=short", "-s"],
            vec!["--pretty=fuller", "-s"],
            vec!["--pretty=raw", "-s"],
            vec!["--date=iso-strict", "-s"],
        ] {
            let mut args = options;
            args.push(rev);
            compare(&repo.0, &args);
        }
    }
    for options in [
        vec![],
        vec!["-s"],
        vec!["--oneline"],
        vec!["--format=%s", "-s"],
        vec!["--pretty=format:%s", "-s"],
    ] {
        let mut args = options;
        args.extend(["HEAD~1", "HEAD"]);
        compare(&repo.0, &args);
    }
    compare(&repo.0, &["HEAD", "HEAD"]);
    compare(&repo.0, &["HEAD", "--", "file"]);
    compare(&repo.0, &["HEAD", "--", "missing"]);
    compare(&repo.0, &["-s", "HEAD", "--", "missing"]);
    compare(&repo.0, &["HEAD", "--", "."]);
    compare(&repo.0, &["HEAD", "--", "./file"]);
    compare(&repo.0, &["--oneline", "--patch-with-stat"]);
    compare(&repo.0, &["--pretty=format:%s", "--patch-with-stat"]);
}

#[test]
fn blobs_trees_tags_and_packed_objects_match_c() {
    let repo = fixture();
    let blob = ok(&repo.0, &["rev-parse", "HEAD:binary"]);
    let tree = ok(&repo.0, &["rev-parse", "HEAD^{tree}"]);
    ok(
        &repo.0,
        &["tag", "-a", "blob-tag", &blob, "-m", "binary tag"],
    );
    ok(&repo.0, &["tag", "-a", "tree-tag", &tree, "-m", "tree tag"]);
    ok(
        &repo.0,
        &["tag", "-a", "outer", "release", "-m", "outer message"],
    );
    ok(&repo.0, &["tag", "lightweight"]);
    for packed in [false, true] {
        if packed {
            ok(&repo.0, &["gc", "--quiet"]);
        }
        for rev in [
            blob.as_str(),
            tree.as_str(),
            "HEAD:file",
            "HEAD:binary",
            "HEAD:sub",
            "HEAD:",
            "HEAD^{tree}",
            "release",
            "release^{}",
            "release^0",
            "release:file",
            "lightweight",
            "blob-tag",
            "tree-tag",
            "outer",
        ] {
            compare(&repo.0, &[rev]);
            compare(&repo.0, &["--no-patch", rev]);
        }
        for format in [
            "--oneline",
            "--pretty=short",
            "--pretty=full",
            "--pretty=fuller",
            "--pretty=raw",
            "--format=%s",
        ] {
            compare(&repo.0, &[format, "--no-patch", "release"]);
        }
        compare(&repo.0, &["HEAD:file", "HEAD:binary"]);
        compare(&repo.0, &["HEAD:sub", "HEAD"]);
    }
}

#[test]
fn errors_match_c_and_unsupported_features_are_explicit() {
    let repo = fixture();
    for args in [
        vec!["missing"],
        vec!["missing", "--"],
        vec!["HEAD:missing"],
        vec!["--pretty=invalid"],
        vec!["ffffffffffffffffffffffffffffffffffffffff"],
    ] {
        compare(&repo.0, &args);
    }
    for args in [
        vec!["--walk"],
        vec!["HEAD~1..HEAD"],
        vec!["--textconv"],
        vec!["--color=always"],
        vec!["--", "*.rs"],
    ] {
        let (code, _, error) = rust(&repo.0, &args);
        assert_eq!(code, 129, "{args:?}");
        assert!(String::from_utf8_lossy(&error).contains("not supported"));
    }
}

#[test]
fn renames_empty_commits_and_first_parent_merge_match_c() {
    let repo = fixture();
    ok(&repo.0, &["mv", "file", "renamed"]);
    ok(&repo.0, &["commit", "-qm", "rename"]);
    for args in [
        vec![],
        vec!["--stat"],
        vec!["--name-status"],
        vec!["--no-renames"],
    ] {
        compare(&repo.0, &args);
    }
    ok(&repo.0, &["commit", "--allow-empty", "-qm", "empty"]);
    compare(&repo.0, &[]);
    compare(&repo.0, &["--stat"]);
    ok(&repo.0, &["checkout", "-qb", "side", "HEAD~1"]);
    std::fs::write(repo.0.join("side"), "side\n").unwrap();
    ok(&repo.0, &["add", "."]);
    ok(&repo.0, &["commit", "-qm", "side"]);
    ok(&repo.0, &["checkout", "-q", "main"]);
    ok(&repo.0, &["merge", "--no-ff", "-qm", "merge", "side"]);
    compare(&repo.0, &["--no-patch"]);
    compare(&repo.0, &["--first-parent"]);
    assert_eq!(rust(&repo.0, &[]).0, 129);
}
