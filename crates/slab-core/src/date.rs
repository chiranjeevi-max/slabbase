//! Calendar dates and UTC timestamps with deterministic text forms.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CoreError;

/// A proleptic Gregorian calendar date stored as days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlabDate {
    days: i32,
}

impl SlabDate {
    /// Builds a date from a year, month, and day.
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Result<Self, CoreError> {
        if !(1..=9999).contains(&year) {
            return Err(CoreError::InvalidDate(format!("year {year} out of range")));
        }
        if !(1..=12).contains(&month) {
            return Err(CoreError::InvalidDate(format!(
                "month {month} out of range"
            )));
        }
        let max_day = days_in_month(year, month);
        if day == 0 || day > max_day {
            return Err(CoreError::InvalidDate(format!(
                "day {day} out of range for {year}-{month:02}"
            )));
        }
        let days = days_from_civil(year, month, day);
        let days =
            i32::try_from(days).map_err(|_| CoreError::InvalidDate("date out of range".into()))?;
        Ok(Self { days })
    }

    /// Days since 1970-01-01.
    pub const fn days_since_epoch(self) -> i32 {
        self.days
    }

    /// Builds a date from days since 1970-01-01.
    pub fn from_days_since_epoch(days: i32) -> Result<Self, CoreError> {
        let (year, _, _) = civil_from_days(i64::from(days));
        if !(1..=9999).contains(&year) {
            return Err(CoreError::InvalidDate("date out of range".into()));
        }
        Ok(Self { days })
    }

    /// The year.
    pub fn year(self) -> i32 {
        civil_from_days(i64::from(self.days)).0
    }

    /// The month, 1 through 12.
    pub fn month(self) -> u8 {
        civil_from_days(i64::from(self.days)).1
    }

    /// The day of the month.
    pub fn day(self) -> u8 {
        civil_from_days(i64::from(self.days)).2
    }

    /// Adds days, rejecting overflow outside the supported year range.
    pub fn checked_add_days(self, days: i32) -> Result<Self, CoreError> {
        let shifted = self
            .days
            .checked_add(days)
            .ok_or_else(|| CoreError::InvalidDate("date overflow".into()))?;
        Self::from_days_since_epoch(shifted)
    }

    /// Parses `YYYY-MM-DD`.
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let bytes = input.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(CoreError::InvalidDate(input.to_owned()));
        }
        let year: i32 = input[0..4]
            .parse()
            .map_err(|_| CoreError::InvalidDate(input.to_owned()))?;
        let month: u8 = input[5..7]
            .parse()
            .map_err(|_| CoreError::InvalidDate(input.to_owned()))?;
        let day: u8 = input[8..10]
            .parse()
            .map_err(|_| CoreError::InvalidDate(input.to_owned()))?;
        Self::from_ymd(year, month, day)
    }

    /// Formats as `YYYY-MM-DD`.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year(), self.month(), self.day())
    }
}

impl fmt::Display for SlabDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.iso())
    }
}

impl Serialize for SlabDate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.iso())
    }
}

impl<'de> Deserialize<'de> for SlabDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DateVisitor;

        impl Visitor<'_> for DateVisitor {
            type Value = SlabDate;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a date string in YYYY-MM-DD form")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                SlabDate::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(DateVisitor)
    }
}

/// A UTC timestamp stored as milliseconds since the Unix epoch.
///
/// The supported calendar range is 1..=9999. `date()` clamps an out-of-range
/// instant to that era so every `SlabDate` it returns is valid and parseable;
/// `millis()` still exposes the exact stored instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlabTimestamp {
    millis: i64,
}

impl SlabTimestamp {
    /// Builds a timestamp from Unix milliseconds.
    pub const fn from_millis(millis: i64) -> Self {
        Self { millis }
    }

    /// Unix milliseconds.
    pub const fn millis(self) -> i64 {
        self.millis
    }

