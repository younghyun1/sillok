//! Distinct UUIDv7 identifier types.
//!
//! Records, events, and archives each get their own type so a record id can
//! never be passed where an event id is expected. All three serialize as
//! hyphenated UUID strings and persist as 16-byte blobs.

use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::SillokError;

/// Namespace for ids derived deterministically during legacy import.
const DERIVED_NAMESPACE: Uuid = Uuid::from_u128(0x5d1c_0c7e_2b6f_4f4a_9a37_61c0_5e11_0c0d);

macro_rules! chronicle_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Generates a new time-ordered UUIDv7.
            pub fn new_v7() -> Self {
                Self(*Uuid::now_v7().as_bytes())
            }

            /// Wraps raw UUID bytes.
            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// Parses a 16-byte SQL blob.
            pub fn from_slice(bytes: &[u8]) -> Result<Self, SillokError> {
                match <[u8; 16]>::try_from(bytes) {
                    Ok(value) => Ok(Self(value)),
                    Err(_) => Err(SillokError::datashape(
                        "invalid_datashape",
                        format!("expected 16 id bytes, got {}", bytes.len()),
                    )),
                }
            }

            /// Parses a hyphenated or simple UUID string.
            pub fn parse(input: &str) -> Result<Self, SillokError> {
                match Uuid::from_str(input.trim()) {
                    Ok(uuid) => Ok(Self(*uuid.as_bytes())),
                    Err(error) => Err(SillokError::invalid(
                        "invalid_id",
                        format!("invalid id `{input}`: {error}"),
                    )),
                }
            }

            /// Returns the raw bytes for blob storage.
            pub fn as_bytes(&self) -> [u8; 16] {
                self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(&Uuid::from_bytes(self.0).hyphenated(), f)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = match <std::borrow::Cow<'de, str>>::deserialize(deserializer) {
                    Ok(value) => value,
                    Err(error) => return Err(error),
                };
                match Uuid::from_str(&raw) {
                    Ok(uuid) => Ok(Self(*uuid.as_bytes())),
                    Err(error) => Err(D::Error::custom(error)),
                }
            }
        }
    };
}

chronicle_id!(
    /// Identifier of an objective or task.
    RecordId
);
chronicle_id!(
    /// Identifier of one immutable event.
    EventId
);
chronicle_id!(
    /// Identifier of a chronicle archive (one per store lineage).
    ArchiveId
);

impl EventId {
    /// Derives a stable event id from another id and a purpose tag.
    ///
    /// Legacy import sometimes splits one 0.10 event into two 1.0 events;
    /// the second needs an id that every machine derives identically so
    /// independently migrated replicas still converge.
    pub fn derived(source: [u8; 16], purpose: &str) -> Self {
        let mut name = Vec::with_capacity(16 + purpose.len());
        name.extend_from_slice(&source);
        name.extend_from_slice(purpose.as_bytes());
        Self(*Uuid::new_v5(&DERIVED_NAMESPACE, &name).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{EventId, RecordId};

    #[test]
    fn parse_roundtrips_display() {
        let id = RecordId::new_v7();
        let parsed = RecordId::parse(&id.to_string());
        assert!(matches!(parsed, Ok(value) if value == id));
    }

    #[test]
    fn parse_rejects_garbage() {
        let parsed = RecordId::parse("not-a-uuid");
        assert!(matches!(parsed, Err(error) if error.code() == "invalid_id"));
    }

    #[test]
    fn from_slice_rejects_wrong_length() {
        assert!(RecordId::from_slice(&[0u8; 15]).is_err());
    }

    #[test]
    fn derived_ids_are_deterministic_and_distinct() {
        let source = [7u8; 16];
        assert_eq!(
            EventId::derived(source, "retract"),
            EventId::derived(source, "retract")
        );
        assert_ne!(
            EventId::derived(source, "retract"),
            EventId::derived(source, "amend")
        );
    }
}
