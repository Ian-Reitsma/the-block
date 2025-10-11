#![forbid(unsafe_code)]

use std::fmt;
use std::ops::{Add, Sub};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const SECONDS_PER_DAY: i128 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration {
    nanos: i128,
}

impl Duration {
    pub const ZERO: Self = Self { nanos: 0 };

    pub const fn seconds(seconds: i64) -> Self {
        Self {
            nanos: seconds as i128 * NANOS_PER_SECOND,
        }
    }

    pub const fn minutes(minutes: i64) -> Self {
        Self {
            nanos: minutes as i128 * 60 * NANOS_PER_SECOND,
        }
    }

    pub const fn hours(hours: i64) -> Self {
        Self {
            nanos: hours as i128 * 3_600 * NANOS_PER_SECOND,
        }
    }

    pub const fn days(days: i64) -> Self {
        Self {
            nanos: days as i128 * 86_400 * NANOS_PER_SECOND,
        }
    }

    pub const fn total_seconds(self) -> i128 {
        self.nanos / NANOS_PER_SECOND
    }

    pub(crate) const fn total_nanos(self) -> i128 {
        self.nanos
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDateTime {
    nanos_since_unix_epoch: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeError;

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timestamp out of range")
    }
}

impl std::error::Error for RangeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    Iso8601,
    CompactDate,
    CompactDateTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatError;

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "datetime formatting failed")
    }
}

impl std::error::Error for FormatError {}

impl UtcDateTime {
    pub fn now() -> Self {
        SystemTime::now().into()
    }

    pub fn from_unix_timestamp(seconds: i64) -> Result<Self, RangeError> {
        Ok(Self {
            nanos_since_unix_epoch: seconds as i128 * NANOS_PER_SECOND,
        })
    }

    pub fn unix_timestamp(self) -> Result<i64, RangeError> {
        let seconds = self.nanos_since_unix_epoch / NANOS_PER_SECOND;
        if seconds < i64::MIN as i128 || seconds > i64::MAX as i128 {
            return Err(RangeError);
        }
        Ok(seconds as i64)
    }

    pub fn format(&self, kind: FormatKind) -> Result<String, FormatError> {
        let components = self.components()?;
        let mut out = String::with_capacity(components.estimated_length(kind));
        match kind {
            FormatKind::Iso8601 => {
                write_iso8601(&mut out, &components);
            }
            FormatKind::CompactDate => {
                write_compact_date(&mut out, &components);
            }
            FormatKind::CompactDateTime => {
                write_compact_datetime(&mut out, &components);
            }
        }
        Ok(out)
    }

    pub fn format_iso8601(&self) -> Result<String, FormatError> {
        self.format(FormatKind::Iso8601)
    }

    pub fn format_compact_date(&self) -> Result<String, FormatError> {
        self.format(FormatKind::CompactDate)
    }

    pub fn format_compact_datetime(&self) -> Result<String, FormatError> {
        self.format(FormatKind::CompactDateTime)
    }

