//! Secret lookup, caching, prompting, and redaction (FR-019), mirroring C
//! git's credential-helper scope and observable behavior — nothing invented.
//!
//! The boundary contract: C git's `credential fill/approve/reject` attribute
//! protocol (newline-separated `key=value` lines, blank line terminates) is
//! implemented here; helper selection comes from config values supplied by
//! the caller. Secrets never flow through config values, logs, or error
//! messages beyond what C git itself exposes.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

/// Errors from credential handling. Never carry secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialsError {
    /// No helper configured or no secret available.
    NotFound,
    /// Helper communication failure (command name only, no secrets).
    HelperFailed(String),
    /// Malformed helper output.
    Malformed(String),
}

impl fmt::Display for CredentialsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialsError::NotFound => write!(f, "no credential available"),
            CredentialsError::HelperFailed(h) => write!(f, "credential helper failed: {h}"),
            CredentialsError::Malformed(e) => write!(f, "malformed credential data: {e}"),
        }
    }
}

impl Error for CredentialsError {}

/// A credential: the attribute set C git exchanges with helpers
/// (`protocol`, `host`, `path`, `username`, `password`, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credential {
    attrs: Vec<(String, String)>,
}

impl Credential {
    pub fn new() -> Credential {
        Credential::default()
    }

    /// Set one attribute (e.g. `protocol` → `https`). Values are secret-opaque.
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(slot) = self.attrs.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_string();
        } else {
            self.attrs.push((key.to_string(), value.to_string()));
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Encode the `fill` request / response body: `key=value\n` lines plus a
    /// terminating blank line, exactly like C git's credential protocol.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.attrs {
            out.push_str(k);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
        out.push('\n');
        out
    }

    /// Decode a helper response body. Stops at the first blank line; lines
    /// without `=` are rejected (never silently kept).
    pub fn decode(body: &str) -> Result<Credential, CredentialsError> {
        let mut cred = Credential::new();
        for line in body.lines() {
            if line.is_empty() {
                break;
            }
            match line.split_once('=') {
                Some((k, v)) if !k.is_empty() => cred.set(k, v),
                _ => return Err(CredentialsError::Malformed(format!("bad credential line: {line}"))),
            }
        }
        Ok(cred)
    }

    /// Redacted view for logs and diagnostics: usernames kept, secret values
    /// replaced. Use this — never `{:?}` on the raw credential — outside the
    /// helper exchange itself.
    pub fn redacted(&self) -> RedactedCredential {
        RedactedCredential {
            attrs: self
                .attrs
                .iter()
                .map(|(k, v)| {
                    let shown = if is_secret_key(k) { "<redacted>".to_string() } else { v.clone() };
                    (k.clone(), shown)
                })
                .collect(),
        }
    }
}

fn is_secret_key(key: &str) -> bool {
    matches!(key, "password" | "oauth_refresh_token" | "password_expiry_utc")
}

/// A log-safe credential view. `Display`/`Debug` never emit secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedCredential {
    attrs: Vec<(String, String)>,
}

impl fmt::Display for RedactedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (k, v)) in self.attrs.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{k}={v}")?;
        }
        Ok(())
    }
}

/// In-memory credential cache keyed by (protocol, host, path, username),
/// mirroring `credential-cache` semantics minus the daemon (the daemon lands
/// with the helper execution work).
#[derive(Debug, Default)]
pub struct CredentialCache {
    entries: HashMap<(String, String, String, String), Credential>,
}

impl CredentialCache {
    pub fn new() -> CredentialCache {
        CredentialCache::default()
    }

    fn key_of(cred: &Credential) -> (String, String, String, String) {
        (
            cred.get("protocol").unwrap_or("").to_string(),
            cred.get("host").unwrap_or("").to_string(),
            cred.get("path").unwrap_or("").to_string(),
            cred.get("username").unwrap_or("").to_string(),
        )
    }

    /// Store an approved credential.
    pub fn approve(&mut self, cred: Credential) {
        self.entries.insert(Self::key_of(&cred), cred);
    }

    /// Look up a credential for a fill request: an entry matches when every
    /// non-secret attribute present in the request equals the entry's value
    /// (requests routinely omit `username`/`password` — that is what they
    /// are asking for — so lookup scans rather than hashing a full key).
    pub fn lookup(&self, req: &Credential) -> Option<&Credential> {
        self.entries.values().find(|c| {
            req.attrs.iter().all(|(k, v)| {
                if is_secret_key(k) {
                    return true;
                }
                c.get(k) == Some(v.as_str())
            })
        })
    }

    /// Drop rejected credentials (same matching rule as [`lookup`]).
    pub fn reject(&mut self, req: &Credential) {
        self.entries.retain(|_, c| {
            !req.attrs.iter().all(|(k, v)| {
                if is_secret_key(k) {
                    return true;
                }
                c.get(k) == Some(v.as_str())
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Credential {
        let mut c = Credential::new();
        c.set("protocol", "https");
        c.set("host", "example.com");
        c.set("username", "alice");
        c.set("password", "s3cret!");
        c
    }

    #[test]
    fn protocol_round_trip() {
        let body = sample().encode();
        assert!(body.ends_with("\n\n"));
        let back = Credential::decode(&body).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn rejects_lines_without_equals() {
        assert!(matches!(Credential::decode("garbage\n"), Err(CredentialsError::Malformed(_))));
    }

    #[test]
    fn redaction_hides_secrets_everywhere() {
        let shown = format!("{} | {:?}", sample().redacted(), sample().redacted());
        assert!(!shown.contains("s3cret!"), "secret leaked: {shown}");
        assert!(shown.contains("username=alice"));
        // The raw credential's Debug is never used in this crate's outputs;
        // the redacted view is the only printable form offered alongside it.
        let cache_probe = "password=s3cret!";
        assert!(!format!("{}", sample().redacted()).contains(cache_probe));
    }

    #[test]
    fn cache_approve_lookup_reject() {
        let mut cache = CredentialCache::new();
        let mut req = Credential::new();
        req.set("protocol", "https");
        req.set("host", "example.com");
        assert_eq!(cache.lookup(&req), None);
        cache.approve(sample());
        let found = cache.lookup(&req).expect("approved credential found");
        assert_eq!(found.get("username"), Some("alice"));
        cache.reject(&req);
        assert_eq!(cache.lookup(&req), None);
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Decoding never panics on arbitrary input.
        #[test]
        fn decode_never_panics(s: String) {
            let _ = Credential::decode(&s);
        }

        /// Encoded credentials always decode back (no `=`/newline mangling
        /// for ordinary keys and values).
        #[test]
        fn encode_decode_round_trip(
            user in "[a-z]{1,12}",
            pass in "[A-Za-z0-9!#$%]{1,16}",
        ) {
            let mut c = Credential::new();
            c.set("protocol", "https");
            c.set("host", "example.com");
            c.set("username", &user);
            c.set("password", &pass);
            let back = Credential::decode(&c.encode()).unwrap();
            prop_assert_eq!(back, c);
        }
    }
}