    /// Builds a timestamp from calendar fields. Millis are clamped to 0..=999.
    pub fn from_parts(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        millis: u16,
    ) -> Result<Self, CoreError> {
        if hour > 23 || minute > 59 || second > 59 || millis > 999 {
            return Err(CoreError::InvalidDate(format!(
                "time {hour:02}:{minute:02}:{second:02}.{millis:03} out of range"
            )));
        }
        let date = SlabDate::from_ymd(year, month, day)?;
        let seconds = i64::from(date.days_since_epoch())
            .checked_mul(86_400)
            .and_then(|value| value.checked_add(i64::from(hour) * 3_600))
            .and_then(|value| value.checked_add(i64::from(minute) * 60))
            .and_then(|value| value.checked_add(i64::from(second)))
            .ok_or_else(|| CoreError::InvalidDate("timestamp overflow".into()))?;
        let millis_total = seconds
            .checked_mul(1_000)
            .and_then(|value| value.checked_add(i64::from(millis)))
            .ok_or_else(|| CoreError::InvalidDate("timestamp overflow".into()))?;
        Ok(Self {
            millis: millis_total,
        })
    }

    /// The UTC calendar date. Instants outside the supported 1..=9999 era are
    /// clamped to its first or last day so the returned `SlabDate` is valid.
    pub fn date(self) -> SlabDate {
        let days = self.clamped_millis().div_euclid(86_400_000);
        SlabDate {
            days: i32::try_from(days).unwrap_or(0),
        }
    }

    /// The instant clamped to the supported 1..=9999 era.
    fn clamped_millis(self) -> i64 {
        self.millis
            .clamp(MIN_SUPPORTED_MILLIS, MAX_SUPPORTED_MILLIS)
    }

    /// Milliseconds within the second, 0 through 999.
    pub fn millisecond(self) -> u16 {
        let value = self.clamped_millis().rem_euclid(1_000);
        u16::try_from(value).unwrap_or(0)
    }

    /// Seconds within the minute.
    pub fn second(self) -> u8 {
        let value = self.clamped_millis().div_euclid(1_000).rem_euclid(60);
        u8::try_from(value).unwrap_or(0)
    }

    /// Minutes within the hour.
    pub fn minute(self) -> u8 {
        let value = self.clamped_millis().div_euclid(60_000).rem_euclid(60);
        u8::try_from(value).unwrap_or(0)
    }

