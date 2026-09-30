//! IANA timezone handling for day windows and naive timestamp parsing.
//!
//! Days are not stored; a day is the half-open instant range between two
//! local midnights in the zone chosen at query time (`--tz`, then
//! `SILLOK_TZ`, then the system zone).

use std::str::FromStr;

use chrono::{Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// Naive timestamp layouts accepted after RFC 3339.
const NAIVE_FORMATS: [&str; 4] = [
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M",
];

/// A resolved IANA timezone.
#[derive(Debug, Clone, Copy)]
pub struct Zone {
    tz: Tz,
}

impl Zone {
    /// UTC, used when the system zone cannot be determined.
    pub fn utc() -> Self {
        Self { tz: Tz::UTC }
    }

    /// Resolves an explicit zone name, or the system zone when absent.
    ///
    /// Returns a warning instead of failing when the system zone is unknown,
    /// because day views must still work on minimal containers.
    pub fn resolve(name: Option<&str>) -> Result<(Self, Option<String>), SillokError> {
        match name {
            Some(raw) => match Tz::from_str(raw.trim()) {
                Ok(tz) => Ok((Self { tz }, None)),
                Err(error) => Err(SillokError::invalid(
                    "invalid_timezone",
                    format!("invalid timezone `{raw}`: {error}"),
                )),
            },
            None => match iana_time_zone::get_timezone() {
                Ok(system) => match Tz::from_str(&system) {
                    Ok(tz) => Ok((Self { tz }, None)),
                    Err(_) => Ok((
                        Self::utc(),
                        Some(format!("system timezone `{system}` is unknown; using UTC")),
                    )),
                },
                Err(error) => Ok((
                    Self::utc(),
                    Some(format!(
                        "could not read system timezone ({error}); using UTC"
                    )),
                )),
            },
        }
    }

    /// Returns the IANA name.
    pub fn name(&self) -> &'static str {
        self.tz.name()
    }

    /// Parses RFC 3339 (offset kept) or a naive local time in this zone.
    pub fn parse_instant(&self, raw: &str) -> Result<Timestamp, SillokError> {
        let trimmed = raw.trim();
        if let Ok(value) = Timestamp::parse_rfc3339(trimmed) {
            return Ok(value);
        }
        for format in NAIVE_FORMATS {
            if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, format) {
                return self.local_to_instant(naive, trimmed);
            }
        }
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            return self.start_of(date);
        }
        Err(SillokError::invalid(
            "invalid_timestamp",
            format!("invalid timestamp `{raw}`; use RFC 3339 or YYYY-MM-DDTHH:MM:SS"),
        ))
    }

    /// Parses a `YYYY-MM-DD` calendar date.
    pub fn parse_date(&self, raw: &str) -> Result<NaiveDate, SillokError> {
        match NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d") {
            Ok(date) => Ok(date),
            Err(error) => Err(SillokError::invalid(
                "invalid_date",
                format!("invalid date `{raw}`: {error}"),
            )),
        }
    }

    /// Returns the local calendar date of an instant.
    pub fn date_of(&self, ts: Timestamp) -> Result<NaiveDate, SillokError> {
        match ts.to_datetime() {
            Ok(value) => Ok(value.with_timezone(&self.tz).date_naive()),
            Err(error) => Err(error),
        }
    }

    /// Returns the half-open instant window `[start, end)` of a local date.
    pub fn day_window(&self, date: NaiveDate) -> Result<(Timestamp, Timestamp), SillokError> {
        let start = match self.start_of(date) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let next = match date.checked_add_signed(Duration::days(1)) {
            Some(value) => value,
            None => {
                return Err(SillokError::invalid(
                    "invalid_date",
                    format!("date `{date}` is out of range"),
                ));
            }
        };
        match self.start_of(next) {
            Ok(end) => Ok((start, end)),
            Err(error) => Err(error),
        }
    }

    /// Formats an instant as local wall-clock time for human output.
    pub fn format_human(&self, ts: Timestamp) -> String {
        match ts.to_datetime() {
            Ok(value) => value
                .with_timezone(&self.tz)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            Err(_) => ts.to_rfc3339(),
        }
    }

    /// First instant of a local date. Zones that skip midnight for DST (for
    /// example America/Santiago) start the day at the first valid hour.
    fn start_of(&self, date: NaiveDate) -> Result<Timestamp, SillokError> {
        for hour in 0..24 {
            let time = match NaiveTime::from_hms_opt(hour, 0, 0) {
                Some(value) => value,
                None => continue,
            };
            match self.tz.from_local_datetime(&date.and_time(time)) {
                LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                    return Ok(Timestamp::from_datetime(value.to_utc()));
                }
                LocalResult::None => continue,
            }
        }
        Err(SillokError::invalid(
            "invalid_date",
            format!("date `{date}` has no valid local time in {}", self.name()),
        ))
    }

    fn local_to_instant(&self, naive: NaiveDateTime, raw: &str) -> Result<Timestamp, SillokError> {
        match self.tz.from_local_datetime(&naive) {
            LocalResult::Single(value) => Ok(Timestamp::from_datetime(value.to_utc())),
            LocalResult::Ambiguous(_, _) => Err(SillokError::invalid(
                "ambiguous_timestamp",
                format!("`{raw}` is ambiguous in {}; add an offset", self.name()),
            )),
            LocalResult::None => Err(SillokError::invalid(
                "invalid_timestamp",
                format!("`{raw}` does not exist in {}", self.name()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Zone;

    fn zone(name: &str) -> Zone {
        match Zone::resolve(Some(name)) {
            Ok((value, _)) => value,
            Err(_) => Zone::utc(),
        }
    }

    #[test]
    fn day_window_spans_a_dst_change() -> Result<(), crate::error::SillokError> {
        let denver = zone("America/Denver");
        let date = match denver.parse_date("2026-03-08") {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let (start, end) = match denver.day_window(date) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(end.as_millis() - start.as_millis(), 23 * 3_600_000);
        Ok(())
    }

    #[test]
    fn naive_times_use_the_zone() {
        let seoul = zone("Asia/Seoul");
        let parsed = seoul.parse_instant("2026-05-13T21:30:00");
        assert!(matches!(parsed, Ok(ts) if ts.to_rfc3339() == "2026-05-13T12:30:00.000Z"));
    }

    #[test]
    fn rfc3339_keeps_its_offset() {
        let seoul = zone("Asia/Seoul");
        let parsed = seoul.parse_instant("2026-05-13T21:30:00Z");
        assert!(matches!(parsed, Ok(ts) if ts.to_rfc3339() == "2026-05-13T21:30:00.000Z"));
    }

    #[test]
    fn nonexistent_local_time_is_rejected() {
        let denver = zone("America/Denver");
        let parsed = denver.parse_instant("2026-03-08T02:30:00");
        assert!(matches!(parsed, Err(error) if error.code() == "invalid_timestamp"));
    }

    #[test]
    fn unknown_zone_is_rejected() {
        let parsed = Zone::resolve(Some("Mars/Olympus"));
        assert!(matches!(parsed, Err(error) if error.code() == "invalid_timezone"));
    }
}
