use std::fmt::{Display, Formatter};
use std::str::FromStr;

use bitcode::{Decode, Encode};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::SillokError;

/// UUIDv7-backed id used for archive, event, day, task, and objective records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct ChronicleId([u8; 16]);

impl ChronicleId {
    /// Generates a new UUIDv7 id.
    pub fn new_v7() -> Self {
        Self(*Uuid::now_v7().as_bytes())
    }

    /// Builds an id from raw UUID bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Parses raw UUID bytes into a chronicle id.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, SillokError> {
        let parsed: [u8; 16] = match bytes.try_into() {
            Ok(value) => value,
            Err(_) => {
                return Err(SillokError::new(
                    "invalid_id",
                    format!("expected 16 UUID bytes, got {}", bytes.len()),
                ));
            }
        };
        Ok(Self(parsed))
    }

    /// Parses a UUID string into a chronicle id.
    pub fn parse(input: &str) -> Result<Self, SillokError> {
        match Uuid::from_str(input) {
            Ok(uuid) => Ok(Self(*uuid.as_bytes())),
            Err(error) => Err(SillokError::new(
                "invalid_id",
                format!("invalid UUID `{input}`: {error}"),
            )),
        }
    }

    /// Converts the id into a uuid crate value.
    pub fn as_uuid(&self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    /// Returns raw UUID bytes for compact database storage.
    pub fn as_bytes(&self) -> [u8; 16] {
        self.0
    }

    /// Returns raw UUID bytes as an owned vector for SQL parameters.
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl Display for ChronicleId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_uuid())
    }
}

impl Serialize for ChronicleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ChronicleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match Uuid::from_str(&raw) {
            Ok(uuid) => Ok(Self(*uuid.as_bytes())),
            Err(error) => Err(D::Error::custom(error.to_string())),
        }
    }
}