    /// Hours within the day.
    pub fn hour(self) -> u8 {
        let value = self.clamped_millis().div_euclid(3_600_000).rem_euclid(24);
        u8::try_from(value).unwrap_or(0)
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS[.mmm][Z]`, with an optional space
    /// separator. A trailing `Z` is optional; the value is always UTC.
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let trimmed = input.trim();
        let bytes = trimmed.as_bytes();
        if bytes.len() < 19 {
            return Err(CoreError::InvalidDate(trimmed.to_owned()));
        }
        if !(bytes[10] == b'T' || bytes[10] == b' ') || bytes[13] != b':' || bytes[16] != b':' {
            return Err(CoreError::InvalidDate(trimmed.to_owned()));
        }
        // Every fixed field must be sliced on an ASCII boundary. A multibyte
        // character in the seconds or fraction position would otherwise make
        // `&trimmed[..]` panic inside a character instead of returning the
        // typed invalid-date error.
        let date = trimmed
            .get(0..10)
            .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?;
        let date = SlabDate::parse(date)?;
        let hour: u8 = trimmed
            .get(11..13)
            .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?
            .parse()
            .map_err(|_| CoreError::InvalidDate(trimmed.to_owned()))?;
        let minute: u8 = trimmed
            .get(14..16)
            .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?
            .parse()
            .map_err(|_| CoreError::InvalidDate(trimmed.to_owned()))?;
        let second: u8 = trimmed
            .get(17..19)
            .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?
            .parse()
            .map_err(|_| CoreError::InvalidDate(trimmed.to_owned()))?;
        let rest = trimmed
            .get(19..)
            .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?;
        let (millis, suffix) = if let Some(fraction) = rest.strip_prefix('.') {
            let digits = fraction
                .get(0..3)
                .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?;
            let millis: u16 = digits
                .parse()
                .map_err(|_| CoreError::InvalidDate(trimmed.to_owned()))?;
            let suffix = fraction
                .get(3..)
                .ok_or_else(|| CoreError::InvalidDate(trimmed.to_owned()))?;
            (millis, suffix)
        } else {
            (0, rest)
        };
        if !(suffix.is_empty() || suffix.eq_ignore_ascii_case("Z")) {
            return Err(CoreError::InvalidDate(trimmed.to_owned()));
        }
        Self::from_parts(
            date.year(),
            date.month(),
            date.day(),
            hour,
            minute,
            second,
            millis,
        )
    }

    /// Formats as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn iso(self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}.{:03}Z",
            self.date().iso(),
            self.hour(),
            self.minute(),
            self.second(),
            self.millisecond()
        )
    }

    /// Formats as `YYYY-MM-DD HH:MM`.
    pub fn display_minute(self) -> String {
        format!(
            "{} {:02}:{:02}",
            self.date().iso(),
            self.hour(),
            self.minute()
        )
    }
}

impl fmt::Display for SlabTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.iso())
    }
}

impl Serialize for SlabTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.iso())
    }
}

impl<'de> Deserialize<'de> for SlabTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TimestampVisitor;

        impl Visitor<'_> for TimestampVisitor {
            type Value = SlabTimestamp;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a UTC timestamp in RFC 3339 form")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                SlabTimestamp::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(TimestampVisitor)
    }
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since 1970-01-01 for 0001-01-01.
const MIN_SUPPORTED_DAYS: i64 = -719_162;
/// Days since 1970-01-01 for 9999-12-31.
const MAX_SUPPORTED_DAYS: i64 = 2_932_896;
/// First supported Unix millisecond.
pub const MIN_SUPPORTED_MILLIS: i64 = MIN_SUPPORTED_DAYS * 86_400_000;
/// Last supported Unix millisecond.
pub const MAX_SUPPORTED_MILLIS: i64 = MAX_SUPPORTED_DAYS * 86_400_000 + 86_399_999;

const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year + i64::from(month <= 2);
    (
        i32::try_from(year).unwrap_or(0),
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trip() {
        assert_eq!(
            SlabDate::parse("1970-01-01")
                .expect("date")
                .days_since_epoch(),
            0
        );
        assert_eq!(
            SlabDate::from_days_since_epoch(0).expect("date").iso(),
            "1970-01-01"
        );
    }

    #[test]
    fn leap_day_is_valid_only_in_leap_years() {
        assert!(SlabDate::parse("2024-02-29").is_ok());
        assert!(SlabDate::parse("2025-02-29").is_err());
        assert!(SlabDate::parse("2000-02-29").is_ok());
        assert!(SlabDate::parse("1900-02-29").is_err());
    }

    #[test]
    fn dates_round_trip_across_centuries() {
        for input in ["1900-03-01", "1999-12-31", "2026-11-25", "2100-01-01"] {
            let date = SlabDate::parse(input).expect("date");
            assert_eq!(date.iso(), input);
        }
    }

    #[test]
    fn timestamp_round_trip() {
        let stamp = SlabTimestamp::parse("2026-11-25T09:12:44.017Z").expect("stamp");
        assert_eq!(stamp.iso(), "2026-11-25T09:12:44.017Z");
        assert_eq!(stamp.date().iso(), "2026-11-25");
        assert_eq!(stamp.hour(), 9);
    }

    #[test]
    fn negative_timestamps_are_utc_correct() {
        let stamp = SlabTimestamp::from_millis(-1);
        assert_eq!(stamp.iso(), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn timestamp_without_trailing_z_round_trips() {
        for input in ["2026-11-25T00:00:00", "2026-11-25 00:00:00"] {
            let stamp = SlabTimestamp::parse(input).expect("timestamp");
            assert_eq!(stamp.iso(), "2026-11-25T00:00:00.000Z");
        }
        let stamp = SlabTimestamp::parse("2026-11-25T00:00:00.017").expect("fraction");
        assert_eq!(stamp.iso(), "2026-11-25T00:00:00.017Z");
        assert_eq!(stamp.millisecond(), 17);
    }

    #[test]
    fn timestamp_date_clamps_to_the_supported_era() {
        assert_eq!(
            SlabTimestamp::from_millis(i64::MAX).date().iso(),
            "9999-12-31"
        );
        assert_eq!(
            SlabTimestamp::from_millis(i64::MIN).date().iso(),
            "0001-01-01"
        );
        assert_eq!(
            SlabTimestamp::from_millis(i64::MAX).iso(),
            "9999-12-31T23:59:59.999Z"
        );
        assert_eq!(
            SlabTimestamp::from_millis(i64::MIN).iso(),
            "0001-01-01T00:00:00.000Z"
        );
        for days in [MIN_SUPPORTED_DAYS, MAX_SUPPORTED_DAYS] {
            let days = i32::try_from(days).expect("in i32 range");
            assert!(SlabDate::from_days_since_epoch(days).is_ok());
        }
    }
}
