//! Single zlib/deflate boundary for the workspace (FR-007).
//!
//! All compression use goes through this crate; no other component may
//! depend on `flate2` (or any other deflate provider) directly. The provider
//! is an implementation detail of this facade.
//!
//! Memory contract (FR-026): one-shot `*_all`/`decode_bounded` helpers are
//! **buffering** (they materialize the full output); bulk paths MUST use the
//! streaming [`Encoder`]/[`Decoder`] or the cap-enforcing [`Inflater`].

use std::error::Error;
use std::fmt;
use std::io::{Read, Write};

/// Errors from the compression boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressError {
    /// Underlying I/O failure while streaming.
    Io(String),
    /// Input is not valid zlib-framed deflate data.
    Corrupt(String),
    /// Output exceeded the caller's size cap.
    TooLarge { limit: u64 },
}

impl fmt::Display for CompressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompressError::Io(e) => write!(f, "compression I/O error: {e}"),
            CompressError::Corrupt(e) => write!(f, "corrupt deflate data: {e}"),
            CompressError::TooLarge { limit } => write!(f, "deflate output exceeds {limit} bytes"),
        }
    }
}

impl Error for CompressError {}

/// Deflate with zlib framing at the default level (matches loose objects).
/// Buffering: materializes the full output.
pub fn encode_all(data: &[u8]) -> Vec<u8> {
    encode_all_level(data, 6)
}

/// Deflate with zlib framing at `level` 0–9 (clamped). Buffering.
pub fn encode_all_level(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(
        Vec::new(),
        flate2::Compression::new(level.min(9)),
    );
    encoder.write_all(data).expect("deflate to memory cannot fail");
    encoder.finish().expect("deflate to memory cannot fail")
}

/// Inflate zlib-framed data. Buffering: materializes the full output with no
/// cap — only for inputs whose size is already bounded by the caller.
/// Prefer [`decode_bounded`] or streaming [`Decoder`].
pub fn decode_all(data: &[u8]) -> Result<Vec<u8>, CompressError> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| CompressError::Corrupt(e.to_string()))?;
    Ok(out)
}

