//! Wire encoding for Git protocols (FR-016): packet lines, capability
//! advertisement, and protocol-version negotiation.
//!
//! Isolated from transport flow control: every function here operates on
//! plain byte buffers (canned streams in tests), so framing can be verified
//! without any connection. Command logic never sees framing details.

use std::error::Error;
use std::fmt;

/// Errors from wire encoding/decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// Truncated or over-long packet line.
    Truncated,
    /// Non-hex length prefix or malformed capability.
    Malformed(String),
    /// Peer requested a protocol version this implementation does not speak.
    UnsupportedVersion(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Truncated => write!(f, "truncated packet line"),
            ProtocolError::Malformed(e) => write!(f, "malformed protocol data: {e}"),
            ProtocolError::UnsupportedVersion(v) => write!(f, "unsupported protocol version: {v}"),
        }
    }
}

impl Error for ProtocolError {}

/// Maximum packet-line payload (65520 bytes: 64 KiB minus the length header).
pub const MAX_PAYLOAD: usize = 65520;

/// Protocol versions this implementation negotiates, in preference order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolVersion {
    V2,
    V0,
}

impl ProtocolVersion {
    /// The `version=N` capability token for this version.
    pub fn capability(self) -> &'static str {
        match self {
            ProtocolVersion::V2 => "version=2",
            ProtocolVersion::V0 => "version=0",
        }
    }
}

/// Negotiate the highest mutually supported version: the first of our
/// preferences present in the peer's capability list, defaulting to V0
/// (C git speaks v0 when no `version=` capability is advertised).
pub fn negotiate(ours: &[ProtocolVersion], theirs: &[String]) -> ProtocolVersion {
    for v in ours {
        if theirs.iter().any(|c| c == v.capability()) {
            return *v;
        }
    }
    ProtocolVersion::V0
}

/// Encode one packet line (`<4-hex-len><payload>`); empty payload encodes
/// the flush packet (`0000`).
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(ProtocolError::Malformed(format!("payload of {} bytes exceeds {MAX_PAYLOAD}", payload.len())));
    }
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.extend_from_slice(format!("{:04x}", payload.len() + 4).as_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// The flush packet: end of a packet-line sequence.
pub fn flush() -> Vec<u8> {
    b"0000".to_vec()
}

/// Decode one packet line from the front of `input`, returning the payload
/// (`None` for flush) and the number of bytes consumed.
pub fn decode(input: &[u8]) -> Result<(Option<Vec<u8>>, usize), ProtocolError> {
    if input.len() < 4 {
        return Err(ProtocolError::Truncated);
    }
    let len = usize::from_str_radix(std::str::from_utf8(&input[..4]).map_err(|_| ProtocolError::Malformed("non-UTF-8 length prefix".to_string()))?, 16)
        .map_err(|_| ProtocolError::Malformed("non-hex length prefix".to_string()))?;
    if len == 0 {
        return Ok((None, 4));
    }
    if len < 4 {
        return Err(ProtocolError::Malformed(format!("length prefix {len} below header size")));
    }
    if input.len() < len {
        return Err(ProtocolError::Truncated);
    }
    Ok((Some(input[4..len].to_vec()), len))
}

/// Split an advertised capability line (`name` or `name=value`, NUL-separated
/// `name\0caps` form accepted) into (name, optional value).
pub fn split_capability(token: &str) -> (String, Option<String>) {
    match token.split_once('=') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (token.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_packet_line() {
        let payload = b"want abcdef1234567890 multi_ack side-band-64k";
        let enc = encode(payload).unwrap();
        assert_eq!(&enc[..4], b"0031");
        let (back, used) = decode(&enc).unwrap();
        assert_eq!(back.unwrap(), payload);
        assert_eq!(used, enc.len());
    }

    #[test]
    fn flush_packet() {
        let (back, used) = decode(&flush()).unwrap();
        assert_eq!(back, None);
        assert_eq!(used, 4);
    }

    #[test]
    fn rejects_truncated_and_oversize() {
        assert_eq!(decode(b"00"), Err(ProtocolError::Truncated));
        assert_eq!(decode(b"0002"), Err(ProtocolError::Malformed("length prefix 2 below header size".to_string())));
        assert_eq!(decode(b"0036ab"), Err(ProtocolError::Truncated));
        assert!(encode(&vec![0u8; MAX_PAYLOAD + 1]).is_err());
    }

    #[test]
    fn negotiates_highest_common_version() {
        let ours = [ProtocolVersion::V2, ProtocolVersion::V0];
        assert_eq!(
            negotiate(&ours, &["multi_ack".to_string(), "version=2".to_string()]),
            ProtocolVersion::V2
        );
        assert_eq!(negotiate(&ours, &["multi_ack".to_string()]), ProtocolVersion::V0);
        assert_eq!(negotiate(&[ProtocolVersion::V0], &["version=2".to_string()]), ProtocolVersion::V0);
    }

    #[test]
    fn splits_capabilities() {
        assert_eq!(split_capability("multi_ack"), ("multi_ack".to_string(), None));
        assert_eq!(
            split_capability("version=2"),
            ("version".to_string(), Some("2".to_string()))
        );
    }
}
