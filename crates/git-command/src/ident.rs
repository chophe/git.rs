//! Identity and timestamp resolution for commit authorship.
//!
//! A git-compatible subset of `ident.c`: resolve name/email from the
//! `GIT_AUTHOR_*`/`GIT_COMMITTER_*` environment variables or `user.name` /
//! `user.email` configuration, and the timestamp from `GIT_AUTHOR_DATE` /
//! `GIT_COMMITTER_DATE` (parsed via `git-date`) or the current time.

use crate::CommandError;
use git_core::Repository;
use git_date::{parse, Timestamp};

/// The current time in UTC.
pub fn now_utc() -> Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Timestamp::new(secs, 0)
}

/// Resolve an author or committer identity line like
/// `Name <email> 1582024274 +0000`.
pub fn user_ident(repo: &Repository, author: bool) -> Result<String, CommandError> {
    let prefix = if author { "AUTHOR" } else { "COMMITTER" };

    let name = std::env::var(format!("GIT_{prefix}_NAME"))
        .ok()
        .or_else(|| repo.get("user", "name").map(str::to_string))
        .filter(|s| !s.is_empty());
    let email = std::env::var(format!("GIT_{prefix}_EMAIL"))
        .ok()
        .or_else(|| repo.get("user", "email").map(str::to_string))
        .filter(|s| !s.is_empty());

    let name = name.ok_or_else(|| missing_ident_error())?;
    let email = email.ok_or_else(|| missing_ident_error())?;

    let now = now_utc();
    let ts = match std::env::var(format!("GIT_{prefix}_DATE")) {
        Ok(d) => parse(&d, now).map_err(|e| CommandError::fatal(format!("bad date: {e}")))?,
        Err(_) => now,
    };

    Ok(format!("{name} <{email}> {}", ts.format_raw()))
}

fn missing_ident_error() -> CommandError {
    CommandError::fatal(
        "Please tell me who you are.\n\n\
         Run\n\n  git config --global user.email \"you@example.com\"\n  \
         git config --global user.name \"Your Name\"\n\n\
         to set your account's default identity.\n\
         Set GIT_AUTHOR_NAME/GIT_AUTHOR_EMAIL or GIT_COMMITTER_NAME/\
         GIT_COMMITTER_EMAIL to override.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn repo_with_user(name: &str, email: &str) -> git_core::Repository {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-ident-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::create_dir_all(git.join("refs")).unwrap();
        std::fs::write(
            git.join("config"),
            format!("[user]\n\tname = {name}\n\temail = {email}\n"),
        )
        .unwrap();
        let repo = git_core::Repository::discover_from(&dir, &git_core::RepoEnv::default()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        repo
    }

    fn env_path() -> PathBuf {
        PathBuf::from("")
    }

    #[test]
    fn resolves_from_config() {
        let repo = repo_with_user("Alice", "alice@example.com");
        let ident = user_ident(&repo, true).unwrap();
        assert!(ident.starts_with("Alice <alice@example.com> "));
        assert!(ident.ends_with(" +0000"));
    }

    #[test]
    fn env_overrides_config() {
        let repo = repo_with_user("Alice", "alice@example.com");
        crate::tests::serialized(|| {
            std::env::set_var("GIT_AUTHOR_NAME", "Bob");
            std::env::set_var("GIT_AUTHOR_EMAIL", "bob@example.com");
            let ident = user_ident(&repo, true).unwrap();
            assert!(ident.starts_with("Bob <bob@example.com> "));
            std::env::remove_var("GIT_AUTHOR_NAME");
            std::env::remove_var("GIT_AUTHOR_EMAIL");
        });
    }

    #[test]
    fn parses_author_date() {
        let repo = repo_with_user("Alice", "alice@example.com");
        crate::tests::serialized(|| {
            std::env::set_var("GIT_AUTHOR_DATE", "2020-02-18 11:11:14 +0000");
            let ident = user_ident(&repo, true).unwrap();
            assert!(ident.ends_with("1582024274 +0000"), "got: {ident}");
            std::env::remove_var("GIT_AUTHOR_DATE");
        });
    }

    #[test]
    fn missing_identity_errors() {
        let repo = git_core::Repository {
            git_dir: env_path(),
            common_dir: env_path(),
            work_tree: None,
            bare: false,
            hash_algo: git_hash::HashAlgorithm::Sha1,
            config: git_config::ConfigSet::new(),
            index_file: None,
            object_dir: None,
            alternates: Vec::new(),
        };
        let err = user_ident(&repo, false).unwrap_err();
        assert_eq!(err.code, 128);
        assert!(err.message.contains("Please tell me who you are"));
    }
}
