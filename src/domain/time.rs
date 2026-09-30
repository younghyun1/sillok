//! Millisecond UTC instants.
//!
//! Instants are stored as integer milliseconds and exchanged as RFC 3339 UTC
//! strings with millisecond precision (`2026-09-30T01:40:35.773Z`), which keeps
//! event JSON byte-stable and readable in Git diffs.

use std::fmt::{Display, Formatter};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::SillokError;

/// Millisecond-precision UTC instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Returns the current instant.
    pub fn now() -> Self {
        Self(Utc::now().timestamp_millis())
    }

    /// Wraps raw milliseconds since the Unix epoch.
    pub fn from_millis(value: i64) -> Self {
        Self(value)
    }

    /// Returns raw milliseconds since the Unix epoch.
    pub fn as_millis(self) -> i64 {
        self.0
    }

    /// Converts a chrono instant, truncating to milliseconds.
    pub fn from_datetime(value: DateTime<Utc>) -> Self {
        Self(value.timestamp_millis())
    }

    /// Converts into a chrono instant.
    pub fn to_datetime(self) -> Result<DateTime<Utc>, SillokError> {
        match DateTime::<Utc>::from_timestamp_millis(self.0) {
            Some(value) => Ok(value),
            None => Err(SillokError::invalid(
                "invalid_timestamp",
                format!("timestamp out of range: {} ms", self.0),
            )),
        }
    }

    /// Parses an RFC 3339 instant with any offset.
    pub fn parse_rfc3339(raw: &str) -> Result<Self, SillokError> {
        match DateTime::parse_from_rfc3339(raw) {
            Ok(value) => Ok(Self::from_datetime(value.with_timezone(&Utc))),
            Err(error) => Err(SillokError::invalid(
                "invalid_timestamp",
                format!("invalid timestamp `{raw}`: {error}"),
            )),
        }
    }

    /// Formats as RFC 3339 UTC with milliseconds; out-of-range values fall
    /// back to raw milliseconds so rendering never fails.
    pub fn to_rfc3339(self) -> String {
        match self.to_datetime() {
            Ok(value) => value.to_rfc3339_opts(SecondsFormat::Millis, true),
            Err(_) => self.0.to_string(),
        }
    }

    /// Returns the `YYYY-MM` UTC month used to bucket sync files.
    pub fn utc_month(self) -> String {
        match self.to_datetime() {
            Ok(value) => value.format("%Y-%m").to_string(),
            Err(_) => "invalid".to_string(),
        }
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = match <std::borrow::Cow<'de, str>>::deserialize(deserializer) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        match DateTime::parse_from_rfc3339(&raw) {
            Ok(value) => Ok(Self::from_datetime(value.with_timezone(&Utc))),
            Err(error) => Err(D::Error::custom(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Timestamp;

    #[test]
    fn rfc3339_is_utc_with_millis() {
        let ts = Timestamp::from_millis(1_790_732_435_773);
        assert_eq!(ts.to_rfc3339(), "2026-09-30T01:40:35.773Z");
        assert_eq!(ts.utc_month(), "2026-09");
    }

    #[test]
    fn parse_accepts_offsets() {
        let parsed = Timestamp::parse_rfc3339("2026-09-29T19:40:35.773-06:00");
        assert!(matches!(parsed, Ok(ts) if ts.as_millis() == 1_790_732_435_773));
    }

    #[test]
    fn serde_roundtrip() -> Result<(), serde_json::Error> {
        let ts = Timestamp::from_millis(42);
        let encoded = match serde_json::to_string(&ts) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let decoded: Timestamp = match serde_json::from_str(&encoded) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(decoded, ts);
        Ok(())
    }
}
