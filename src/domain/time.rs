use std::fmt::{Display, Formatter};
use std::str::FromStr;

use bitcode::{Decode, Encode};
use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::SillokError;

/// Millisecond UTC timestamp. Milliseconds keep storage compact and sorting cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Returns the current UTC timestamp.
    pub fn now() -> Self {
        Self(Utc::now().timestamp_millis())
    }

    /// Converts a chrono timestamp into the compact representation.
    pub fn from_datetime(value: DateTime<Utc>) -> Self {
        Self(value.timestamp_millis())
    }

    /// Builds a timestamp from raw milliseconds.
    pub fn from_millis(value: i64) -> Self {
        Self(value)
    }

    /// Returns raw milliseconds since the Unix epoch.
    pub fn as_millis(&self) -> i64 {
        self.0
    }

    /// Converts into chrono UTC time.
    pub fn to_utc(self) -> Result<DateTime<Utc>, SillokError> {
        match DateTime::<Utc>::from_timestamp_millis(self.0) {
            Some(value) => Ok(value),
            None => Err(SillokError::new(
                "invalid_timestamp",
                format!("timestamp milliseconds out of range: {}", self.0),
            )),
        }
    }

    /// Formats the timestamp as RFC3339 for stable JSON output.
    pub fn to_rfc3339(self) -> String {
        match self.to_utc() {
            Ok(value) => value.to_rfc3339(),
            Err(_) => self.0.to_string(),
        }
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match DateTime::parse_from_rfc3339(&raw) {
            Ok(value) => Ok(Self::from_datetime(value.with_timezone(&Utc))),
            Err(error) => Err(D::Error::custom(error.to_string())),
        }
    }
}

/// Day key derived from an event timestamp and the selected timezone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Decode, Encode)]
pub struct DayKey {
    pub date: String,
    pub timezone: String,
}

/// Timezone selector used for day attribution and naive timestamp parsing.
#[derive(Debug, Clone)]
pub enum ZoneChoice {
    Local,
    Named(Tz),
}

impl ZoneChoice {
    /// Parses an optional timezone name. Absence means system-local time.
    pub fn parse(value: Option<&str>) -> Result<Self, SillokError> {
        match value {
            Some(raw) => match Tz::from_str(raw) {
                Ok(tz) => Ok(Self::Named(tz)),
                Err(error) => Err(SillokError::new(
                    "invalid_timezone",
                    format!("invalid timezone `{raw}`: {error}"),
                )),
            },
            None => Ok(Self::Local),
        }
    }

    /// Returns a stable label for persisted day records.
    pub fn label(&self) -> String {
        match self {
            Self::Local => "local".to_string(),
            Self::Named(tz) => tz.to_string(),
        }
    }

    /// Derives the local calendar day for a timestamp.
    pub fn day_key(&self, timestamp: Timestamp) -> Result<DayKey, SillokError> {
        let utc = timestamp.to_utc()?;
        let date = match self {
            Self::Local => utc.with_timezone(&Local).date_naive(),
            Self::Named(tz) => utc.with_timezone(tz).date_naive(),
        };
        Ok(self.day_key_for_date(date))
    }

    /// Builds a day key for an explicit local date.
    pub fn day_key_for_date(&self, date: NaiveDate) -> DayKey {
        DayKey {
            date: date.format("%Y-%m-%d").to_string(),
            timezone: self.label(),
        }
    }

    /// Parses a timestamp. RFC3339 inputs keep their offset; naive inputs use this zone.
    pub fn parse_timestamp(&self, raw: &str) -> Result<Timestamp, SillokError> {
        if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
            return Ok(Timestamp::from_datetime(value.with_timezone(&Utc)));
        }

        let naive = match NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
            Ok(value) => value,
            Err(_) => match NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
                Ok(value) => value,
                Err(error) => {
                    return Err(SillokError::new(
                        "invalid_timestamp",
                        format!("invalid timestamp `{raw}`: {error}"),
                    ));
                }
            },
        };

        match self {
            Self::Local => match Local.from_local_datetime(&naive) {
                LocalResult::Single(value) => {
                    Ok(Timestamp::from_datetime(value.with_timezone(&Utc)))
                }
                LocalResult::Ambiguous(_, _) => Err(SillokError::new(
                    "ambiguous_timestamp",
                    format!("timestamp `{raw}` is ambiguous in {}", self.label()),
                )),
                LocalResult::None => Err(SillokError::new(
                    "invalid_timestamp",
                    format!("timestamp `{raw}` does not exist in {}", self.label()),
                )),
            },
            Self::Named(tz) => match tz.from_local_datetime(&naive) {
                LocalResult::Single(value) => {
                    Ok(Timestamp::from_datetime(value.with_timezone(&Utc)))
                }
                LocalResult::Ambiguous(_, _) => Err(SillokError::new(
                    "ambiguous_timestamp",
                    format!("timestamp `{raw}` is ambiguous in {}", self.label()),
                )),
                LocalResult::None => Err(SillokError::new(
                    "invalid_timestamp",
                    format!("timestamp `{raw}` does not exist in {}", self.label()),
                )),
            },
        }
    }

    /// Parses a YYYY-MM-DD date.
    pub fn parse_date(&self, raw: &str) -> Result<DayKey, SillokError> {
        match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            Ok(date) => Ok(self.day_key_for_date(date)),
            Err(error) => Err(SillokError::new(
                "invalid_date",
                format!("invalid date `{raw}`: {error}"),
            )),
        }
    }
}
