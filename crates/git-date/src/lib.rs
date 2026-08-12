//! Date and timestamp parsing and formatting.
//!
//! A git-compatible subset of `date.c`. Timestamps carry a UTC offset in
//! minutes (east of UTC), matching git's internal `date.offset`.
//!
//! Scope note: no timezone database is consulted; inputs without an explicit
//! offset are interpreted as UTC (git uses the local timezone). Calendar math
//! uses the proleptic Gregorian calendar via the well-known civil/epoch
//! conversions.

use std::error::Error;
use std::fmt;

/// A point in time with a UTC offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// Seconds since the Unix epoch (UTC).
    pub secs: i64,
    /// Offset from UTC in minutes, east is positive.
    pub offset_min: i32,
}

impl Timestamp {
    pub fn new(secs: i64, offset_min: i32) -> Timestamp {
        Timestamp { secs, offset_min }
    }
}

/// Errors returned while parsing dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateError {
    UnknownFormat,
    InvalidInteger,
    OutOfRange,
    InvalidTimezone,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DateError::UnknownFormat => "unknown date format",
            DateError::InvalidInteger => "invalid integer in date",
            DateError::OutOfRange => "date out of range",
            DateError::InvalidTimezone => "invalid timezone",
        };
        write!(f, "{s}")
    }
}

impl Error for DateError {}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + (if m > 2 { -3 } else { 9 })) as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Civil date from days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Weekday index, 0 = Sunday.
fn weekday_from_days(z: i64) -> usize {
    (z + 4).rem_euclid(7) as usize
}

fn secs_from_ymdhms(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<i64> {
    let days = days_from_civil(y, mo, d);
    days.checked_mul(86400)?
        .checked_add(h as i64 * 3600 + mi as i64 * 60 + s as i64)
}

/// Parse a timezone suffix like `+0200`, `-0530`, or `+02:00` into minutes.
fn parse_tz(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    if b.len() < 3 {
        return None;
    }
    let sign = match b[0] {
        b'+' => 1i32,
        b'-' => -1i32,
        _ => return None,
    };
    let (hh, mm) = match b.len() {
        5 => {
            // +HHMM
            let hh = parse_digits(&s[1..3])?;
            let mm = parse_digits(&s[3..5])?;
            (hh, mm)
        }
        6 => {
            // +HH:MM
            let hh = parse_digits(&s[1..3])?;
            if b[3] != b':' {
                return None;
            }
            let mm = parse_digits(&s[4..6])?;
            (hh, mm)
        }
        _ => return None,
    };
    if hh > 24 || mm > 59 {
        return None;
    }
    Some(sign * (hh * 60 + mm))
}

fn parse_digits(s: &str) -> Option<i32> {
    s.parse().ok()
}

/// The offset as `+HHMM`.
fn fmt_tz(offset_min: i32) -> String {
    let abs = offset_min.unsigned_abs() as u32;
    let sign = if offset_min < 0 { '-' } else { '+' };
    format!("{sign}{:02}{:02}", abs / 60, abs % 60)
}

impl Timestamp {
    /// Format like git's default: `2020-02-18 11:11:14 +0000`.
    pub fn format_default(self) -> String {
        self.format_iso()
    }

    /// Format as `YYYY-MM-DD HH:MM:SS +HHMM`.
    pub fn format_iso(self) -> String {
        let local = self.secs + (self.offset_min as i64) * 60;
        let (y, mo, d) = civil_from_days(local.div_euclid(86400));
        let tod = local.rem_euclid(86400);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
            y,
            mo,
            d,
            tod / 3600,
            (tod % 3600) / 60,
            tod % 60,
            fmt_tz(self.offset_min)
        )
    }

    /// Format as `Wed, 14 Oct 2015 12:00:00 +0200` (RFC 2822 / git `%aD`).
    pub fn format_rfc2822(self) -> String {
        let local = self.secs + (self.offset_min as i64) * 60;
        let days = local.div_euclid(86400);
        let tod = local.rem_euclid(86400);
        let (y, mo, d) = civil_from_days(days);
        format!(
            "{}, {:02} {} {:04} {:02}:{:02}:{:02} {}",
            DAYS[weekday_from_days(days)],
            d,
            MONTHS[(mo - 1) as usize],
            y,
            tod / 3600,
            (tod % 3600) / 60,
            tod % 60,
            fmt_tz(self.offset_min)
        )
    }

    /// Format as `1581990674 +0000` (git `--date=raw`).
    pub fn format_raw(self) -> String {
        format!("{} {}", self.secs, fmt_tz(self.offset_min))
    }
}

