//! Local timezone resolution.
//!
//! A minimal TZif (RFC 8536) reader over the system timezone database
//! (`/usr/share/zoneinfo`, `/etc/localtime`), matching C git's
//! `localtime_r` semantics for offset lookup at a given instant. Only what
//! `date.c` needs is implemented: the UTC offset for a timestamp, honoring
//! DST via transition tables.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One transition: from `time` onward, use `offset_min`.
#[derive(Debug, Clone)]
struct Tz {
    /// (transition start, offset in minutes east of UTC), ascending.
    transitions: Vec<(i64, i32)>,
    /// Offset used before the first transition.
    default_offset: i32,
}

/// Cache keyed on the resolved timezone file path so `TZ` changes (as in
/// tests or crosswise runs) reload correctly while a single `TZ` stays cheap.
fn tz_cache() -> &'static Mutex<Option<(Option<PathBuf>, Option<Tz>)>> {
    static TZ_CACHE: OnceLock<Mutex<Option<(Option<PathBuf>, Option<Tz>)>>> = OnceLock::new();
    TZ_CACHE.get_or_init(|| Mutex::new(None))
}

fn tz_file_path() -> Option<std::path::PathBuf> {
    match std::env::var("TZ") {
        Ok(tz) if tz.is_empty() => Some(std::path::PathBuf::from("/etc/localtime")),
        Ok(tz) => {
            let name = tz.trim_start_matches(':');
            if name.is_empty() {
                return Some(std::path::PathBuf::from("/etc/localtime"));
            }
            // POSIX TZ specs like "UTC+3" or "EST5EDT" without a slash are
            // approximated as UTC unless a zoneinfo file exists.
            let p = std::path::PathBuf::from("/usr/share/zoneinfo").join(name);
            if p.exists() {
                Some(p)
            } else if name == "UTC" || name == "GMT" {
                None
            } else {
                None
            }
        }
        Err(_) => Some(std::path::PathBuf::from("/etc/localtime")),
    }
}

fn load_tz() -> Option<Tz> {
    let path = tz_file_path();
    let mut guard = tz_cache().lock().unwrap();
    match &*guard {
        Some((p, tz)) if *p == path => tz.clone(),
        _ => {
            let tz = path
                .as_ref()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| parse_tzif(&d));
            *guard = Some((path, tz.clone()));
            tz
        }
    }
}

/// Parse a TZif file, preferring the v2+ 64-bit block when present.
fn parse_tzif(data: &[u8]) -> Option<Tz> {
    if data.len() < 44 || &data[0..4] != b"TZif" {
        return None;
    }
    let version = data[4];
    let v1 = parse_block(data, 20, 4)?;
    if version == 0 {
        return Some(v1);
    }
    // Skip the v1 block and parse the 64-bit one.
    let end = v1_block_end(data, 20)?;
    let v2 = parse_block(data, end + 20, 8);
    v2.or(Some(v1))
}

