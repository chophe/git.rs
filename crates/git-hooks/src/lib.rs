//! Hook discovery, execution environment, and skip/force policy (FR-018).
//!
//! Command flows call explicit hook points; behavior with hooks absent must
//! be identical modulo the hook's own effects. Hook lookup reads repository
//! paths and config as values supplied by the caller — never globals.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Errors from hook discovery/execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HooksError {
    /// Filesystem I/O failure.
    Io(String),
    /// The hook exited nonzero (its stderr is attached when captured).
    Failed { point: String, code: i32, stderr: String },
    /// The hook path is not executable and cannot run.
    NotExecutable(String),
}

impl fmt::Display for HooksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HooksError::Io(e) => write!(f, "hook I/O error: {e}"),
            HooksError::Failed { point, code, stderr } => {
                write!(f, "{point} hook failed with exit {code}")?;
                if !stderr.is_empty() {
                    write!(f, ": {stderr}")?;
                }
                Ok(())
            }
            HooksError::NotExecutable(p) => write!(f, "hook not executable: {p}"),
        }
    }
}

impl Error for HooksError {}

/// Named hook points (C git's `$GIT_DIR/hooks/<name>` set, extensible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
    PreCommit,
    PrepareCommitMsg,
    CommitMsg,
    PostCommit,
    PrePush,
    PreReceive,
    Update,
    PostReceive,
    PostCheckout,
    PostMerge,
}

impl HookPoint {
    /// The file name under `$GIT_DIR/hooks/`.
    pub fn file_name(self) -> &'static str {
        match self {
            HookPoint::PreCommit => "pre-commit",
            HookPoint::PrepareCommitMsg => "prepare-commit-msg",
            HookPoint::CommitMsg => "commit-msg",
            HookPoint::PostCommit => "post-commit",
            HookPoint::PrePush => "pre-push",
            HookPoint::PreReceive => "pre-receive",
            HookPoint::Update => "update",
            HookPoint::PostReceive => "post-receive",
            HookPoint::PostCheckout => "post-checkout",
            HookPoint::PostMerge => "post-merge",
        }
    }
}

/// Skip/force policy for one hook invocation (e.g. `--no-verify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPolicy {
    /// Run the hook when present.
    Run,
    /// Skip even when present (`--no-verify`).
    Skip,
}

/// Locate an enabled hook: `$GIT_DIR/hooks/<name>` must exist and be
/// executable. Returns `None` when hooks are absent (the common case —
/// callers then behave identically).
pub fn find_hook(git_dir: &Path, point: HookPoint) -> Option<PathBuf> {
    let path = git_dir.join("hooks").join(point.file_name());
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}

/// Environment for hook execution: the repository context plus caller extras
/// (C git sets `GIT_DIR`, `GIT_INDEX_FILE`, etc. — assembled here, never
/// read from the process by libraries).
#[derive(Debug, Clone, Default)]
pub struct HookEnv {
    vars: Vec<(String, String)>,
    args: Vec<String>,
}

impl HookEnv {
    pub fn new() -> HookEnv {
        HookEnv::default()
    }

    pub fn var(mut self, key: &str, value: &str) -> HookEnv {
        self.vars.push((key.to_string(), value.to_string()));
        self
    }

    pub fn arg(mut self, value: &str) -> HookEnv {
        self.args.push(value.to_string());
        self
    }
}

/// Run one hook point under `policy` with `stdin_data` on stdin. Returns
/// `Ok(true)` when a hook ran and passed, `Ok(false)` when skipped or
/// absent, `Err` on failure.
pub fn run_hook(
    git_dir: &Path,
    point: HookPoint,
    policy: HookPolicy,
    env: &HookEnv,
    stdin_data: &[u8],
) -> Result<bool, HooksError> {
    if policy == HookPolicy::Skip {
        return Ok(false);
    }
    let Some(path) = find_hook(git_dir, point) else {
        return Ok(false);
    };
    let mut child = Command::new(&path)
        .args(&env.args)
        .envs(env.vars.iter().cloned())
        .env("GIT_DIR", git_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| HooksError::Io(e.to_string()))?;
    use std::io::Write as _;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_data);
    }
    let out = child.wait_with_output().map_err(|e| HooksError::Io(e.to_string()))?;
    if out.status.success() {
        return Ok(true);
    }
    Err(HooksError::Failed {
        point: point.file_name().to_string(),
        code: out.status.code().unwrap_or(128),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "git-hooks-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("hooks")).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn absent_hooks_run_nothing() {
        let gd = git_dir();
        assert_eq!(find_hook(&gd, HookPoint::PreCommit), None);
        let ran = run_hook(&gd, HookPoint::PreCommit, HookPolicy::Run, &HookEnv::new(), b"").unwrap();
        assert!(!ran);
        std::fs::remove_dir_all(&gd).ok();
    }

    #[test]
    fn skip_policy_beats_present_hooks() {
        let gd = git_dir();
        let hook = gd.join("hooks").join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let ran = run_hook(&gd, HookPoint::PreCommit, HookPolicy::Skip, &HookEnv::new(), b"").unwrap();
        assert!(!ran);
        std::fs::remove_dir_all(&gd).ok();
    }

    #[test]
    fn failing_hook_reports_point_and_code() {
        let gd = git_dir();
        let hook = gd.join("hooks").join("pre-push");
        std::fs::write(&hook, "#!/bin/sh\necho blocked >&2\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(unix)]
        {
            let err = run_hook(&gd, HookPoint::PrePush, HookPolicy::Run, &HookEnv::new(), b"").unwrap_err();
            assert!(matches!(err, HooksError::Failed { code: 3, .. }));
            assert!(format!("{err}").contains("pre-push"));
        }
        std::fs::remove_dir_all(&gd).ok();
    }

    #[test]
    fn hook_points_map_to_c_names() {
        assert_eq!(HookPoint::PrepareCommitMsg.file_name(), "prepare-commit-msg");
        assert_eq!(HookPoint::PostCheckout.file_name(), "post-checkout");
    }
}