/// Parse a date string.
///
/// Supported forms:
/// - epoch: `1234567890` or `@1234567890`
/// - ISO: `2020-02-18 11:11:14 +0000` (also `T` separator, optional tz)
/// - RFC 2822: `Wed, 14 Oct 2015 12:00:00 +0200`
/// - relative: `now`, `yesterday`, `tomorrow`, `N units ago` for
///   second/minute/hour/day/week (month/year approximate)
pub fn parse(s: &str, now: Timestamp) -> Result<Timestamp, DateError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(DateError::UnknownFormat);
    }
    if let Some(rest) = t.strip_prefix('@') {
        let secs = rest.parse::<i64>().map_err(|_| DateError::InvalidInteger)?;
        return Ok(Timestamp::new(secs, 0));
    }
    if let Ok(secs) = t.parse::<i64>() {
        // Pure integer: treat as raw epoch.
        return Ok(Timestamp::new(secs, 0));
    }

    if t == "now" {
        return Ok(now);
    }
    if t == "yesterday" {
        return Ok(Timestamp::new(now.secs - 86400, now.offset_min));
    }
    if t == "tomorrow" {
        return Ok(Timestamp::new(now.secs + 86400, now.offset_min));
    }
    if let Some(rel) = parse_relative(t, now) {
        return Ok(rel);
    }

    // Try structured formats; only hard errors (out-of-range fields) propagate,
    // structural mismatches fall through to the next format.
    match parse_iso(t)? {
        Some(ts) => return Ok(ts),
        None => {}
    }
    match parse_rfc2822(t)? {
        Some(ts) => return Ok(ts),
        None => {}
    }
    Err(DateError::UnknownFormat)
}

fn parse_relative(s: &str, now: Timestamp) -> Option<Timestamp> {
    let s = s.trim();
    let (amount_s, rest) = s.split_once(' ')?;
    let amount: i64 = amount_s.parse().ok()?;
    let mut words = rest.split_whitespace();
    let unit = words.next()?;
    let ago = words.next() == Some("ago");
    let secs = match unit.trim_end_matches('s') {
        "second" => amount * 1,
        "minute" => amount * 60,
        "hour" => amount * 3600,
        "day" => amount * 86400,
        "week" => amount * 7 * 86400,
        // Approximate; git uses calendar-aware math here.
        "month" => amount * 30 * 86400,
        "year" => amount * 365 * 86400,
        _ => return None,
    };
    Some(Timestamp::new(if ago { now.secs - secs } else { now.secs + secs }, now.offset_min))
}

fn parse_iso(s: &str) -> Result<Option<Timestamp>, DateError> {
    // Expected: YYYY-MM-DD[ T]HH:MM[:SS][ TZ]
    let s = s.replace('T', " ");
    let (date_part, rest) = match s.split_once(' ') {
        Some(x) => x,
        None => return Ok(None),
    };
    let mut parts = date_part.split('-');
    let y = match parts.next().and_then(parse_digits) {
        Some(y) => y as i64,
        None => return Ok(None),
    };
    let mo = match parts.next().and_then(parse_digits) {
        Some(mo) => mo as u32,
        None => return Ok(None),
    };
    let d = match parts.next().and_then(parse_digits) {
        Some(d) => d as u32,
        None => return Ok(None),
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return Err(DateError::OutOfRange);
    }

    let (time_part, tz_part) = match rest.split_once(' ') {
        Some((tp, tz)) => (tp, Some(tz)),
        None => (rest, None),
    };
    let mut tp = time_part.split(':');
    let h: u32 = match tp.next().and_then(parse_digits) {
        Some(h) => h as u32,
        None => return Ok(None),
    };
    let mi: u32 = match tp.next().and_then(parse_digits) {
        Some(mi) => mi as u32,
        None => return Ok(None),
    };
    let sec: u32 = match tp.next().and_then(parse_digits) {
        Some(x) => x as u32,
        None => 0,
    };
    if h > 23 || mi > 59 || sec > 60 {
        return Err(DateError::OutOfRange);
    }

    let offset = match tz_part {
        Some(tz) => parse_tz(tz).ok_or(DateError::InvalidTimezone)?,
        None => 0,
    };

    let secs = secs_from_ymdhms(y, mo, d, h, mi, sec).ok_or(DateError::OutOfRange)?;
    Ok(Some(Timestamp::new(secs - (offset as i64) * 60, offset)))
}

