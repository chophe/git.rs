//! Data movement for fetch/push-class commands (FR-015): connection
//! handling, pack negotiation, and progress — with C-git retry, timeout,
//! failure, and progress behavior as the contract (Clarifications 2026-09-20).
//!
//! Dependency direction: this component depends on store/language layers and
//! is depended on only by fetch/push-class commands. Store components never
//! depend on transport. Network I/O itself lands with fetch/push; this
//! scaffold owns the negotiation state machine and progress reporting so
//! both can be tested without a connection.

use std::error::Error;
use std::fmt;

use git_hash::Oid;

/// Errors from transport negotiation/progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Remote hung up or returned an unexpected message.
    UnexpectedEnd(String),
    /// Negotiation failed (no common base, unsupported capability).
    NegotiationFailed(String),
    /// The endpoint URL/selector is not usable.
    BadEndpoint(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::UnexpectedEnd(e) => write!(f, "remote hung up unexpectedly: {e}"),
            TransportError::NegotiationFailed(e) => write!(f, "negotiation failed: {e}"),
            TransportError::BadEndpoint(e) => write!(f, "bad transport endpoint: {e}"),
        }
    }
}

impl Error for TransportError {}

/// Where to fetch from / push to: a URL or a local path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Url(String),
    LocalPath(String),
}

impl Endpoint {
    /// Classify a remote selector the way C git does: anything containing
    /// `://`, `user@host:path`, or an existing local path is usable;
    /// empty selectors are rejected explicitly.
    pub fn parse(selector: &str) -> Result<Endpoint, TransportError> {
        if selector.is_empty() {
            return Err(TransportError::BadEndpoint("empty remote selector".to_string()));
        }
        if selector.contains("://") || looks_like_scp(selector) {
            return Ok(Endpoint::Url(selector.to_string()));
        }
        Ok(Endpoint::LocalPath(selector.to_string()))
    }

    /// True for local paths (no network behavior applies).
    pub fn is_local(&self) -> bool {
        matches!(self, Endpoint::LocalPath(_))
    }
}

fn looks_like_scp(s: &str) -> bool {
    match s.find(':') {
        Some(i) => {
            let host = &s[..i];
            !host.is_empty()
                && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
                && !s[i + 1..].starts_with('/')
        }
        None => false,
    }
}

/// One progress event, mirroring C git's `Counting objects`, `Compressing
/// objects`, `Receiving objects` reporting shape (percent + counts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
}

impl ProgressEvent {
    pub fn percent(&self) -> u64 {
        if self.total == 0 {
            return 100;
        }
        self.done.saturating_mul(100) / self.total.max(1)
    }
}

/// Sink collecting progress events (rendering stays at the CLI edge).
#[derive(Debug, Clone, Default)]
pub struct Progress {
    events: Vec<ProgressEvent>,
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    pub fn report(&mut self, phase: &'static str, done: u64, total: u64) {
        // Monotonic per phase: never report backwards.
        if let Some(last) = self.events.iter().rev().find(|e| e.phase == phase) {
            if done < last.done {
                return;
            }
        }
        self.events.push(ProgressEvent { phase, done, total });
    }

    pub fn events(&self) -> &[ProgressEvent] {
        &self.events
    }
}

/// Pack negotiation state (fetch side): which objects we want, which the
/// peer is known to have, and whether another round is needed. Mirrors C
/// git's multi-round `want`/`have` exchange without any I/O.
#[derive(Debug, Clone, Default)]
pub struct Negotiation {
    wants: Vec<Oid>,
    haves: Vec<Oid>,
    /// Peer `have`s acknowledged in the last round.
    acked: Vec<Oid>,
    done: bool,
}

impl Negotiation {
    pub fn new(wants: Vec<Oid>, haves: Vec<Oid>) -> Negotiation {
        Negotiation { wants, haves, acked: Vec::new(), done: false }
    }

    pub fn wants(&self) -> &[Oid] {
        &self.wants
    }

    pub fn haves(&self) -> &[Oid] {
        &self.haves
    }

    /// Record the peer's acknowledgements; returns true when negotiation is
    /// complete (an ack, or nothing left to offer — matching C git's
    /// "done" conditions).
    pub fn acknowledge(&mut self, acked: &[Oid], peer_done: bool) -> bool {
        self.acked.extend_from_slice(acked);
        if peer_done || !self.acked.is_empty() {
            self.done = true;
        } else if self.haves.is_empty() {
            self.done = true;
        }
        self.done
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;

    fn oid(byte: u8) -> Oid {
        Oid::new(HashAlgorithm::Sha1, &vec![byte; 20])
    }

    #[test]
    fn classifies_endpoints() {
        assert!(matches!(Endpoint::parse("https://example.com/r.git"), Ok(Endpoint::Url(_))));
        assert!(matches!(Endpoint::parse("git@example.com:r.git"), Ok(Endpoint::Url(_))));
        assert!(matches!(Endpoint::parse("/srv/repos/r.git"), Ok(Endpoint::LocalPath(_))));
        assert!(matches!(Endpoint::parse(""), Err(TransportError::BadEndpoint(_))));
    }

    #[test]
    fn progress_is_monotonic_per_phase() {
        let mut p = Progress::new();
        p.report("Receiving objects", 10, 100);
        p.report("Receiving objects", 5, 100); // backwards: dropped
        p.report("Receiving objects", 50, 100);
        assert_eq!(p.events().len(), 2);
        assert_eq!(p.events()[1].percent(), 50);
    }

    #[test]
    fn negotiation_completes_on_ack_or_exhaustion() {
        let mut n = Negotiation::new(vec![oid(1)], vec![oid(2)]);
        assert!(!n.is_done());
        assert!(!n.acknowledge(&[], false));
        assert!(n.acknowledge(&[oid(2)], false));
        assert!(n.is_done());

        let mut n = Negotiation::new(vec![oid(1)], vec![]);
        assert!(n.acknowledge(&[], false));
    }
}
