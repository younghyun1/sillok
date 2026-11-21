//! Resolving an abbreviated record id.
//!
//! Ids are UUIDs, 36 characters that agents pay for on every command. A
//! prefix that names one record is accepted wherever an id is, like an
//! abbreviated Git hash. The leading characters of a UUIDv7 are its
//! timestamp, so records created close together need a longer prefix.

use crate::domain::id::RecordId;
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;

/// Shortest accepted prefix, in hex digits.
pub const MIN_PREFIX_DIGITS: usize = 8;

/// The hex digits of `raw` when it is a usable id prefix: only hex digits
/// and hyphens, at least `MIN_PREFIX_DIGITS` and fewer than a full id.
pub fn prefix_digits(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    let digits: String = trimmed
        .chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    match (MIN_PREFIX_DIGITS..32).contains(&digits.len()) {
        true => Some(digits),
        false => None,
    }
}

/// Lowest and highest id beginning with `digits`.
fn bounds(digits: &str) -> ([u8; 16], [u8; 16]) {
    let mut low = [0u8; 16];
    let mut high = [0xffu8; 16];
    for (index, digit) in digits.bytes().enumerate().take(32) {
        let value = match digit {
            b'0'..=b'9' => digit - b'0',
            _ => digit - b'a' + 10,
        };
        let (byte, shift) = (index / 2, if index % 2 == 0 { 4 } else { 0 });
        low[byte] |= value << shift;
        high[byte] = (high[byte] & !(0x0f << shift)) | (value << shift);
    }
    (low, high)
}

impl Store {
    /// The one record whose id starts with `digits` (from `prefix_digits`).
    /// A range scan of the primary key, stopped at the second match.
    pub fn resolve_prefix(&self, raw: &str, digits: &str) -> Result<RecordId, SillokError> {
        let (low, high) = bounds(digits);
        let mut statement = match self.conn.prepare_cached(
            "SELECT record_id FROM record WHERE record_id BETWEEN ?1 AND ?2 LIMIT 2",
        ) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let rows = match statement.query_map([low.to_vec(), high.to_vec()], |row| {
            row.get::<_, Vec<u8>>(0)
        }) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let mut found = Vec::with_capacity(2);
        for row in rows {
            match row {
                Ok(bytes) => found.push(bytes),
                Err(error) => return Err(error.into()),
            }
        }
        match found.as_slice() {
            [only] => RecordId::from_slice(only),
            [] => Err(SillokError::RecordNotFound(raw.to_string())),
            _ => Err(SillokError::invalid(
                "ambiguous_id",
                format!("id prefix `{raw}` matches more than one record; use more characters"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bounds, prefix_digits};

    #[test]
    fn prefixes_are_hex_and_bounded() {
        assert_eq!(
            prefix_digits("01A1fd46-a231"),
            Some("01a1fd46a231".to_string())
        );
        assert_eq!(prefix_digits("01a1fd4"), None);
        assert_eq!(prefix_digits("not-an-id"), None);
        assert_eq!(prefix_digits(&"a".repeat(32)), None);
    }

    #[test]
    fn bounds_cover_exactly_the_prefix() {
        let (low, high) = bounds("01a1fd46a");
        assert_eq!(&low[..5], &[0x01, 0xa1, 0xfd, 0x46, 0xa0]);
        assert_eq!(&high[..5], &[0x01, 0xa1, 0xfd, 0x46, 0xaf]);
        assert!(low[5..].iter().all(|byte| *byte == 0));
        assert!(high[5..].iter().all(|byte| *byte == 0xff));
    }
}