fn parse_rfc2822(s: &str) -> Result<Option<Timestamp>, DateError> {
    // Expected: Dow, DD Mon YYYY HH:MM[:SS] TZ
    let s = s.replace(',', " ");
    let mut parts = s.split_whitespace();
    // Dow (optional, unvalidated)
    let _dow = match parts.next() {
        Some(dow) => dow,
        None => return Ok(None),
    };
    let d = match parts.next().and_then(parse_digits) {
        Some(d) => d as u32,
        None => return Ok(None),
    };
    let mon = match parts.next() {
        Some(mon) => mon,
        None => return Ok(None),
    };
    let mo = match MONTHS.iter().position(|m| *m == mon) {
        Some(i) => i as u32 + 1,
        None => return Ok(None),
    };
    let y = match parts.next().and_then(parse_digits) {
        Some(y) => y as i64,
        None => return Ok(None),
    };
    let time = match parts.next() {
        Some(t) => t,
        None => return Ok(None),
    };
    let mut tp = time.split(':');
    let h: u32 = match tp.next().and_then(parse_digits) {
        Some(h) => h as u32,
        None => return Ok(None),
    };
    let mi: u32 = match tp.next().and_then(parse_digits) {
        Some(mi) => mi as u32,
        None => return Ok(None),
    };
    let sec: u32 = match tp.next().and_then(parse_digits) {
        Some(x) => x as u32,
        None => 0,
    };
    if !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return Err(DateError::OutOfRange);
    }
    let offset = match parts.next() {
        Some(tz) => parse_tz(tz).ok_or(DateError::InvalidTimezone)?,
        None => 0,
    };

    let secs = secs_from_ymdhms(y, mo, d, h, mi, sec).ok_or(DateError::OutOfRange)?;
    Ok(Some(Timestamp::new(secs - (offset as i64) * 60, offset)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp {
        secs: 1_700_000_000,
        offset_min: 0,
    };

    #[test]
    fn parses_epoch_forms() {
        assert_eq!(parse("1234567890", NOW).unwrap(), Timestamp::new(1_234_567_890, 0));
        assert_eq!(parse("@1234567890", NOW).unwrap(), Timestamp::new(1_234_567_890, 0));
    }

    #[test]
    fn parses_iso() {
        // 2020-02-18 11:11:14 +0000 == epoch 1582024274
        let ts = parse("2020-02-18 11:11:14 +0000", NOW).unwrap();
        assert_eq!(ts, Timestamp::new(1_582_024_274, 0));
        // With a positive offset the epoch shifts earlier.
        let ts = parse("2020-02-18 11:11:14 +0200", NOW).unwrap();
        assert_eq!(ts, Timestamp::new(1_582_024_274 - 2 * 3600, 120));
        // T separator and no timezone.
        let ts = parse("2020-02-18T11:11:14", NOW).unwrap();
        assert_eq!(ts.secs, 1_582_024_274);
    }

    #[test]
    fn parses_rfc2822() {
        // Wed, 18 Feb 2020 11:11:14 +0000
        let ts = parse("Wed, 18 Feb 2020 11:11:14 +0000", NOW).unwrap();
        assert_eq!(ts, Timestamp::new(1_582_024_274, 0));
        // The day-of-week token is accepted but not validated.
        let ts = parse("Thu, 18 Feb 2020 11:11:14 +0000", NOW).unwrap();
        assert_eq!(ts.secs, 1_582_024_274);
    }

    #[test]
    fn parses_relative() {
        assert_eq!(parse("now", NOW).unwrap(), NOW);
        assert_eq!(parse("yesterday", NOW).unwrap().secs, NOW.secs - 86400);
        assert_eq!(parse("tomorrow", NOW).unwrap().secs, NOW.secs + 86400);
        assert_eq!(parse("2 days ago", NOW).unwrap().secs, NOW.secs - 2 * 86400);
        assert_eq!(parse("90 minutes ago", NOW).unwrap().secs, NOW.secs - 90 * 60);
        assert_eq!(parse("1 week ago", NOW).unwrap().secs, NOW.secs - 7 * 86400);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("", NOW).is_err());
        assert!(parse("not a date", NOW).is_err());
        assert!(parse("2020-13-40 00:00:00 +0000", NOW).is_err());
        assert!(parse("2020-02-18 11:11:14 +9x00", NOW).is_err());
    }

    #[test]
    fn formats() {
        let ts = Timestamp::new(1_582_024_274, 0);
        // 2020-02-18 was a Tuesday.
        assert_eq!(ts.format_iso(), "2020-02-18 11:11:14 +0000");
        assert_eq!(ts.format_rfc2822(), "Tue, 18 Feb 2020 11:11:14 +0000");
        assert_eq!(ts.format_raw(), "1582024274 +0000");

        let shifted = Timestamp::new(1_582_024_274, 120);
        // +0200: local time is 13:11:14 on the same date.
        assert_eq!(shifted.format_iso(), "2020-02-18 13:11:14 +0200");
        assert_eq!(shifted.format_rfc2822(), "Tue, 18 Feb 2020 13:11:14 +0200");
    }

    #[test]
    fn parse_and_format_round_trip() {
        for input in [
            "2020-02-18 11:11:14 +0000",
            "2021-12-31 23:59:59 -0530",
            "1999-01-01 00:00:00 +0230",
        ] {
            let ts = parse(input, NOW).unwrap();
            assert_eq!(ts.format_iso(), input, "round-trip {input}");
        }
    }

    #[test]
    fn rfc2822_round_trip() {
        let ts = parse("Sun, 06 Nov 1994 08:49:37 +0000", NOW).unwrap();
        assert_eq!(ts.format_rfc2822(), "Sun, 06 Nov 1994 08:49:37 +0000");
    }
}