/// Parse one data block; `time_size` is 4 for v1, 8 for v2+.
fn parse_block(data: &[u8], counts_off: usize, time_size: usize) -> Option<Tz> {
    if data.len() < counts_off + 24 {
        return None;
    }
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let isutcnt = be32(&data[counts_off..]) as usize;
    let isstdcnt = be32(&data[counts_off + 4..]) as usize;
    let leapcnt = be32(&data[counts_off + 8..]) as usize;
    let timecnt = be32(&data[counts_off + 12..]) as usize;
    let typecnt = be32(&data[counts_off + 16..]) as usize;
    let charcnt = be32(&data[counts_off + 20..]) as usize;

    let mut pos = counts_off + 24;
    let mut transitions = Vec::with_capacity(timecnt);
    for i in 0..timecnt {
        let off = pos + i * time_size;
        if off + time_size > data.len() {
            return None;
        }
        let t = if time_size == 4 {
            be32(&data[off..]) as i64
        } else {
            i64::from_be_bytes([
                data[off],
                data[off + 1],
                data[off + 2],
                data[off + 3],
                data[off + 4],
                data[off + 5],
                data[off + 6],
                data[off + 7],
            ])
        };
        transitions.push(t);
    }
    pos += timecnt * time_size;
    let type_idx = &data.get(pos..pos + timecnt)?;
    pos += timecnt;
    if pos + typecnt * 6 > data.len() {
        return None;
    }
    let mut offsets = Vec::with_capacity(typecnt);
    let mut isdst = Vec::with_capacity(typecnt);
    for i in 0..typecnt {
        let o = pos + i * 6;
        let gmtoff = i32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) / 60;
        offsets.push(gmtoff);
        isdst.push(data[o + 4] != 0);
    }
    pos += typecnt * 6;
    pos += charcnt;
    pos += leapcnt * (time_size + 4);
    pos += isstdcnt;
    pos += isutcnt;
    let _ = pos;

    // Offset before the first transition: the first non-DST type, else type 0.
    let default_offset = offsets
        .iter()
        .zip(isdst.iter())
        .find(|(_, dst)| !**dst)
        .map(|(o, _)| *o)
        .or_else(|| offsets.first().copied())
        .unwrap_or(0);

    let mut out = Vec::with_capacity(timecnt);
    for (i, t) in transitions.into_iter().enumerate() {
        let ty = *type_idx.get(i).unwrap_or(&0) as usize;
        let off = offsets.get(ty).copied().unwrap_or(0);
        out.push((t, off));
    }
    Some(Tz { transitions: out, default_offset })
}

/// End offset of the v1 data block (the start of the second header).
fn v1_block_end(data: &[u8], counts_off: usize) -> Option<usize> {
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let isutcnt = be32(&data[counts_off..]) as usize;
    let isstdcnt = be32(&data[counts_off + 4..]) as usize;
    let leapcnt = be32(&data[counts_off + 8..]) as usize;
    let timecnt = be32(&data[counts_off + 12..]) as usize;
    let typecnt = be32(&data[counts_off + 16..]) as usize;
    let charcnt = be32(&data[counts_off + 20..]) as usize;
    Some(counts_off + 24 + timecnt * 4 + timecnt + typecnt * 6 + charcnt + leapcnt * 8 + isstdcnt + isutcnt)
}

/// The local UTC offset (minutes, east positive) in effect at Unix time
/// `secs` — the equivalent of C git's `localtime_r` + `tm_gmtoff`.
pub fn local_offset(secs: i64) -> i32 {
    match load_tz() {
        Some(tz) => {
            // Binary search for the last transition at or before `secs`.
            let idx = tz
                .transitions
                .partition_point(|(t, _)| *t <= secs);
            if idx == 0 {
                tz.default_offset
            } else {
                tz.transitions[idx - 1].1
            }
        }
        None => 0,
    }
}

/// Convert a local wall-clock time (no explicit offset given) to a Unix
/// timestamp plus the local offset that applies to it.
pub fn wall_to_local(wall_secs: i64) -> (i64, i32) {
    let off1 = local_offset(wall_secs);
    let epoch1 = wall_secs - (off1 as i64) * 60;
    let off2 = local_offset(epoch1);
    if off2 == off1 {
        (epoch1, off1)
    } else {
        (wall_secs - (off2 as i64) * 60, off2)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Serialize tests that touch the `TZ` environment variable across all
    /// of `git-date`'s test modules.
    pub(crate) fn lock_tz(tz: &str) -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        std::env::set_var("TZ", tz);
        guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tz::test_support::lock_tz;

    #[test]
    fn utc_env_is_zero() {
        let _tz = lock_tz("UTC");
        assert_eq!(local_offset(1582024274), 0);
    }

    #[test]
    fn reads_a_zoneinfo_file() {
        // Only meaningful on systems with a tz database.
        if !std::path::Path::new("/usr/share/zoneinfo/Asia/Tehran").exists() {
            return;
        }
        let _tz = lock_tz("Asia/Tehran");
        // 2020-02-18 11:11:14 local = 07:41:14 UTC, offset +03:30.
        let wall = 1582024274; // 2020-02-18 11:11:14 +0330
        let utc = 1582024274 - 3 * 3600 - 30 * 60;
        assert_eq!(local_offset(utc), 210);
        let _ = wall;
    }
}