    pub fn components(&self) -> Result<DateTimeComponents, FormatError> {
        let total_seconds = self.nanos_since_unix_epoch.div_euclid(NANOS_PER_SECOND);
        let seconds_of_day = self
            .nanos_since_unix_epoch
            .rem_euclid(NANOS_PER_SECOND * SECONDS_PER_DAY)
            .div_euclid(NANOS_PER_SECOND);
        let days = total_seconds.div_euclid(SECONDS_PER_DAY);
        if days < i64::MIN as i128 || days > i64::MAX as i128 {
            return Err(FormatError);
        }
        let (year, month, day) = civil_from_days(days as i64);
        let hour = (seconds_of_day / 3_600) as u8;
        let minute = ((seconds_of_day % 3_600) / 60) as u8;
        let second = (seconds_of_day % 60) as u8;
        Ok(DateTimeComponents {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }
}

impl From<SystemTime> for UtcDateTime {
    fn from(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => Self {
                nanos_since_unix_epoch: duration.as_secs() as i128 * NANOS_PER_SECOND
                    + duration.subsec_nanos() as i128,
            },
            Err(err) => {
                let duration = err.duration();
                let nanos =
                    duration.as_secs() as i128 * NANOS_PER_SECOND + duration.subsec_nanos() as i128;
                Self {
                    nanos_since_unix_epoch: -nanos,
                }
            }
        }
    }
}

impl From<UtcDateTime> for SystemTime {
    fn from(value: UtcDateTime) -> Self {
        if value.nanos_since_unix_epoch >= 0 {
            let seconds = value.nanos_since_unix_epoch / NANOS_PER_SECOND;
            if seconds > u64::MAX as i128 {
                return SystemTime::UNIX_EPOCH
                    + StdDuration::new(u64::MAX, (NANOS_PER_SECOND - 1) as u32);
            }
            let nanos = value.nanos_since_unix_epoch.rem_euclid(NANOS_PER_SECOND) as u32;
            UNIX_EPOCH + StdDuration::new(seconds as u64, nanos)
        } else {
            let nanos = (-value.nanos_since_unix_epoch) as u128;
            let seconds = nanos / NANOS_PER_SECOND as u128;
            let nanos_remainder = (nanos % NANOS_PER_SECOND as u128) as u32;
            if seconds > u64::MAX as u128 {
                return SystemTime::UNIX_EPOCH;
            }
            UNIX_EPOCH
                .checked_sub(StdDuration::new(seconds as u64, nanos_remainder))
                .unwrap_or(SystemTime::UNIX_EPOCH)
        }
    }
}

impl Add<Duration> for UtcDateTime {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self {
            nanos_since_unix_epoch: self.nanos_since_unix_epoch + rhs.total_nanos(),
        }
    }
}

impl Sub<Duration> for UtcDateTime {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        Self {
            nanos_since_unix_epoch: self.nanos_since_unix_epoch - rhs.total_nanos(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTimeComponents {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl DateTimeComponents {
    fn estimated_length(&self, kind: FormatKind) -> usize {
        match kind {
            FormatKind::Iso8601 => 20,
            FormatKind::CompactDate => 8,
            FormatKind::CompactDateTime => 16,
        }
    }
}

fn write_iso8601(buffer: &mut String, c: &DateTimeComponents) {
    use fmt::Write;
    write!(
        buffer,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
    .unwrap();
}

fn write_compact_date(buffer: &mut String, c: &DateTimeComponents) {
    use fmt::Write;
    write!(buffer, "{:04}{:02}{:02}", c.year, c.month, c.day).unwrap();
}

fn write_compact_datetime(buffer: &mut String, c: &DateTimeComponents) {
    use fmt::Write;
    write!(
        buffer,
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
    .unwrap();
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (mp + if mp < 10 { 3 } else { -9 }) as u8;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_match_expectations() {
        let ts = UtcDateTime::from_unix_timestamp(1_369_353_600).unwrap();
        assert_eq!(ts.format_iso8601().unwrap(), "2013-05-24T00:00:00Z");
        assert_eq!(ts.format_compact_date().unwrap(), "20130524");
        assert_eq!(ts.format_compact_datetime().unwrap(), "20130524T000000Z");
    }

    #[test]
    fn duration_arithmetic() {
        let base = UtcDateTime::from_unix_timestamp(0).unwrap();
        let later = base + Duration::days(2) + Duration::hours(6);
        assert_eq!(later.unix_timestamp().unwrap(), 183_600);
        let earlier = later - Duration::hours(1);
        assert_eq!(earlier.unix_timestamp().unwrap(), 179_000);
    }

    #[test]
    fn now_round_trip() {
        let now = UtcDateTime::now();
        let system: SystemTime = now.into();
        let round = UtcDateTime::from(system);
        assert!((round.nanos_since_unix_epoch - now.nanos_since_unix_epoch).abs() < 1_000_000);
    }
}
