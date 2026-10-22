//! Local-time date helpers. No time crate: the only thing needed is "which local days does
//! today / yesterday / the last week cover", as UTC cut-offs for the walker and as local calendar
//! dates for the search index.

use marmalade_core::Since;

const DAY: i64 = 86_400;

/// A half-open UTC range in unix seconds: `from <= t < to` (`to == None` means "now").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub from: i64,
    pub to: Option<i64>,
}

impl DateRange {
    pub fn contains(&self, t: i64) -> bool {
        t >= self.from && self.to.is_none_or(|to| t < to)
    }
}

/// Cut-offs for a relative date, anchored to local midnight.
///
/// `tz_offset` is local minus UTC in seconds. "Last week" is the last 7 days including today and
/// "last month" the last 30, which is how people use the words, not calendar weeks.
pub fn date_range(since: Since, now_utc: i64, tz_offset: i64) -> DateRange {
    let local_now = now_utc + tz_offset;
    let midnight_utc = local_now - local_now.rem_euclid(DAY) - tz_offset;
    match since {
        Since::Today => DateRange {
            from: midnight_utc,
            to: None,
        },
        Since::Yesterday => DateRange {
            from: midnight_utc - DAY,
            to: Some(midnight_utc),
        },
        Since::Week => DateRange {
            from: midnight_utc - 6 * DAY,
            to: None,
        },
        Since::Month => DateRange {
            from: midnight_utc - 29 * DAY,
            to: None,
        },
    }
}

/// `YYYY-MM-DD` of the local calendar day containing the UTC instant.
pub fn iso_local_date(t_utc: i64, tz_offset: i64) -> String {
    let (y, m, d) = civil_from_days((t_utc + tz_offset).div_euclid(DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
/// Howard Hinnant's `civil_from_days`.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Local minus UTC in seconds, from the Windows time-zone settings (0 if unavailable).
#[cfg(windows)]
pub fn local_tz_offset() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    // SAFETY: `tz` is a valid out-parameter.
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    // Windows reports the bias as UTC = local + bias (minutes).
    let bias_min = match id {
        1 => tz.Bias + tz.StandardBias,
        2 => tz.Bias + tz.DaylightBias,
        _ => tz.Bias,
    };
    -(bias_min as i64) * 60
}

#[cfg(not(windows))]
pub fn local_tz_offset() -> i64 {
    0
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
