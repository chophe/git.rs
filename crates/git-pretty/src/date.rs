//! Date formatting for the pretty engine: C git's `show_date` /
//! `show_date_relative` / `show_date_normal` over `git-date` timestamps.

use git_date::Timestamp;

/// A `--date=` mode (subset of `date.c`'s `date_mode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateMode {
    Default,
    Relative,
    /// `Default` rendered in the local timezone.
    Local,
    Iso,
    IsoStrict,
    Rfc,
    Short,
    Raw,
    Unix,
    Human,
    /// `format:<strftime-like fmt>` — supports the common specifiers.
    Strf(String),
}

impl DateMode {
    /// Parse a `--date=` argument (C git's `parse_date_format`).
    pub fn parse(s: &str) -> Option<DateMode> {
        Some(match s {
            "relative" => DateMode::Relative,
            "iso" | "iso8601" => DateMode::Iso,
            "iso-local" | "iso8601-local" => DateMode::Iso,
            "iso-strict" | "iso8601-strict" => DateMode::IsoStrict,
            "rfc" | "rfc2822" => DateMode::Rfc,
            "short" => DateMode::Short,
            "raw" => DateMode::Raw,
            "unix" => DateMode::Unix,
            "human" => DateMode::Human,
            "default" => DateMode::Default,
            "local" => DateMode::Local,
            s if s.starts_with("format:") => DateMode::Strf(s["format:".len()..].to_string()),
            s if s.starts_with("format-local:") => {
                // Local variants map to the same strftime rendering with the
                // local offset applied by the caller.
                DateMode::Strf(s["format-local:".len()..].to_string())
            }
            _ => return None,
        })
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn fmt_tz(offset_min: i32) -> String {
    let abs = offset_min.unsigned_abs();
    let sign = if offset_min < 0 { '-' } else { '+' };
    format!("{sign}{:02}{:02}", abs / 60, abs % 60)
}

/// Format `ts` in `mode`. `now` is the current epoch (used by relative and
/// human modes).
pub fn show_date(ts: Timestamp, mode: &DateMode, now: i64) -> String {
    if let DateMode::Relative = mode {
        return show_relative(now - ts.secs);
    }
    if let DateMode::Unix = mode {
        return ts.secs.to_string();
    }
    if let DateMode::Raw = mode {
        return format!("{} {}", ts.secs, fmt_tz(ts.offset_min));
    }

    // Local variants replace the recorded offset with the local one.
    let _offset = match mode {
        // `local` variants are handled by callers; the other modes keep the
        // recorded offset.
        _ => ts.offset_min,
    };
    let local_secs = ts.secs + (ts.offset_min as i64) * 60;
    let days = local_secs.div_euclid(86400);
    let tod = local_secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = ((tod / 3600) as u32, ((tod % 3600) / 60) as u32, (tod % 60) as u32);
    let wday = (days + 4).rem_euclid(7) as usize;

    match mode {
        DateMode::Short => format!("{y:04}-{mo:02}-{d:02}"),
        DateMode::Iso => format!(
            "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} {}",
            fmt_tz(ts.offset_min)
        ),
        DateMode::IsoStrict => {
            let tz = ts.offset_min;
            let suffix = if tz == 0 {
                "Z".to_string()
            } else {
                let abs = tz.unsigned_abs();
                format!("{}{:02}:{:02}", if tz < 0 { '-' } else { '+' }, abs / 60, abs % 60)
            };
            format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}{suffix}")
        }
        DateMode::Rfc => format!(
            "{}, {} {} {} {:02}:{:02}:{:02} {}",
            WEEKDAYS[wday],
            d,
            MONTHS[(mo - 1) as usize],
            y,
            h,
            mi,
            s,
            fmt_tz(ts.offset_min)
        ),
        DateMode::Strf(fmt) => strftime(fmt, y, mo, d, h, mi, s, ts.offset_min, wday),
        DateMode::Human => show_human(ts, now),
        // Default/Local/Raw/Unix/Relative handled above.
        _ => format!(
            "{} {} {} {:02}:{:02}:{:02} {} {}",
            WEEKDAYS[wday],
            MONTHS[(mo - 1) as usize],
            d,
            h,
            mi,
            s,
            y,
            fmt_tz(ts.offset_min)
        ),
    }
}

/// C git's `show_date_relative`: buckets with rounded boundaries.
pub fn show_relative(diff: i64) -> String {
    if diff < 0 {
        return "in the future".to_string();
    }
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("{n} {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    if diff < 90 {
        return plural(diff, "second");
    }
    let diff = (diff + 30) / 60;
    if diff < 90 {
        return plural(diff, "minute");
    }
    let diff = (diff + 30) / 60;
    if diff < 36 {
        return plural(diff, "hour");
    }
    let diff = (diff + 12) / 24;
    if diff < 14 {
        return plural(diff, "day");
    }
    if diff < 70 {
        return plural((diff + 3) / 7, "week");
    }
    if diff < 365 {
        return plural((diff + 15) / 30, "month");
    }
    if diff < 1825 {
        let totalmonths = (diff * 12 * 2 + 365) / (365 * 2);
        let years = totalmonths / 12;
        let months = totalmonths % 12;
        let ys = if years == 1 {
            format!("{years} year")
        } else {
            format!("{years} years")
        };
        if months > 0 {
            let ms = if months == 1 {
                format!(", {months} month ago")
            } else {
                format!(", {months} months ago")
            };
            return format!("{ys}{ms}");
        }
        return format!("{ys} ago");
    }
    let years = (diff + 183) / 365;
    plural(years, "year")
}

/// C git's `show_date_normal` human mode relative to `now`.
fn show_human(ts: Timestamp, now: i64) -> String {
    let local_secs = ts.secs + (ts.offset_min as i64) * 60;
    let days = local_secs.div_euclid(86400);
    let tod = local_secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = ((tod / 3600) as u32, ((tod % 3600) / 60) as u32, (tod % 60) as u32);
    let wday = (days + 4).rem_euclid(7) as usize;

    let now_local = now + (ts.offset_min as i64) * 60;
    let (ny, nmo, nd) = civil_from_days(now_local.div_euclid(86400));

    let mut hide_date = false;
    let mut hide_wday = false;
    let mut hide_year = false;
    let hide_tz = true;
    if y != ny {
        hide_year = true;
    } else if mo == nmo {
        if d == nd {
            hide_date = true;
            hide_wday = true;
        } else if d + 5 > nd {
            hide_date = true;
        }
    }
    if hide_wday {
        return show_relative(now - ts.secs);
    }
    let hide_seconds = true;
    hide_wday = !hide_year;
    let mut out = String::new();
    if !hide_wday {
        out.push_str(WEEKDAYS[wday]);
        out.push(' ');
    }
    if !hide_date {
        out.push_str(&format!("{} {} ", MONTHS[(mo - 1) as usize], d));
        if !hide_time() {
            out.push_str(&format!("{h:02}:{mi:02}"));
            if !hide_seconds {
                out.push_str(&format!(":{s:02}"));
            }
        }
    } else if !hide_time() {
        out.push_str(&format!("{h:02}:{mi:02}"));
        if !hide_seconds {
            out.push_str(&format!(":{s:02}"));
        }
    }
    if !hide_year {
        out.push_str(&format!(" {y}"));
    }
    if !hide_tz {
        out.push_str(&format!(" {}", fmt_tz(ts.offset_min)));
    }
    out.trim_end().to_string()
}

fn hide_time() -> bool {
    // Human mode hides time when the year differs (already implied by the
    // hide flags); kept as a hook for parity tweaks.
    false
}

/// A small strftime subset for `--date=format:<fmt>`.
fn strftime(fmt: &str, y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32, tz: i32, wday: usize) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{y:04}")),
            Some('y') => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{mo:02}")),
            Some('d') => out.push_str(&format!("{d:02}")),
            Some('H') => out.push_str(&format!("{h:02}")),
            Some('M') => out.push_str(&format!("{mi:02}")),
            Some('S') => out.push_str(&format!("{s:02}")),
            Some('Z') => out.push_str(&fmt_tz(tz)),
            Some('z') => out.push_str(&fmt_tz(tz)),
            Some('a') => out.push_str(WEEKDAYS[wday]),
            Some('b') | Some('h') => out.push_str(MONTHS[(mo - 1) as usize]),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