/// Inflate zlib-framed data, failing with [`CompressError::TooLarge`] when
/// the output would exceed `limit` bytes. Buffering up to the cap.
pub fn decode_bounded(data: &[u8], limit: u64) -> Result<Vec<u8>, CompressError> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    let mut total = 0u64;
    let mut chunk = [0u8; 8192];
    loop {
        let n = decoder.read(&mut chunk).map_err(|e| CompressError::Corrupt(e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(CompressError::TooLarge { limit });
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

/// Streaming zlib encoder: wraps a writer, compressing on the fly with
/// bounded memory. Finish via [`Encoder::finish`] to flush the trailer.
pub struct Encoder<W: Write> {
    inner: flate2::write::ZlibEncoder<W>,
}

impl<W: Write> Encoder<W> {
    /// Begin streaming deflate at `level` 0–9 (clamped) with zlib framing.
    pub fn new(writer: W, level: u32) -> Encoder<W> {
        Encoder {
            inner: flate2::write::ZlibEncoder::new(writer, flate2::Compression::new(level.min(9))),
        }
    }

    /// Flush the stream trailer and return the underlying writer.
    pub fn finish(self) -> std::io::Result<W> {
        self.inner.finish()
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Streaming zlib decoder: wraps a reader, inflating on the fly with bounded
/// memory. I/O errors surface as-is; framing errors surface on read.
pub struct Decoder<R: Read> {
    inner: flate2::read::ZlibDecoder<R>,
}

impl<R: Read> Decoder<R> {
    /// Begin streaming inflate of zlib-framed data.
    pub fn new(reader: R) -> Decoder<R> {
        Decoder { inner: flate2::read::ZlibDecoder::new(reader) }
    }
}

impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

/// Whether more input is needed, more output space, or the stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateStatus {
    /// Progress was made; call again with remaining input / fresh output space.
    Ok,
    /// The stream ended cleanly.
    StreamEnd,
    /// No progress possible with the given buffers (truncated input or full
    /// output — the caller decides, matching pack delta application).
    BufError,
}

/// How to flush the inflate stream (pack paths use [`InflateFlush::None`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateFlush {
    None,
    Sync,
    Finish,
}

/// Chunked low-level inflate with an enforced output cap, for pack delta
/// application paths that stream base objects without materializing them.
/// Wraps the provider's raw decompressor so callers never touch it directly.
pub struct Inflater {
    inner: flate2::Decompress,
    produced: u64,
    cap: u64,
}

impl Inflater {
    /// New zlib-framed inflate stream; at most `cap` total output bytes
    /// (exceeding it reports [`CompressError::TooLarge`]).
    pub fn new_zlib(cap: u64) -> Inflater {
        Inflater { inner: flate2::Decompress::new(true), produced: 0, cap }
    }

    /// Total compressed bytes consumed so far.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    /// Total bytes produced so far.
    pub fn total_out(&self) -> u64 {
        self.produced
    }

    /// Inflate `input` into `output`, returning bytes consumed, bytes
    /// produced, and the stream status.
    pub fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: InflateFlush,
    ) -> Result<(usize, usize, InflateStatus), CompressError> {
        let before_in = self.inner.total_in();
        let before_out = self.inner.total_out();
        let fl = match flush {
            InflateFlush::None => flate2::FlushDecompress::None,
            InflateFlush::Sync => flate2::FlushDecompress::Sync,
            InflateFlush::Finish => flate2::FlushDecompress::Finish,
        };
        let status = self
            .inner
            .decompress(input, output, fl)
            .map_err(|e| CompressError::Corrupt(e.to_string()))?;
        let consumed = (self.inner.total_in() - before_in) as usize;
        let produced = (self.inner.total_out() - before_out) as usize;
        self.produced += produced as u64;
        if self.produced > self.cap {
            return Err(CompressError::TooLarge { limit: self.cap });
        }
        Ok((
            consumed,
            produced,
            match status {
                flate2::Status::Ok => InflateStatus::Ok,
                flate2::Status::StreamEnd => InflateStatus::StreamEnd,
                flate2::Status::BufError => InflateStatus::BufError,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_round_trip() {
        let data = b"hello world, hello world, hello world";
        let enc = encode_all(data);
        assert_eq!(decode_all(&enc).unwrap(), data);
    }

    #[test]
    fn levels_round_trip() {
        for level in [0, 1, 6, 9, 99] {
            let enc = encode_all_level(b"level test payload", level);
            assert_eq!(decode_bounded(&enc, 1 << 20).unwrap(), b"level test payload");
        }
    }

    #[test]
    fn bounded_rejects_oversize_output() {
        let enc = encode_all(&vec![7u8; 100_000]);
        let err = decode_bounded(&enc, 10).unwrap_err();
        assert_eq!(err, CompressError::TooLarge { limit: 10 });
    }

    #[test]
    fn corrupt_input_errors() {
        assert!(matches!(decode_all(b"not deflate at all!!"), Err(CompressError::Corrupt(_))));
    }

    #[test]
    fn streaming_round_trip() {
        let data = vec![42u8; 50_000];
        let mut enc = Encoder::new(Vec::new(), 6);
        enc.write_all(&data).unwrap();
        let compressed = enc.finish().unwrap();
        let mut dec = Decoder::new(&compressed[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn inflater_streams_with_cap() {
        let data = b"inflater chunk test ".repeat(100);
        let enc = encode_all(&data);
        let mut infl = Inflater::new_zlib(1 << 20);
        let mut out = vec![0u8; 64];
        let mut pos = 0;
        let mut got = Vec::new();
        loop {
            let (used, made, st) = infl.decompress(&enc[pos..], &mut out, InflateFlush::None).unwrap();
            pos += used;
            got.extend_from_slice(&out[..made]);
            match st {
                InflateStatus::StreamEnd => break,
                InflateStatus::BufError if pos >= enc.len() => break,
                _ => {}
            }
        }
        assert_eq!(got, data);
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// One-shot encode/decode round-trips arbitrary bytes at every level.
        #[test]
        fn round_trip(data: Vec<u8>, level in 0u32..12u32) {
            let enc = encode_all_level(&data, level);
            let back = decode_bounded(&enc, (data.len() as u64).saturating_add(16)).unwrap();
            prop_assert_eq!(back, data);
        }

        /// Decoding arbitrary bytes never panics.
        #[test]
        fn decode_never_panics(data: Vec<u8>) {
            let _ = decode_all(&data);
            let _ = decode_bounded(&data, 1024);
        }
    }
}
